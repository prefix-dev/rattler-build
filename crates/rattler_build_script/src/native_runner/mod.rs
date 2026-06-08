//! Platform-native wrapper execution support.
//!
//! This module selects the [`NativeShellRunner`] for the current platform and
//! provides helpers for writing shell-specific scripts. Specialized interpreters
//! are described by `crate::interpreter` and are emitted as commands inside the
//! native wrapper.

mod bash;
mod cmd_exe;

use std::path::Path;

use rattler_shell::shell::{Shell, ShellEnum};

/// Defines platform-native wrapper execution.
pub(crate) trait NativeShellRunner: Send + Sync {
    /// Returns the shell syntax used for the generated native wrapper script.
    fn shell(&self) -> ShellEnum;

    /// Returns the shell preamble inserted at the top of `conda_build.*`.
    fn preamble(&self, activation_script_path: &Path) -> String;

    /// Returns process argv used to execute the generated native wrapper script.
    fn command_to_run_script<'a>(&self, build_script_path: &'a str) -> Vec<&'a str>;

    /// Returns the replacement template used when streaming process output.
    fn replacements_template(&self) -> &'static str;

    /// Returns whether this native shell runner supports rattler-sandbox execution.
    fn supports_sandbox(&self) -> bool {
        true
    }

    /// Returns human-readable reproduction instructions shown when execution fails.
    fn debug_info(&self, work_dir: &Path, run_prefix: &Path, build_prefix: Option<&Path>)
    -> String;
}

pub(crate) fn native_runner() -> Box<dyn NativeShellRunner> {
    if cfg!(windows) {
        Box::new(cmd_exe::CmdExeNativeRunner)
    } else {
        Box::new(bash::BashNativeRunner)
    }
}

pub(crate) fn write_shell_script(
    shell: ShellEnum,
    script: &str,
) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    shell.write_script(&mut bytes, script)?;
    Ok(bytes)
}
