//! Script execution backends and their sessions.

mod environment;
mod local;

use std::path::PathBuf;

use async_trait::async_trait;
use indexmap::IndexMap;
use rattler_conda_types::Platform;

pub(crate) use environment::resolve_process_env;
pub use local::LocalRunner;

/// Error returned while preparing or driving a runner session.
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    /// An I/O failure while starting or driving a process.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A path on the machine rattler-build runs on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostPath(
    /// The underlying path.
    pub PathBuf,
);

/// A path as seen from inside a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GuestPath(
    /// The underlying path.
    pub PathBuf,
);

/// One bind mount from host into guest.
#[derive(Debug, Clone)]
pub struct Mount {
    /// The source path on the host.
    pub host: HostPath,
    /// The destination path in the guest.
    pub guest: GuestPath,
    /// Whether the guest may write through the mount.
    pub writable: bool,
}

/// Everything needed to start a session.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    /// The platform scripts execute on.
    pub platform: Platform,
    /// The bind mounts the session must provide.
    pub mounts: Vec<Mount>,
    /// The container image for runners that need one.
    pub image: Option<String>,
    /// The session working directory as a guest path.
    pub work_dir: GuestPath,
}

/// One command execution inside a session.
#[derive(Debug, Clone)]
pub struct ExecSpec {
    /// The program followed by its arguments.
    pub argv: Vec<String>,
    /// The command's current directory as a guest path.
    pub cwd: GuestPath,
    /// Explicit environment variables for the command.
    pub env: IndexMap<String, String>,
}

/// Exit status of an executed command.
#[derive(Debug, Clone, Copy)]
pub struct ExecStatus {
    /// The process exit code, or `None` when it was terminated by a signal.
    pub code: Option<i32>,
}

impl ExecStatus {
    /// Returns whether the command exited successfully.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

impl From<std::process::ExitStatus> for ExecStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        Self {
            code: status.code(),
        }
    }
}

/// Facts about the guest execution environment discovered by a runner.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GuestInfo {
    /// The platform scripts execute on.
    pub platform: Platform,
}

/// Which stream a raw output line arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    /// The standard output stream.
    Stdout,
    /// The standard error stream.
    Stderr,
}

/// Receives raw, CRLF-normalized output lines from an executing command.
pub trait OutputSink: Send {
    /// Receives one line from `stream` without its trailing newline.
    fn line(&mut self, stream: OutputStream, line: &str);
}

/// An execution backend for build, staging, and test scripts.
#[async_trait]
pub trait Runner: Send + Sync {
    /// Short identifier for this runner.
    fn name(&self) -> &str;

    /// The platform scripts execute on.
    fn execution_platform(&self) -> Platform;

    /// Performs a cheap availability check.
    async fn check_usable(&self) -> Result<(), RunnerError>;

    /// Discovers facts about the guest execution environment.
    async fn probe(&self) -> Result<GuestInfo, RunnerError>;

    /// Starts a session according to `spec`.
    async fn start_session(&self, spec: SessionSpec) -> Result<Box<dyn Session>, RunnerError>;
}

/// A started execution context.
#[async_trait]
pub trait Session: Send {
    /// Runs a command, streaming raw output lines into `sink`.
    async fn exec(
        &mut self,
        spec: ExecSpec,
        sink: &mut dyn OutputSink,
    ) -> Result<ExecStatus, RunnerError>;

    /// Runs a command with stdio attached to the invoking terminal.
    async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecStatus, RunnerError>;

    /// Finds the first `names` entry on the given guest search paths.
    async fn find_executable(
        &mut self,
        names: &[String],
        search_paths: &[GuestPath],
    ) -> Result<Option<GuestPath>, RunnerError>;
}
