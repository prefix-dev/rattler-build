//! A runner that confines every command of a session to `rattler-sandbox`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rattler_conda_types::Subdir;

use super::{
    ExecSpec, ExecStatus, GuestInfo, GuestPath, LocalRunner, Mounts, OutputSink, Runner,
    RunnerError, Session, SessionSpec,
};
use crate::{RuntimeEnv, SandboxConfiguration};

/// The [`Runner::name`] of [`SandboxRunner`].
const SANDBOX_RUNNER_NAME: &str = "sandbox";

/// The executable looked up on the runtime `PATH`.
const SANDBOX_EXECUTABLE: &str = "rattler-sandbox";

/// The command that installs [`SANDBOX_EXECUTABLE`].
const SANDBOX_INSTALL_HINT: &str = "pixi global install rattler-sandbox";

/// A runner that executes every command of a session inside `rattler-sandbox`.
///
/// The static policy (network access and the system paths every build may
/// read, execute, or write) is the [`SandboxConfiguration`] the runner was
/// created with. The per-session file-system access is derived from the
/// session's [`Mount`]s: a writable mount grants read, write, and execute
/// access, a read-only mount grants read access. The sandbox exposes host
/// paths unchanged, so every mount has to be mirrored (host path equal to
/// guest path) and is granted by its host path.
///
/// Process execution, output streaming, and executable lookup are delegated
/// to a [`LocalRunner`] with the same [`RuntimeEnv`]. A runner only exists
/// for a Linux or macOS runtime with `rattler-sandbox` on its `PATH`:
/// [`SandboxRunner::new`] reports anything else as a [`RunnerError`] before
/// any script runs.
pub struct SandboxRunner {
    configuration: SandboxConfiguration,
    executable: PathBuf,
    runtime: RuntimeEnv,
    local: LocalRunner,
}

impl SandboxRunner {
    /// Creates a sandbox runner for the current process environment.
    ///
    /// Fails with [`RunnerError::UnsupportedPlatform`] when the current
    /// platform has no sandbox support and with
    /// [`RunnerError::MissingExecutable`] when `rattler-sandbox` is not on
    /// `PATH`.
    pub fn new(configuration: SandboxConfiguration) -> Result<Self, RunnerError> {
        Self::with_runtime(configuration, RuntimeEnv::current())
    }

    /// Creates a sandbox runner for `runtime`, with the checks of
    /// [`SandboxRunner::new`] applied to that runtime.
    pub fn with_runtime(
        configuration: SandboxConfiguration,
        runtime: RuntimeEnv,
    ) -> Result<Self, RunnerError> {
        let executable = locate_sandbox(&runtime)?;
        Ok(Self {
            configuration,
            executable,
            local: LocalRunner::new().with_runtime(runtime.clone()),
            runtime,
        })
    }
}

/// Checks that `runtime` is a platform `rattler-sandbox` can confine
/// processes on and locates the sandbox executable on its `PATH`.
fn locate_sandbox(runtime: &RuntimeEnv) -> Result<PathBuf, RunnerError> {
    let platform = runtime.process_platform();
    if !(platform.is_linux() || platform.is_osx()) {
        return Err(RunnerError::UnsupportedPlatform {
            runner: SANDBOX_RUNNER_NAME,
            platform,
        });
    }

    which::which_in_global(SANDBOX_EXECUTABLE, Some(runtime.path()))
        .ok()
        .and_then(|mut found| found.next())
        .ok_or(RunnerError::MissingExecutable {
            name: SANDBOX_EXECUTABLE,
            install_hint: SANDBOX_INSTALL_HINT,
        })
}

/// The arguments placed in front of every command: the sandbox executable, the
/// static policy, and the access derived from `mounts`.
fn sandbox_argv_prefix(
    executable: &Path,
    configuration: &SandboxConfiguration,
    mounts: &Mounts,
) -> Vec<String> {
    let mut argv = vec![executable.to_string_lossy().into_owned()];
    argv.extend(configuration.to_args());
    for mount in mounts {
        let path = mount.host.0.to_string_lossy().into_owned();
        if mount.writable {
            argv.push("--fs-write-and-read".to_string());
            argv.push(path.clone());
            argv.push("--fs-exec-and-read".to_string());
            argv.push(path);
        } else {
            argv.push("--fs-read".to_string());
            argv.push(path);
        }
    }
    argv
}

#[async_trait]
impl Runner for SandboxRunner {
    fn name(&self) -> &str {
        SANDBOX_RUNNER_NAME
    }

    fn execution_platform(&self) -> Subdir {
        self.local.execution_platform()
    }

    async fn check_usable(&self) -> Result<(), RunnerError> {
        locate_sandbox(&self.runtime)?;
        self.local.check_usable().await
    }

    async fn probe(&self) -> Result<GuestInfo, RunnerError> {
        self.local.probe().await
    }

    async fn start_session(&self, spec: SessionSpec) -> Result<Box<dyn Session>, RunnerError> {
        spec.mounts.require_mirrored(SANDBOX_RUNNER_NAME)?;
        let argv_prefix = sandbox_argv_prefix(&self.executable, &self.configuration, &spec.mounts);
        tracing::info!("{}", self.configuration);
        tracing::debug!(mounts = ?spec.mounts, "Granting the session mounts to the sandbox");
        let inner = self.local.start_session(spec).await?;
        Ok(Box::new(SandboxSession { argv_prefix, inner }))
    }
}

/// A session whose commands run through `rattler-sandbox` in an inner session.
struct SandboxSession {
    argv_prefix: Vec<String>,
    inner: Box<dyn Session>,
}

impl SandboxSession {
    /// Prefixes the command of `spec` with the sandbox invocation.
    fn confine(&self, spec: ExecSpec) -> Result<ExecSpec, RunnerError> {
        if spec.argv.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot execute an empty argv",
            )
            .into());
        }
        let mut argv = self.argv_prefix.clone();
        argv.extend(spec.argv);
        Ok(ExecSpec { argv, ..spec })
    }
}

#[async_trait]
impl Session for SandboxSession {
    async fn exec(
        &mut self,
        spec: ExecSpec,
        sink: &mut dyn OutputSink,
    ) -> Result<ExecStatus, RunnerError> {
        let spec = self.confine(spec)?;
        self.inner.exec(spec, sink).await
    }

    async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecStatus, RunnerError> {
        let spec = self.confine(spec)?;
        self.inner.exec_interactive(spec).await
    }

    async fn find_executable(
        &mut self,
        names: &[String],
        search_paths: &[GuestPath],
    ) -> Result<Option<GuestPath>, RunnerError> {
        self.inner.find_executable(names, search_paths).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use indexmap::IndexMap;

    use super::*;
    use crate::execution::{BuildScriptSection, ExecutionArgs, ResolvedScriptContents, run_script};
    use crate::runner::test_support::VecSink;
    use crate::runner::{HostPath, Mount, OutputStream};
    use crate::sandbox::SandboxArguments;
    use crate::{EnvironmentIsolation, ExecutionContext, InterpreterError};

    /// A static policy with network access and one read-only system path.
    fn configuration() -> SandboxConfiguration {
        Option::<SandboxConfiguration>::from(SandboxArguments {
            sandbox: true,
            allow_network: true,
            allow_read: vec![PathBuf::from("/opt/data")],
            allow_read_execute: Vec::new(),
            allow_read_write: Vec::new(),
            overwrite_default_sandbox_config: true,
        })
        .expect("the sandbox is enabled")
    }

    fn mounts() -> Mounts {
        Mounts::new()
            .writable("/builds/pkg")
            .read_only("/recipes/pkg")
    }

    fn session_spec(mounts: Mounts) -> SessionSpec {
        SessionSpec {
            platform: Subdir::Linux64,
            mounts,
            image: None,
            work_dir: GuestPath(PathBuf::from("/builds/pkg/work")),
            env_isolation: EnvironmentIsolation::Strict,
        }
    }

    fn exec_spec(argv: Vec<String>) -> ExecSpec {
        let mut env = IndexMap::new();
        env.insert("RB_SANDBOX_VAR".to_string(), "value".to_string());
        ExecSpec {
            argv,
            cwd: GuestPath(PathBuf::from("/builds/pkg/work")),
            env,
        }
    }

    /// A [`Session`] that records the specs it receives.
    #[derive(Default)]
    struct RecordingSession {
        execs: Arc<Mutex<Vec<ExecSpec>>>,
    }

    #[async_trait]
    impl Session for RecordingSession {
        async fn exec(
            &mut self,
            spec: ExecSpec,
            sink: &mut dyn OutputSink,
        ) -> Result<ExecStatus, RunnerError> {
            self.execs.lock().unwrap().push(spec);
            sink.line(OutputStream::Stdout, "confined");
            Ok(ExecStatus { code: Some(0) })
        }

        async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecStatus, RunnerError> {
            self.execs.lock().unwrap().push(spec);
            Ok(ExecStatus { code: Some(3) })
        }

        async fn find_executable(
            &mut self,
            names: &[String],
            search_paths: &[GuestPath],
        ) -> Result<Option<GuestPath>, RunnerError> {
            Ok(search_paths
                .first()
                .zip(names.first())
                .map(|(search_path, name)| GuestPath(search_path.0.join(name))))
        }
    }

    /// Creates an empty file that the runtime `PATH` lookup accepts as the
    /// sandbox executable.
    fn create_fake_sandbox(directory: &Path) {
        let executable = directory.join(format!(
            "{SANDBOX_EXECUTABLE}{}",
            std::env::consts::EXE_SUFFIX
        ));
        fs_err::write(&executable, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs_err::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Creates a sandbox executable that prints every argument after its own
    /// name on a line of its own and exits successfully without running the
    /// command: a shell script on Unix, a batch script on Windows.
    fn create_argv_printing_sandbox(directory: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable = directory.join(SANDBOX_EXECUTABLE);
            fs_err::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
            fs_err::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        {
            let executable = directory.join(format!("{SANDBOX_EXECUTABLE}.cmd"));
            fs_err::write(
                &executable,
                "@echo off\r\n:next\r\nif \"%~1\"==\"\" goto done\r\necho %~1\r\nshift\r\ngoto next\r\n:done\r\n",
            )
            .unwrap();
        }
    }

    fn runner_with_path(platform: Subdir, path: &Path) -> Result<SandboxRunner, RunnerError> {
        SandboxRunner::with_runtime(
            configuration(),
            RuntimeEnv::for_test(platform).with_var("PATH", path.to_string_lossy().into_owned()),
        )
    }

    /// Execution arguments for an inline build script of a Linux build in
    /// `build_dir`, with the build directory writable and the recipe
    /// directory read-only, as `Directories::session_mounts` grants them.
    fn execution_args(runtime: RuntimeEnv, build_dir: &Path, recipe_dir: &Path) -> ExecutionArgs {
        let work_dir = build_dir.join("work");
        let prefix = build_dir.join("host");
        fs_err::create_dir_all(&work_dir).unwrap();
        fs_err::create_dir_all(&prefix).unwrap();
        fs_err::create_dir_all(recipe_dir).unwrap();
        ExecutionArgs {
            sections: vec![BuildScriptSection {
                interpreter: None,
                content: ResolvedScriptContents::Inline("echo hello".to_string()),
                env: IndexMap::new(),
                cwd: None,
                label: None,
            }],
            env_vars: IndexMap::new(),
            secrets: IndexMap::new(),
            context: ExecutionContext::shared(runtime, prefix, Subdir::Linux64, Subdir::Linux64),
            work_dir,
            mounts: Mounts::new().writable(build_dir).read_only(recipe_dir),
            env_isolation: EnvironmentIsolation::None,
        }
    }

    /// Every command of the session is prefixed with the sandbox executable,
    /// the static policy, and the mount grants, in that order; the rest of
    /// the spec and the output pass through untouched.
    #[tokio::test]
    async fn sandbox_session_confines_every_command() {
        let recording = RecordingSession::default();
        let execs = Arc::clone(&recording.execs);
        let mut session = SandboxSession {
            argv_prefix: sandbox_argv_prefix(
                Path::new("/usr/bin/rattler-sandbox"),
                &configuration(),
                &mounts(),
            ),
            inner: Box::new(recording),
        };
        let mut sink = VecSink::default();
        let command = vec!["bash".to_string(), "-c".to_string(), "echo".to_string()];

        let status = session
            .exec(exec_spec(command.clone()), &mut sink)
            .await
            .unwrap();
        let interactive_status = session
            .exec_interactive(exec_spec(command.clone()))
            .await
            .unwrap();

        assert!(status.success());
        assert_eq!(interactive_status.code, Some(3));
        assert_eq!(sink.0, vec![(OutputStream::Stdout, "confined".to_string())]);
        let mut expected_argv = vec![
            "/usr/bin/rattler-sandbox".to_string(),
            "--network".to_string(),
            "--fs-read".to_string(),
            "/opt/data".to_string(),
            "--fs-write-and-read".to_string(),
            "/builds/pkg".to_string(),
            "--fs-exec-and-read".to_string(),
            "/builds/pkg".to_string(),
            "--fs-read".to_string(),
            "/recipes/pkg".to_string(),
        ];
        expected_argv.extend(command);
        let execs = execs.lock().unwrap();
        assert_eq!(execs.len(), 2);
        for exec in execs.iter() {
            assert_eq!(exec.argv, expected_argv);
            assert_eq!(exec.cwd, GuestPath(PathBuf::from("/builds/pkg/work")));
            assert_eq!(
                exec.env.get("RB_SANDBOX_VAR").map(String::as_str),
                Some("value")
            );
        }
    }

    #[tokio::test]
    async fn sandbox_session_rejects_empty_argv_and_delegates_lookup() {
        let mut session = SandboxSession {
            argv_prefix: sandbox_argv_prefix(
                Path::new("/usr/bin/rattler-sandbox"),
                &configuration(),
                &Mounts::new(),
            ),
            inner: Box::new(RecordingSession::default()),
        };

        let error = session
            .exec(exec_spec(Vec::new()), &mut VecSink::default())
            .await
            .unwrap_err();
        assert!(
            matches!(error, RunnerError::Io(ref error) if error.kind() == std::io::ErrorKind::InvalidInput),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            session
                .find_executable(
                    &["python".to_string()],
                    &[GuestPath(PathBuf::from("/prefix/bin"))]
                )
                .await
                .unwrap(),
            Some(GuestPath(PathBuf::from("/prefix/bin").join("python")))
        );
    }

    #[test]
    fn sandbox_runner_rejects_a_windows_runtime() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_sandbox(temp_dir.path());

        let error = runner_with_path(Subdir::Win64, temp_dir.path())
            .err()
            .expect("a Windows runtime has no sandbox");

        assert!(
            matches!(
                error,
                RunnerError::UnsupportedPlatform {
                    runner: "sandbox",
                    platform: Subdir::Win64
                }
            ),
            "unexpected error: {error:?}"
        );
    }

    /// A missing executable is reported when the runner is created, before
    /// any script runs, and keeps the install hint when it reaches the
    /// script layer as an [`InterpreterError`].
    #[test]
    fn missing_sandbox_executable_is_reported_at_construction() {
        let temp_dir = tempfile::tempdir().unwrap();

        let error = runner_with_path(Subdir::Linux64, temp_dir.path())
            .err()
            .expect("the sandbox executable is missing");

        assert!(
            matches!(error, RunnerError::MissingExecutable { .. }),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "rattler-sandbox executable not found. Please install it with: pixi global install rattler-sandbox"
        );
        let error = InterpreterError::from(error);
        assert!(
            matches!(
                error,
                InterpreterError::Runner(RunnerError::MissingExecutable { .. })
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "rattler-sandbox executable not found. Please install it with: pixi global install rattler-sandbox"
        );
    }

    #[tokio::test]
    async fn sandbox_runner_starts_a_session_with_the_executable_on_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_sandbox(temp_dir.path());
        let runner = runner_with_path(Subdir::Linux64, temp_dir.path()).unwrap();

        runner.check_usable().await.unwrap();
        assert_eq!(runner.name(), "sandbox");
        assert_eq!(runner.execution_platform(), Subdir::Linux64);
        assert!(runner.start_session(session_spec(mounts())).await.is_ok());
    }

    /// The executable is looked up again by `check_usable`, so a sandbox
    /// removed after the runner was created is reported.
    #[tokio::test]
    async fn check_usable_reports_an_executable_removed_after_construction() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_sandbox(temp_dir.path());
        let runner = runner_with_path(Subdir::Linux64, temp_dir.path()).unwrap();
        fs_err::remove_file(temp_dir.path().join(format!(
            "{SANDBOX_EXECUTABLE}{}",
            std::env::consts::EXE_SUFFIX
        )))
        .unwrap();

        let error = runner.check_usable().await.unwrap_err();

        assert!(
            matches!(error, RunnerError::MissingExecutable { .. }),
            "unexpected error: {error:?}"
        );
    }

    /// The sandbox cannot place a host directory at another guest path.
    #[tokio::test]
    async fn sandbox_runner_rejects_a_mount_with_a_different_guest_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_sandbox(temp_dir.path());
        let runner = runner_with_path(Subdir::Linux64, temp_dir.path()).unwrap();
        let mounts = mounts().mount(Mount {
            host: HostPath(PathBuf::from("/host/source")),
            guest: GuestPath(PathBuf::from("/guest/source")),
            writable: true,
        });

        let error = runner
            .start_session(session_spec(mounts))
            .await
            .err()
            .expect("a mount with a different guest path is rejected");

        assert!(
            matches!(
                &error,
                RunnerError::UnsupportedMount { runner: "sandbox", host, guest }
                    if host.0 == Path::new("/host/source") && guest.0 == Path::new("/guest/source")
            ),
            "unexpected error: {error:?}"
        );
    }

    /// `run_script` through the runner spawns the sandbox executable found on
    /// the runtime `PATH` with the static policy, the grants of the mounts,
    /// and the wrapper command, and its output reaches the build log with the
    /// work directory replaced.
    #[tokio::test]
    async fn sandbox_runner_confines_the_wrapper_of_run_script() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_argv_printing_sandbox(temp_dir.path());
        let runner = runner_with_path(Subdir::Linux64, temp_dir.path()).unwrap();
        let build_dir = temp_dir.path().join("build");
        let recipe_dir = temp_dir.path().join("recipe");
        let args = execution_args(runner.runtime.clone(), &build_dir, &recipe_dir);

        run_script(args, &runner).await.unwrap();

        let log = fs_err::read_to_string(build_dir.join("work").join("conda_build.log")).unwrap();
        let build_dir = build_dir.to_string_lossy().into_owned();
        let expected = vec![
            "--network".to_string(),
            "--fs-read".to_string(),
            "/opt/data".to_string(),
            "--fs-write-and-read".to_string(),
            build_dir.clone(),
            "--fs-exec-and-read".to_string(),
            build_dir,
            "--fs-read".to_string(),
            recipe_dir.to_string_lossy().into_owned(),
            "bash".to_string(),
            format!("$SRC_DIR{}conda_build.sh", std::path::MAIN_SEPARATOR),
        ];
        assert_eq!(log.lines().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn runner_is_object_safe() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_sandbox(temp_dir.path());
        let runner = runner_with_path(Subdir::Linux64, temp_dir.path()).unwrap();
        let _: Arc<dyn Runner> = Arc::new(runner);
    }
}
