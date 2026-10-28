use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use indexmap::IndexMap;

use super::{
    ExecSpec, ExecStatus, GuestInfo, GuestPath, OutputSink, OutputStream, Runner, RunnerError,
    Session, SessionSpec, resolve_process_env,
};
use crate::RuntimeEnv;
use crate::execution::spawn_process;

/// A runner that executes commands directly on the local machine.
///
/// Each session applies the [`SessionSpec::env_isolation`] it was started
/// with against the runner's captured [`RuntimeEnv`].
pub struct LocalRunner {
    runtime: RuntimeEnv,
}

impl LocalRunner {
    /// Creates a local runner that captures the current process environment.
    pub fn new() -> Self {
        Self {
            runtime: RuntimeEnv::current(),
        }
    }

    /// Overrides the captured runtime environment.
    #[must_use]
    pub fn with_runtime(mut self, runtime: RuntimeEnv) -> Self {
        self.runtime = runtime;
        self
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
        self.runtime.process_platform()
    }

    async fn check_usable(&self) -> Result<(), RunnerError> {
        Ok(())
    }

    async fn probe(&self) -> Result<GuestInfo, RunnerError> {
        Ok(GuestInfo {
            platform: self.runtime.process_platform(),
        })
    }

    async fn start_session(&self, spec: SessionSpec) -> Result<Box<dyn Session>, RunnerError> {
        Ok(Box::new(LocalSession {
            runtime: self.runtime.clone(),
            spec,
        }))
    }
}

struct LocalSession {
    runtime: RuntimeEnv,
    spec: SessionSpec,
}

#[async_trait]
impl Session for LocalSession {
    async fn exec(
        &mut self,
        spec: ExecSpec,
        sink: &mut dyn OutputSink,
    ) -> Result<ExecStatus, RunnerError> {
        let (program, args) = spec.argv.split_first().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot execute an empty argv",
            )
        })?;
        let process_env = resolve_process_env(
            self.spec.env_isolation,
            &spec.env,
            &IndexMap::new(),
            &self.runtime,
        );
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

        Ok(process.wait().await?.into())
    }

    async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecStatus, RunnerError> {
        let (program, args) = spec.argv.split_first().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot execute an empty argv",
            )
        })?;
        let status = tokio::process::Command::new(program)
            .args(args)
            .envs(&spec.env)
            .current_dir(&spec.cwd.0)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?
            .wait()
            .await?;

        Ok(status.into())
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
    use std::path::Path;
    #[cfg(unix)]
    use std::path::PathBuf;
    use std::sync::Arc;

    use indexmap::IndexMap;
    use rattler_conda_types::Subdir;

    use super::*;
    use crate::EnvironmentIsolation;
    use crate::runner::test_support::VecSink;
    use crate::runner::{Mounts, SessionSpec};

    fn session_spec(work_dir: &Path, env_isolation: EnvironmentIsolation) -> SessionSpec {
        SessionSpec {
            platform: Subdir::current().unwrap(),
            mounts: Mounts::new(),
            image: None,
            work_dir: GuestPath(work_dir.to_path_buf()),
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
                },
                &mut sink,
            )
            .await
            .unwrap();

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
                },
                &mut sink,
            )
            .await
            .unwrap();

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
                },
                &mut sink,
            )
            .await
            .unwrap();

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
                },
                &mut sink,
            ),
        )
        .await
        .expect("reading invalid output must not hang")
        .unwrap();

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

    #[cfg(unix)]
    #[tokio::test]
    async fn exec_interactive_reports_exit_status_and_inherits_host_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new();
        let mut session =
            start_local_session(&runner, temp_dir.path(), EnvironmentIsolation::Strict).await;

        let status = session
            .exec_interactive(ExecSpec {
                argv: shell_command("test -n \"$HOME\""),
                cwd: GuestPath(PathBuf::from(temp_dir.path())),
                env: IndexMap::new(),
            })
            .await
            .unwrap();

        assert!(status.success());

        let status = session
            .exec_interactive(ExecSpec {
                argv: shell_command("exit 3"),
                cwd: GuestPath(PathBuf::from(temp_dir.path())),
                env: IndexMap::new(),
            })
            .await
            .unwrap();

        assert_eq!(status.code, Some(3));
    }
}
