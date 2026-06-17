//! Platform-native wrapper execution support.
//!
//! This module selects the [`NativeShellRunner`] for a platform and provides
//! helpers for writing shell-specific scripts. Specialized interpreters are
//! described by `crate::interpreter` and are emitted as commands inside the
//! native wrapper.

mod bash;
mod cmd_exe;

use std::path::Path;

use rattler_conda_types::Platform;
use rattler_shell::shell::{Shell, ShellEnum};

/// Defines platform-native wrapper execution.
pub(crate) trait NativeShellRunner: Send + Sync {
    /// Returns the shell syntax used for the generated native wrapper script.
    fn shell(&self) -> ShellEnum;

    /// The recipe interpreter name for this wrapper shell (`bash`/`cmd`), used
    /// as the default when no interpreter is specified.
    fn default_interpreter(&self) -> &'static str;

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

/// Selects the native wrapper shell for the given platform: `cmd.exe` on
/// Windows, `bash` elsewhere. The script runs on the host, so callers pass the
/// runtime platform (which equals the host).
pub(crate) fn native_runner(platform: Platform) -> Box<dyn NativeShellRunner> {
    if platform.is_windows() {
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

/// Quotes a single command argument for the given shell when it contains
/// whitespace (or is empty). `rattler_shell::Shell::run_command` joins arguments
/// with spaces without quoting, so a resolved interpreter or script path
/// containing spaces (e.g. `C:\Program Files\nodejs\node.exe`) would otherwise
/// be split by the shell.
pub(crate) fn quote_arg(shell: &ShellEnum, arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(char::is_whitespace) {
        return arg.to_string();
    }
    match shell {
        ShellEnum::CmdExe(_) => format!("\"{arg}\""),
        // POSIX single-quoting neutralizes spaces and metacharacters; an
        // embedded single quote is closed, escaped, and reopened.
        _ => format!("'{}'", arg.replace('\'', r"'\''")),
    }
}

#[cfg(test)]
mod tests {
    use super::{native_runner, quote_arg};
    use rattler_conda_types::Platform;
    use rattler_shell::shell::{self, Shell};

    #[test]
    fn native_runner_follows_the_platform() {
        // Independent of the host this test runs on.
        assert_eq!(native_runner(Platform::Win64).shell().extension(), "bat");
        assert_eq!(native_runner(Platform::Linux64).shell().extension(), "sh");
        assert_eq!(native_runner(Platform::OsxArm64).shell().extension(), "sh");

        assert_eq!(native_runner(Platform::Win64).default_interpreter(), "cmd");
        assert_eq!(
            native_runner(Platform::Linux64).default_interpreter(),
            "bash"
        );
    }

    #[test]
    fn quotes_only_when_needed() {
        let bash = shell::Bash::default().into();
        // No whitespace: left untouched (flags must not be quoted).
        assert_eq!(quote_arg(&bash, "-NoLogo"), "-NoLogo");
        assert_eq!(quote_arg(&bash, "/usr/bin/python"), "/usr/bin/python");
        // Whitespace: single-quoted for posix shells.
        assert_eq!(
            quote_arg(&bash, "/opt/my tools/node"),
            "'/opt/my tools/node'"
        );
        // Embedded single quote is escaped.
        assert_eq!(quote_arg(&bash, "a'b c"), "'a'\\''b c'");
    }

    #[test]
    fn quotes_for_cmd_with_double_quotes() {
        let cmd = shell::CmdExe.into();
        assert_eq!(quote_arg(&cmd, "/d"), "/d");
        assert_eq!(
            quote_arg(&cmd, r"C:\Program Files\nodejs\node.exe"),
            "\"C:\\Program Files\\nodejs\\node.exe\""
        );
    }
}
