//! Script execution backends and their sessions.

mod environment;
mod local;
mod mounts;
mod output;
mod sandbox;
#[cfg(test)]
mod test_support;

use std::path::PathBuf;

use async_trait::async_trait;
use indexmap::IndexMap;
use rattler_conda_types::Subdir;

use crate::EnvironmentIsolation;

pub(crate) use environment::resolve_process_env;
pub use local::LocalRunner;
pub use mounts::Mounts;
pub use output::{BuildLogSink, ReplacingSink};
pub use sandbox::SandboxRunner;

/// Error returned while preparing or driving a runner session.
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    /// An I/O failure while starting or driving a process.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The runner cannot execute on the platform rattler-build runs on.
    #[error("the {runner} runner is not supported on {platform}")]
    UnsupportedPlatform {
        /// The [`Runner::name`] of the runner.
        runner: &'static str,
        /// The platform rattler-build runs on.
        platform: Subdir,
    },
    /// An executable the runner depends on is not on the runtime `PATH`.
    #[error("{name} executable not found. Please install it with: {install_hint}")]
    MissingExecutable {
        /// The name of the executable that was looked up.
        name: &'static str,
        /// The command that installs the executable.
        install_hint: &'static str,
    },
    /// A session asked for a mount the runner cannot provide, because it
    /// exposes host paths unchanged.
    #[error(
        "the {runner} runner cannot mount {} at the different guest path {}",
        host.0.display(),
        guest.0.display()
    )]
    UnsupportedMount {
        /// The [`Runner::name`] of the runner.
        runner: &'static str,
        /// The source path on the host.
        host: HostPath,
        /// The requested destination path in the guest.
        guest: GuestPath,
    },
    /// A command left more bytes in a channel file than a session returns
    /// (see [`ExecSpec::channels`]). The command itself has finished; its
    /// exit status is carried along for callers that discard the contents.
    #[error("the channel file of {variable} holds more than the {limit} bytes a session returns")]
    ChannelTooLarge {
        /// The environment variable the channel was declared under.
        variable: String,
        /// The largest size a session returns.
        limit: u64,
        /// How the command exited.
        status: ExecStatus,
    },
}

/// The largest number of bytes a session returns per channel file; a larger
/// file fails the command with [`RunnerError::ChannelTooLarge`].
pub const CHANNEL_SIZE_LIMIT: u64 = 16 * 1024 * 1024;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// The source path on the host.
    pub host: HostPath,
    /// The destination path in the guest.
    pub guest: GuestPath,
    /// Whether the guest may write through the mount.
    pub writable: bool,
}

impl Mount {
    /// A mount that exposes the host directory `path` at the same path in the
    /// guest.
    pub fn mirrored(path: impl Into<PathBuf>, writable: bool) -> Self {
        let path = path.into();
        Self {
            host: HostPath(path.clone()),
            guest: GuestPath(path),
            writable,
        }
    }
}

/// Everything needed to start a session.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    /// The platform scripts execute on.
    pub platform: Subdir,
    /// The bind mounts the session must provide.
    pub mounts: Mounts,
    /// The container image for runners that need one.
    pub image: Option<String>,
    /// The session working directory as a guest path.
    pub work_dir: GuestPath,
    /// A writable directory, as a guest path, for the session's own files.
    ///
    /// The directory must exist and be writable in the guest. The session
    /// creates one uniquely named subdirectory inside it, keeps everything it
    /// needs during its lifetime (such as the channel files of
    /// [`ExecSpec::channels`]) in that subdirectory, and removes the
    /// subdirectory when it is dropped; nothing else in `scratch_dir` is
    /// touched.
    pub scratch_dir: GuestPath,
    /// How the session builds the environment of every executed command.
    ///
    /// [`EnvironmentIsolation::Strict`] and [`EnvironmentIsolation::CondaBuild`]
    /// start from an empty environment plus the host passthrough allowlist
    /// (certificates, proxies, the SSH agent, and the platform's system
    /// variables); [`EnvironmentIsolation::None`] starts from the complete host
    /// environment. In every mode the explicit [`ExecSpec::env`] is applied
    /// last and overrides whatever the session started from.
    pub env_isolation: EnvironmentIsolation,
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
    /// The environment variables the command receives a channel file under.
    ///
    /// A channel file is the command's way of handing data back to the caller
    /// without going through its output streams. For every name the session
    /// creates a fresh, empty file at a location of its choosing, unique to
    /// this execution, and sets the variable to the file's guest path in the
    /// command's environment, regardless of the session's isolation mode. After
    /// the command exits the session reads every file back into
    /// [`ExecOutcome::channels`], keyed by variable name, whether the command
    /// succeeded or not; a file the command removed reads as empty. A file
    /// larger than [`CHANNEL_SIZE_LIMIT`] fails the execution with
    /// [`RunnerError::ChannelTooLarge`]. A name listed twice shares one file.
    /// The channel variable is set after [`ExecSpec::env`], so it overrides a
    /// variable of the same name given there. The session only transports the
    /// bytes; their meaning is the caller's.
    pub channels: Vec<String>,
}

/// What a command execution left behind.
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    /// How the command exited.
    pub status: ExecStatus,
    /// The contents of the channel files declared in [`ExecSpec::channels`],
    /// in declaration order, every declared name present.
    pub channels: IndexMap<String, Vec<u8>>,
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

/// Methods on the result of [`Session::exec`] and
/// [`Session::exec_interactive`].
pub trait ExecResultExt {
    /// Returns how the command exited and discards the channel files.
    ///
    /// A command that finished but left more than the limit in a channel
    /// file ([`RunnerError::ChannelTooLarge`]) is not a failure for a caller
    /// that does not read the contents: the overflow is logged as a warning
    /// and the exit status the error carries is returned. Every other error
    /// is returned unchanged.
    fn into_status(self) -> Result<ExecStatus, RunnerError>;
}

impl ExecResultExt for Result<ExecOutcome, RunnerError> {
    fn into_status(self) -> Result<ExecStatus, RunnerError> {
        match self {
            Ok(outcome) => Ok(outcome.status),
            Err(RunnerError::ChannelTooLarge {
                variable,
                limit,
                status,
            }) => {
                tracing::warn!(
                    variable,
                    limit,
                    "The command wrote more than the limit into a channel file; its contents are discarded"
                );
                Ok(status)
            }
            Err(
                error @ (RunnerError::Io(_)
                | RunnerError::UnsupportedPlatform { .. }
                | RunnerError::MissingExecutable { .. }
                | RunnerError::UnsupportedMount { .. }),
            ) => Err(error),
        }
    }
}

/// Facts about the guest execution environment discovered by a runner.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GuestInfo {
    /// The platform scripts execute on.
    pub platform: Subdir,
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
    fn execution_platform(&self) -> Subdir;

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
    /// Runs a command, streaming raw output lines into `sink`, and returns
    /// its exit status together with the channel files of `spec`.
    async fn exec(
        &mut self,
        spec: ExecSpec,
        sink: &mut dyn OutputSink,
    ) -> Result<ExecOutcome, RunnerError>;

    /// Runs a command with stdio attached to the invoking terminal, and
    /// returns its exit status together with the channel files of `spec`.
    async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecOutcome, RunnerError>;

    /// Finds the first `names` entry on the given guest search paths.
    async fn find_executable(
        &mut self,
        names: &[String],
        search_paths: &[GuestPath],
    ) -> Result<Option<GuestPath>, RunnerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Discarding the channels keeps the exit status of a finished command,
    /// also when it overfilled a channel, and passes any other error through.
    #[test]
    fn into_status_keeps_the_exit_status() {
        let finished: Result<ExecOutcome, RunnerError> = Ok(ExecOutcome {
            status: ExecStatus { code: Some(3) },
            channels: IndexMap::from([("VAR".to_string(), b"ignored".to_vec())]),
        });
        let overfilled: Result<ExecOutcome, RunnerError> = Err(RunnerError::ChannelTooLarge {
            variable: "VAR".to_string(),
            limit: CHANNEL_SIZE_LIMIT,
            status: ExecStatus { code: Some(5) },
        });
        let failed: Result<ExecOutcome, RunnerError> =
            Err(RunnerError::Io(std::io::Error::other("spawn failed")));

        assert_eq!(finished.into_status().unwrap().code, Some(3));
        assert_eq!(overfilled.into_status().unwrap().code, Some(5));
        assert!(matches!(
            failed.into_status(),
            Err(RunnerError::Io(error)) if error.to_string() == "spawn failed"
        ));
    }
}
