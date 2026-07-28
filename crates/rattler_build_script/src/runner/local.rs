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
use crate::execution::{EnvironmentIsolation, spawn_process};

/// A runner that executes commands directly on the local machine.
pub struct LocalRunner {
    isolation: EnvironmentIsolation,
    runtime: RuntimeEnv,
}

impl LocalRunner {
    /// Creates a local runner with the given environment isolation mode.
    pub fn new(isolation: EnvironmentIsolation) -> Self {
        Self {
            isolation,
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
        Self::new(EnvironmentIsolation::default())
    }
}

#[async_trait]
impl Runner for LocalRunner {
    fn name(&self) -> &str {
        "local"
    }

    fn execution_platform(&self) -> rattler_conda_types::Platform {
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
            isolation: self.isolation,
            runtime: self.runtime.clone(),
            _spec: spec,
        }))
    }
}

struct LocalSession {
    isolation: EnvironmentIsolation,
    runtime: RuntimeEnv,
    _spec: SessionSpec,
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
        let process_env =
            resolve_process_env(self.isolation, &spec.env, &IndexMap::new(), &self.runtime);
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

fn is_executable_file(path: &Path, _platform: rattler_conda_types::Platform) -> bool {
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
    use rattler_conda_types::Platform;

    use super::*;
    use crate::runner::{Mount, OutputSink, SessionSpec};

    #[derive(Default)]
    struct VecSink(Vec<(OutputStream, String)>);

    impl OutputSink for VecSink {
        fn line(&mut self, stream: OutputStream, line: &str) {
            self.0.push((stream, line.to_string()));
        }
    }

    fn session_spec(work_dir: &Path) -> SessionSpec {
        SessionSpec {
            platform: Platform::current(),
            mounts: Vec::<Mount>::new(),
            image: None,
            work_dir: GuestPath(work_dir.to_path_buf()),
        }
    }

    async fn start_local_session(runner: &LocalRunner, work_dir: &Path) -> Box<dyn Session> {
        runner.start_session(session_spec(work_dir)).await.unwrap()
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
        let runner = LocalRunner::new(EnvironmentIsolation::None);
        let mut session = start_local_session(&runner, temp_dir.path()).await;
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
            Platform::Win64
        } else {
            Platform::Linux64
        };
        let runner = LocalRunner::new(EnvironmentIsolation::Strict).with_runtime(
            RuntimeEnv::for_test(platform)
                .with_var("SSL_CERT_FILE", "allowlisted")
                .with_var("RB_RUNNER_HIDDEN", "hidden"),
        );
        let mut session = start_local_session(&runner, temp_dir.path()).await;
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

    #[cfg(unix)]
    #[tokio::test]
    async fn output_decode_error_preserves_exit_status() {
        let temp_dir = tempfile::tempdir().unwrap();
        let runner = LocalRunner::new(EnvironmentIsolation::None);
        let mut session = start_local_session(&runner, temp_dir.path()).await;
        let mut sink = VecSink::default();

        let status = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            session.exec(
                ExecSpec {
                    argv: shell_command("printf '\\377\\n'; head -c 1048576 /dev/zero"),
                    cwd: GuestPath(temp_dir.path().to_path_buf()),
                    env: IndexMap::new(),
                },
                &mut sink,
            ),
        )
        .await
        .expect("draining invalid output must not hang")
        .unwrap();

        assert!(status.success());
        assert!(sink.0.is_empty());
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

        let windows_runner = LocalRunner::new(EnvironmentIsolation::Strict)
            .with_runtime(RuntimeEnv::for_test(Platform::Win64));
        let mut windows_session = start_local_session(&windows_runner, directory).await;
        assert_eq!(
            windows_session
                .find_executable(&names, &search_paths)
                .await
                .unwrap(),
            Some(GuestPath(directory.join("tool.exe")))
        );

        let linux_runner = LocalRunner::new(EnvironmentIsolation::Strict)
            .with_runtime(RuntimeEnv::for_test(Platform::Linux64));
        let mut linux_session = start_local_session(&linux_runner, directory).await;
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
        let runner = LocalRunner::new(EnvironmentIsolation::Strict);
        let mut session = start_local_session(&runner, temp_dir.path()).await;

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
