use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use indexmap::IndexMap;
use tempfile::{TempDir, TempPath};
use tokio::io::AsyncReadExt;

use super::{
    CHANNEL_SIZE_LIMIT, ExecOutcome, ExecSpec, ExecStatus, GuestInfo, GuestPath, OutputSink,
    OutputStream, Runner, RunnerError, Session, SessionSpec, resolve_process_env,
};
use crate::RuntimeEnv;
use crate::execution::spawn_process;

/// The name prefix of the directory a session creates inside
/// [`SessionSpec::scratch_dir`] for its own files.
pub(super) const SESSION_DIRECTORY_PREFIX: &str = "rattler-build-session-";

/// A runner that executes commands directly on the local machine.
///
/// The process environment is captured when a session starts, not when the
/// runner is created: a runner held in a long-lived configuration runs every
/// session against the environment of that moment. Each session applies the
/// [`SessionSpec::env_isolation`] it was started with against that captured
/// [`RuntimeEnv`], for streamed and interactive commands alike. A runtime
/// injected with [`LocalRunner::with_runtime`] replaces the capture for every
/// session.
///
/// Channel files (see [`ExecSpec::channels`]) live in a directory the session
/// creates inside [`SessionSpec::scratch_dir`] and removes when it is
/// dropped.
pub struct LocalRunner {
    runtime: Option<RuntimeEnv>,
    channel_size_limit: u64,
}

impl LocalRunner {
    /// Creates a local runner that captures the process environment whenever
    /// a session starts.
    pub fn new() -> Self {
        Self {
            runtime: None,
            channel_size_limit: CHANNEL_SIZE_LIMIT,
        }
    }

    /// Uses `runtime` for every session instead of capturing the process
    /// environment.
    #[must_use]
    pub fn with_runtime(mut self, runtime: RuntimeEnv) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Fails commands whose channel files grow beyond `limit` bytes instead
    /// of [`CHANNEL_SIZE_LIMIT`].
    #[cfg(test)]
    #[must_use]
    fn with_channel_size_limit(mut self, limit: u64) -> Self {
        self.channel_size_limit = limit;
        self
    }

    /// The runtime a session started now runs against.
    fn runtime(&self) -> RuntimeEnv {
        self.runtime.clone().unwrap_or_else(RuntimeEnv::current)
    }

    /// Starts a session according to `spec` and returns it as the concrete
    /// type, so a runner that wraps this one can learn the directory the
    /// session keeps its channel files in.
    pub(super) fn start_local_session(
        &self,
        spec: SessionSpec,
    ) -> Result<LocalSession, RunnerError> {
        let channel_dir = tempfile::Builder::new()
            .prefix(SESSION_DIRECTORY_PREFIX)
            .tempdir_in(&spec.scratch_dir.0)?;
        Ok(LocalSession {
            runtime: self.runtime(),
            spec,
            channel_dir,
            channel_size_limit: self.channel_size_limit,
        })
    }
}

impl Default for LocalRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runner for LocalRunner {
    fn name(&self) -> &str {
        "local"
    }

    fn execution_platform(&self) -> rattler_conda_types::Subdir {
        self.runtime().process_platform()
    }

    async fn check_usable(&self) -> Result<(), RunnerError> {
        Ok(())
    }

    async fn probe(&self) -> Result<GuestInfo, RunnerError> {
        Ok(GuestInfo {
            platform: self.runtime().process_platform(),
        })
    }

    async fn start_session(&self, spec: SessionSpec) -> Result<Box<dyn Session>, RunnerError> {
        Ok(Box::new(self.start_local_session(spec)?))
    }
}

/// A session of [`LocalRunner`].
///
/// The session owns a channel directory inside [`SessionSpec::scratch_dir`],
/// created when the session starts and removed together with everything in
/// it when the session is dropped. Every channel file of every command the
/// session executes is created in that directory under a unique name, so the
/// directory is the one path a runner wrapping this session has to make
/// writable for channels to work.
pub(super) struct LocalSession {
    runtime: RuntimeEnv,
    spec: SessionSpec,
    channel_dir: TempDir,
    channel_size_limit: u64,
}

impl LocalSession {
    /// The spec the session was started with.
    pub(super) fn spec(&self) -> &SessionSpec {
        &self.spec
    }

    /// The directory holding the channel files of the session's commands.
    pub(super) fn channel_dir(&self) -> &Path {
        self.channel_dir.path()
    }

    /// Resolves the process environment of `spec` through the session's
    /// isolation mode and creates the channel files of `spec`, whose paths
    /// are set in the returned environment on top of the resolved one.
    fn prepare(
        &self,
        spec: &ExecSpec,
    ) -> Result<(IndexMap<String, String>, ChannelFiles), RunnerError> {
        let mut process_env = resolve_process_env(
            self.spec.env_isolation,
            &spec.env,
            &IndexMap::new(),
            &self.runtime,
        );
        let channels = ChannelFiles::create(self.channel_dir.path(), &spec.channels)?;
        for (variable, path) in channels.paths() {
            process_env.insert(variable.clone(), path.to_string_lossy().into_owned());
        }
        Ok((process_env, channels))
    }
}

/// The channel files of one command execution, removed when dropped.
struct ChannelFiles {
    files: IndexMap<String, TempPath>,
}

impl ChannelFiles {
    /// Creates one empty file with a unique name in `directory` for every
    /// distinct variable.
    fn create(directory: &Path, variables: &[String]) -> Result<Self, RunnerError> {
        let mut files = IndexMap::with_capacity(variables.len());
        for variable in variables {
            if files.contains_key(variable) {
                continue;
            }
            let file = tempfile::Builder::new()
                .prefix(&format!("{variable}-"))
                .tempfile_in(directory)?;
            files.insert(variable.clone(), file.into_temp_path());
        }
        Ok(Self { files })
    }

    /// The file of every variable, in declaration order.
    fn paths(&self) -> impl Iterator<Item = (&String, &Path)> {
        self.files
            .iter()
            .map(|(variable, path)| (variable, path.as_ref()))
    }

    /// Reads every file back, keyed by variable, after the command exited
    /// with `status`: a missing file reads as empty and a file holding more
    /// than `limit` bytes fails with [`RunnerError::ChannelTooLarge`].
    async fn collect(
        self,
        limit: u64,
        status: ExecStatus,
    ) -> Result<IndexMap<String, Vec<u8>>, RunnerError> {
        let mut channels = IndexMap::with_capacity(self.files.len());
        for (variable, path) in self.files {
            let contents = read_channel_file(&variable, &path, limit, status).await?;
            channels.insert(variable, contents);
        }
        Ok(channels)
    }
}

/// Reads the channel file of `variable` at `path` according to
/// [`ChannelFiles::collect`]. At most `limit + 1` bytes are read, so the
/// bound holds whatever the file's size claims and whatever keeps writing to
/// it after the command exited.
async fn read_channel_file(
    variable: &str,
    path: &Path,
    limit: u64,
    status: ExecStatus,
) -> Result<Vec<u8>, RunnerError> {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(channel_io_error(variable, path, error)),
    };
    let mut contents = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut contents)
        .await
        .map_err(|error| channel_io_error(variable, path, error))?;
    if u64::try_from(contents.len()).is_ok_and(|size| size > limit) {
        return Err(RunnerError::ChannelTooLarge {
            variable: variable.to_string(),
            limit,
            status,
        });
    }
    Ok(contents)
}

/// Attributes an I/O error on a channel file to its variable and path.
fn channel_io_error(variable: &str, path: &Path, error: std::io::Error) -> RunnerError {
    RunnerError::Io(std::io::Error::new(
        error.kind(),
        format!(
            "failed to read the channel file of {variable} at {}: {error}",
            path.display()
        ),
    ))
}

#[async_trait]
impl Session for LocalSession {
    async fn exec(
        &mut self,
        spec: ExecSpec,
        sink: &mut dyn OutputSink,
    ) -> Result<ExecOutcome, RunnerError> {
        let (program, args) = spec.argv.split_first().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot execute an empty argv",
            )
        })?;
        let (process_env, channels) = self.prepare(&spec)?;
        let mut process = spawn_process(OsStr::new(program), args, &spec.cwd.0, &process_env)?;

        while let Some(line) = process.next_line().await {
            match line {
                Ok(line) => {
                    let stream = if line.is_stderr {
                        OutputStream::Stderr
                    } else {
                        OutputStream::Stdout
                    };
                    sink.line(stream, &line.text);
                }
                Err(error) => {
                    tracing::warn!("Error reading output: {:?}", error);
                    if let Err(error) = process.drain_output().await {
                        tracing::warn!("Error draining output: {:?}", error);
                    }
                    break;
                }
            }
        }

        let status = process.wait().await?.into();

        Ok(ExecOutcome {
            status,
            channels: channels.collect(self.channel_size_limit, status).await?,
        })
    }

    async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecOutcome, RunnerError> {
        let (program, args) = spec.argv.split_first().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot execute an empty argv",
            )
        })?;
        let (process_env, channels) = self.prepare(&spec)?;
        let status = tokio::process::Command::new(program)
            .args(args)
            .env_clear()
            .envs(&process_env)
            .current_dir(&spec.cwd.0)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?
            .wait()
            .await?;

        let status = status.into();

        Ok(ExecOutcome {
            status,
            channels: channels.collect(self.channel_size_limit, status).await?,
        })
    }

    async fn find_executable(
        &mut self,
        names: &[String],
        search_paths: &[GuestPath],
    ) -> Result<Option<GuestPath>, RunnerError> {
        let suffix = self.runtime.exe_suffix();
        for name in names {
            for search_path in search_paths {
                let executable = search_path.0.join(format!("{name}{suffix}"));
                if is_executable_file(&executable, self.runtime.process_platform()) {
                    return Ok(Some(GuestPath(executable)));
                }
            }
        }

        Ok(None)
    }
}

fn is_executable_file(path: &Path, _platform: rattler_conda_types::Subdir) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    if !_platform.is_windows() {
        use std::os::unix::fs::PermissionsExt;
        return path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0);
    }

    true
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use indexmap::IndexMap;
    use rattler_conda_types::Subdir;

    use super::*;
    use crate::EnvironmentIsolation;
    use crate::runner::test_support::VecSink;
    use crate::runner::{Mounts, SessionSpec};

    /// A session spec for `work_dir`, which doubles as the scratch directory.
    fn session_spec(work_dir: &Path, env_isolation: EnvironmentIsolation) -> SessionSpec {
        SessionSpec {
            platform: Subdir::current().unwrap(),
            mounts: Mounts::new(),
            image: None,
            work_dir: GuestPath(work_dir.to_path_buf()),
            scratch_dir: GuestPath(work_dir.to_path_buf()),
            env_isolation,
        }
    }

    async fn start_local_session(
        runner: &LocalRunner,
        work_dir: &Path,
        env_isolation: EnvironmentIsolation,
    ) -> Box<dyn Session> {
        runner
            .start_session(session_spec(work_dir, env_isolation))
            .await
            .unwrap()
    }

    #[cfg(unix)]
    fn shell_command(script: &str) -> Vec<String> {
        vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()]
    }

    #[cfg(windows)]
    fn shell_command(script: &str) -> Vec<String> {
        vec![
            r"C:\Windows\System32\cmd.exe".to_string(),
            "/d".to_string(),
            "/c".to_string(),
            script.to_string(),
        ]
    }

    #[tokio::test]
    async fn local_session_execs_and_streams_lines() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let mut sink = VecSink::default();

        #[cfg(unix)]
        let argv = shell_command("echo stdout; echo stderr >&2");
        #[cfg(windows)]
        let argv = shell_command("echo stdout&>&2 echo stderr");

        let status = session
            .exec(
                ExecSpec {
                    argv,
                    cwd: GuestPath(temp_dir.path().to_path_buf()),
                    env: IndexMap::new(),
                    channels: Vec::new(),
                },
                &mut sink,
            )
            .await
            .unwrap()
            .status;

        assert!(status.success());
        assert!(
            sink.0
                .iter()
                .any(|(stream, line)| *stream == OutputStream::Stdout && line.trim() == "stdout"),
            "unexpected output: {:?}",
            sink.0
        );
        assert!(
            sink.0
                .iter()
                .any(|(stream, line)| *stream == OutputStream::Stderr && line.trim() == "stderr"),
            "unexpected output: {:?}",
            sink.0
        );
    }

    #[tokio::test]
    async fn strict_isolation_applies_resolved_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        let platform = if cfg!(windows) {
            Subdir::Win64
        } else {
            Subdir::Linux64
        };
        let runner = LocalRunner::new().with_runtime(
            RuntimeEnv::for_test(platform)
                .with_var("SSL_CERT_FILE", "allowlisted")
                .with_var("RB_RUNNER_HIDDEN", "hidden"),
        );
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::Strict).await;
        let mut sink = VecSink::default();
        let mut env = IndexMap::new();
        env.insert("RB_RUNNER_EXPLICIT".to_string(), "visible".to_string());

        #[cfg(unix)]
        let argv = vec!["/usr/bin/env".to_string()];
        #[cfg(windows)]
        let argv = shell_command("set");

        let status = session
            .exec(
                ExecSpec {
                    argv,
                    cwd: GuestPath(temp_dir.path().to_path_buf()),
                    env,
                    channels: vec!["RB_RUNNER_CHANNEL".to_string()],
                },
                &mut sink,
            )
            .await
            .unwrap()
            .status;

        assert!(status.success());
        let stdout = sink
            .0
            .iter()
            .filter(|(stream, _)| *stream == OutputStream::Stdout)
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>();
        assert!(stdout.contains(&"RB_RUNNER_EXPLICIT=visible"));
        assert!(stdout.contains(&"SSL_CERT_FILE=allowlisted"));
        assert!(
            stdout
                .iter()
                .any(|line| line.starts_with("RB_RUNNER_CHANNEL=")),
            "the channel variable must survive strict isolation: {stdout:?}"
        );
        assert!(
            !stdout
                .iter()
                .any(|line| line.starts_with("RB_RUNNER_HIDDEN=")),
            "unexpected output: {stdout:?}"
        );
    }

    /// A failing command still delivers every line of both streams, in order
    /// within each stream, together with its exit code.
    #[tokio::test]
    async fn local_session_keeps_output_on_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let mut sink = VecSink::default();

        #[cfg(unix)]
        let argv = shell_command("echo out1; echo err1 >&2; echo out2; echo err2 >&2; exit 7");
        #[cfg(windows)]
        let argv = shell_command("echo out1&1>&2 echo err1&echo out2&1>&2 echo err2&exit 7");

        let status = session
            .exec(
                ExecSpec {
                    argv,
                    cwd: GuestPath(temp_dir.path().to_path_buf()),
                    env: IndexMap::new(),
                    channels: Vec::new(),
                },
                &mut sink,
            )
            .await
            .unwrap()
            .status;

        assert_eq!(status.code, Some(7));
        let lines_on = |stream: OutputStream| {
            sink.0
                .iter()
                .filter(|(line_stream, _)| *line_stream == stream)
                .map(|(_, line)| line.trim())
                .collect::<Vec<_>>()
        };
        assert_eq!(lines_on(OutputStream::Stdout), vec!["out1", "out2"]);
        assert_eq!(lines_on(OutputStream::Stderr), vec!["err1", "err2"]);
    }

    /// Non-UTF-8 output — Windows tools writing in the active code page, or a
    /// build printing raw bytes — must not abort output capture for the rest of
    /// the script.
    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_utf8_output_is_decoded_lossily() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let mut sink = VecSink::default();

        let status = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            session.exec(
                ExecSpec {
                    argv: shell_command("printf 'before\\n\\377\\nafter\\n'; exit 7"),
                    cwd: GuestPath(temp_dir.path().to_path_buf()),
                    env: IndexMap::new(),
                    channels: Vec::new(),
                },
                &mut sink,
            ),
        )
        .await
        .expect("reading invalid output must not hang")
        .unwrap()
        .status;

        assert_eq!(status.code, Some(7));
        let stdout = sink
            .0
            .iter()
            .filter(|(stream, _)| *stream == OutputStream::Stdout)
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>();
        assert_eq!(stdout, vec!["before", "\u{FFFD}", "after"]);
    }

    #[tokio::test]
    async fn find_executable_uses_runtime_platform_suffix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let directory = temp_dir.path();
        fs_err::write(directory.join("tool"), "").unwrap();
        fs_err::write(directory.join("tool.exe"), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs_err::set_permissions(
                directory.join("tool"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        #[cfg(unix)]
        let non_executable_dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        fs_err::write(non_executable_dir.path().join("tool"), "").unwrap();
        #[cfg(unix)]
        let search_paths = vec![
            GuestPath(non_executable_dir.path().to_path_buf()),
            GuestPath(directory.to_path_buf()),
        ];
        #[cfg(not(unix))]
        let search_paths = vec![GuestPath(directory.to_path_buf())];
        let names = vec!["tool".to_string()];

        let windows_runner = LocalRunner::new().with_runtime(RuntimeEnv::for_test(Subdir::Win64));
        let mut windows_session =
            start_local_session(&windows_runner, directory, EnvironmentIsolation::Strict).await;
        assert_eq!(
            windows_session
                .find_executable(&names, &search_paths)
                .await
                .unwrap(),
            Some(GuestPath(directory.join("tool.exe")))
        );

        let linux_runner = LocalRunner::new().with_runtime(RuntimeEnv::for_test(Subdir::Linux64));
        let mut linux_session =
            start_local_session(&linux_runner, directory, EnvironmentIsolation::Strict).await;
        assert_eq!(
            linux_session
                .find_executable(&names, &search_paths)
                .await
                .unwrap(),
            Some(GuestPath(directory.join("tool")))
        );
        assert_eq!(
            linux_session
                .find_executable(&["missing".to_string()], &search_paths)
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn runner_is_object_safe() {
        let _: Arc<dyn Runner> = Arc::new(LocalRunner::default());
    }

    /// Without isolation an interactive command inherits the host
    /// environment plus the explicit variables, and its exit code is reported.
    #[cfg(unix)]
    #[tokio::test]
    async fn exec_interactive_reports_exit_status_and_inherits_host_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let mut env = IndexMap::new();
        env.insert("RB_RUNNER_EXPLICIT".to_string(), "visible".to_string());

        let status = session
            .exec_interactive(ExecSpec {
                argv: shell_command("test -n \"$HOME\" && test \"$RB_RUNNER_EXPLICIT\" = visible"),
                cwd: GuestPath(PathBuf::from(temp_dir.path())),
                env,
                channels: Vec::new(),
            })
            .await
            .unwrap()
            .status;

        assert!(status.success());

        let status = session
            .exec_interactive(ExecSpec {
                argv: shell_command("exit 3"),
                cwd: GuestPath(PathBuf::from(temp_dir.path())),
                env: IndexMap::new(),
                channels: Vec::new(),
            })
            .await
            .unwrap()
            .status;

        assert_eq!(status.code, Some(3));
    }

    /// Strict isolation applies to interactive commands as well: a host
    /// variable outside the passthrough allowlist does not reach the command.
    #[cfg(unix)]
    #[tokio::test]
    async fn exec_interactive_applies_strict_isolation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new().with_runtime(
            RuntimeEnv::for_test(Subdir::Linux64)
                .with_var("SSL_CERT_FILE", "allowlisted")
                .with_var("RB_RUNNER_HIDDEN", "hidden"),
        );
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::Strict).await;

        let status = session
            .exec_interactive(ExecSpec {
                argv: shell_command(
                    "test -z \"$RB_RUNNER_HIDDEN\" && test -z \"$HOME\" && test \"$SSL_CERT_FILE\" = allowlisted",
                ),
                cwd: GuestPath(PathBuf::from(temp_dir.path())),
                env: IndexMap::new(),
                channels: Vec::new(),
            })
            .await
            .unwrap().status;

        assert!(status.success());
    }

    /// A session runs against the runtime the runner holds when the session
    /// starts, so a runtime set after an earlier session is what later
    /// sessions see.
    #[tokio::test]
    async fn session_captures_the_runtime_at_session_start() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut early_session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let platform = if cfg!(windows) {
            Subdir::Win64
        } else {
            Subdir::Linux64
        };
        let mut late_runtime = RuntimeEnv::for_test(platform).with_var("RB_RUNNER_LATE", "late");
        if let Ok(system_root) = std::env::var("SYSTEMROOT") {
            late_runtime = late_runtime.with_var("SYSTEMROOT", system_root);
        }
        let runner = runner.with_runtime(late_runtime);
        let mut late_session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let argv = vec!["/usr/bin/env".to_string()];
        #[cfg(windows)]
        let argv = shell_command("set");
        let exec = ExecSpec {
            argv,
            cwd: GuestPath(temp_dir.path().to_path_buf()),
            env: IndexMap::new(),
            channels: Vec::new(),
        };
        let late_var = |sink: &VecSink| {
            sink.0.iter().any(|(stream, line)| {
                *stream == OutputStream::Stdout && line == "RB_RUNNER_LATE=late"
            })
        };

        let mut early_sink = VecSink::default();
        let status = early_session
            .exec(exec.clone(), &mut early_sink)
            .await
            .unwrap()
            .status;
        assert!(status.success());
        assert!(
            !late_var(&early_sink),
            "unexpected output: {:?}",
            early_sink.0
        );

        let mut late_sink = VecSink::default();
        let status = late_session
            .exec(exec, &mut late_sink)
            .await
            .unwrap()
            .status;
        assert!(status.success());
        assert!(late_var(&late_sink), "unexpected output: {:?}", late_sink.0);
    }

    /// Runs `script` in `session` with `channels` declared, in the session's
    /// work directory and without explicit variables, and returns the outcome
    /// together with the captured output.
    async fn exec_with_channels(
        session: &mut dyn Session,
        work_dir: &Path,
        script: &str,
        channels: &[&str],
    ) -> Result<(ExecOutcome, VecSink), RunnerError> {
        let mut sink = VecSink::default();
        let outcome = session
            .exec(
                ExecSpec {
                    argv: shell_command(script),
                    cwd: GuestPath(work_dir.to_path_buf()),
                    env: IndexMap::new(),
                    channels: channels.iter().map(|name| name.to_string()).collect(),
                },
                &mut sink,
            )
            .await?;
        Ok((outcome, sink))
    }

    /// The bytes of `channel` in `outcome` as text with `\r\n` normalized, so
    /// `echo` output compares equal on both platforms.
    fn channel_text(outcome: &ExecOutcome, channel: &str) -> String {
        String::from_utf8(outcome.channels[channel].clone())
            .unwrap()
            .replace("\r\n", "\n")
    }

    /// What the command appends to the file behind each channel variable
    /// comes back keyed by that variable, in declaration order: two channels
    /// are distinct files, and a channel the command never touches is empty.
    #[tokio::test]
    async fn channels_return_what_the_command_wrote() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let script = "echo one >> \"$RB_FIRST\" && echo two >> \"$RB_FIRST\" && echo other >> \"$RB_SECOND\"";
        #[cfg(windows)]
        let script = "echo one>>%RB_FIRST%&& echo two>>%RB_FIRST%&& echo other>>%RB_SECOND%";

        let (outcome, sink) = exec_with_channels(
            session.as_mut(),
            temp_dir.path(),
            script,
            &["RB_FIRST", "RB_SECOND", "RB_UNTOUCHED"],
        )
        .await
        .unwrap();

        assert!(outcome.status.success(), "unexpected output: {:?}", sink.0);
        assert_eq!(
            outcome.channels.keys().collect::<Vec<_>>(),
            vec!["RB_FIRST", "RB_SECOND", "RB_UNTOUCHED"]
        );
        assert_eq!(channel_text(&outcome, "RB_FIRST"), "one\ntwo\n");
        assert_eq!(channel_text(&outcome, "RB_SECOND"), "other\n");
        assert_eq!(outcome.channels["RB_UNTOUCHED"], Vec::<u8>::new());
    }

    /// Every execution gets a fresh, empty file: a second command in the same
    /// session appending to the same channel does not see what the first one
    /// wrote.
    #[tokio::test]
    async fn channels_start_empty_for_every_execution() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let script = "echo line >> \"$RB_LOG\"";
        #[cfg(windows)]
        let script = "echo line>>%RB_LOG%";

        for _ in 0..2 {
            let (outcome, sink) =
                exec_with_channels(session.as_mut(), temp_dir.path(), script, &["RB_LOG"])
                    .await
                    .unwrap();

            assert!(outcome.status.success(), "unexpected output: {:?}", sink.0);
            assert_eq!(channel_text(&outcome, "RB_LOG"), "line\n");
        }
    }

    /// Two sessions of one runner executing at the same time each get their
    /// own file for the same channel name: what one command writes never
    /// reaches the other, even while both files are in use at once.
    #[tokio::test]
    async fn concurrent_sessions_get_distinct_channel_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut first_session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let mut second_session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        // Each command prints its file, writes its token, waits for the other
        // command to have written as well, and writes its token again.
        #[cfg(unix)]
        let script = |token: &str| {
            format!(
                "echo \"$RB_SHARED\" && echo {token} >> \"$RB_SHARED\" && sleep 1 && echo {token} >> \"$RB_SHARED\""
            )
        };
        #[cfg(windows)]
        let script = |token: &str| {
            format!(
                "echo %RB_SHARED%& echo {token}>>%RB_SHARED%& ping -n 2 127.0.0.1>nul& echo {token}>>%RB_SHARED%"
            )
        };
        let first_script = script("first");
        let second_script = script("second");

        let (first, second) = tokio::join!(
            exec_with_channels(
                first_session.as_mut(),
                temp_dir.path(),
                &first_script,
                &["RB_SHARED"],
            ),
            exec_with_channels(
                second_session.as_mut(),
                temp_dir.path(),
                &second_script,
                &["RB_SHARED"],
            ),
        );
        let (first_outcome, first_sink) = first.unwrap();
        let (second_outcome, second_sink) = second.unwrap();

        assert!(
            first_outcome.status.success(),
            "unexpected output: {:?}",
            first_sink.0
        );
        assert!(
            second_outcome.status.success(),
            "unexpected output: {:?}",
            second_sink.0
        );
        assert_eq!(channel_text(&first_outcome, "RB_SHARED"), "first\nfirst\n");
        assert_eq!(
            channel_text(&second_outcome, "RB_SHARED"),
            "second\nsecond\n"
        );
        let printed_path = |sink: &VecSink| {
            let (_, printed) = sink.0.first().expect("the command printed the path");
            PathBuf::from(printed.trim())
        };
        assert_ne!(
            printed_path(&first_sink),
            printed_path(&second_sink),
            "both sessions handed out the same channel file"
        );
    }

    /// A failing command still hands back what it wrote before exiting.
    #[tokio::test]
    async fn channels_are_returned_when_the_command_fails() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let script = "echo partial >> \"$RB_LOG\"; exit 4";
        #[cfg(windows)]
        let script = "echo partial>>%RB_LOG%& exit 4";

        let (outcome, _) =
            exec_with_channels(session.as_mut(), temp_dir.path(), script, &["RB_LOG"])
                .await
                .unwrap();

        assert_eq!(outcome.status.code, Some(4));
        assert_eq!(channel_text(&outcome, "RB_LOG"), "partial\n");
    }

    /// A channel file larger than the runner's limit fails the execution with
    /// the variable, the limit, and the exit status of the command.
    #[tokio::test]
    async fn oversized_channel_is_a_structured_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new().with_channel_size_limit(8);
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let script = "echo 0123456789 >> \"$RB_BIG\"; exit 6";
        #[cfg(windows)]
        let script = "echo 0123456789>>%RB_BIG%& exit 6";

        let error = exec_with_channels(session.as_mut(), temp_dir.path(), script, &["RB_BIG"])
            .await
            .err()
            .expect("the channel file exceeds the limit");

        match error {
            RunnerError::ChannelTooLarge {
                variable,
                limit,
                status,
            } => {
                assert_eq!(variable, "RB_BIG");
                assert_eq!(limit, 8);
                assert_eq!(status.code, Some(6));
            }
            RunnerError::Io(_)
            | RunnerError::UnsupportedPlatform { .. }
            | RunnerError::MissingExecutable { .. }
            | RunnerError::UnsupportedMount { .. } => {
                panic!("unexpected error: {error:?}")
            }
        }
    }

    /// The channel variable is set after the explicit variables, so it wins
    /// over an explicit variable of the same name.
    #[tokio::test]
    async fn channel_variable_overrides_an_explicit_variable() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;
        let mut sink = VecSink::default();
        let mut env = IndexMap::new();
        env.insert("RB_SHADOWED".to_string(), "explicit".to_string());

        #[cfg(unix)]
        let script = "echo written >> \"$RB_SHADOWED\"";
        #[cfg(windows)]
        let script = "echo written>>%RB_SHADOWED%";

        let outcome = session
            .exec(
                ExecSpec {
                    argv: shell_command(script),
                    cwd: GuestPath(temp_dir.path().to_path_buf()),
                    env,
                    channels: vec!["RB_SHADOWED".to_string()],
                },
                &mut sink,
            )
            .await
            .unwrap();

        assert!(outcome.status.success(), "unexpected output: {:?}", sink.0);
        assert_eq!(channel_text(&outcome, "RB_SHADOWED"), "written\n");
        assert!(
            !temp_dir.path().join("explicit").exists(),
            "the command wrote to the explicit value instead of the channel file"
        );
    }

    /// A name listed twice is one channel with one file, returned once.
    #[tokio::test]
    async fn channel_listed_twice_shares_one_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let script = "echo once >> \"$RB_TWICE\"";
        #[cfg(windows)]
        let script = "echo once>>%RB_TWICE%";

        let (outcome, sink) = exec_with_channels(
            session.as_mut(),
            temp_dir.path(),
            script,
            &["RB_TWICE", "RB_TWICE"],
        )
        .await
        .unwrap();

        assert!(outcome.status.success(), "unexpected output: {:?}", sink.0);
        assert_eq!(outcome.channels.len(), 1);
        assert_eq!(channel_text(&outcome, "RB_TWICE"), "once\n");
    }

    /// An interactive command receives its channels the same way.
    #[cfg(unix)]
    #[tokio::test]
    async fn exec_interactive_returns_channels() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        let outcome = session
            .exec_interactive(ExecSpec {
                argv: shell_command("echo interactive >> \"$RB_CHANNEL\"; exit 2"),
                cwd: GuestPath(temp_dir.path().to_path_buf()),
                env: IndexMap::new(),
                channels: vec!["RB_CHANNEL".to_string()],
            })
            .await
            .unwrap();

        assert_eq!(outcome.status.code, Some(2));
        assert_eq!(channel_text(&outcome, "RB_CHANNEL"), "interactive\n");
    }

    /// The channel file lives in the session's directory inside the scratch
    /// directory, is removed once its contents are collected, and the
    /// session's directory goes away with the session.
    #[tokio::test]
    async fn channel_files_are_removed_with_the_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::None).await;

        #[cfg(unix)]
        let script = "echo \"$RB_PATH\"";
        #[cfg(windows)]
        let script = "echo %RB_PATH%";

        let (outcome, sink) =
            exec_with_channels(session.as_mut(), temp_dir.path(), script, &["RB_PATH"])
                .await
                .unwrap();

        assert!(outcome.status.success(), "unexpected output: {:?}", sink.0);
        let (_, printed) = sink.0.first().expect("the command printed the path");
        let channel_file = PathBuf::from(printed.trim());
        let channel_dir = channel_file.parent().unwrap().to_path_buf();
        assert_eq!(channel_dir.parent(), Some(temp_dir.path()));
        assert!(
            channel_dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(SESSION_DIRECTORY_PREFIX)),
            "unexpected channel directory: {}",
            channel_dir.display()
        );
        assert!(
            !channel_file.exists(),
            "the channel file outlived its collection"
        );
        assert!(
            channel_dir.is_dir(),
            "the channel directory must live as long as the session"
        );

        drop(session);

        assert!(
            !channel_dir.exists(),
            "the channel directory outlived the session"
        );
    }
}
