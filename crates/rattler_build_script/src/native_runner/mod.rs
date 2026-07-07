//! Platform-native wrapper execution support.
//!
//! This module selects the [`NativeShellRunner`] for a platform and provides
//! helpers for writing shell-specific scripts. Specialized interpreters are
//! described by `crate::interpreter` and are emitted as commands inside the
//! native wrapper.

mod bash;
mod cmd_exe;

use std::path::Path;

use indexmap::IndexMap;
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

    /// Wraps a non-empty section body in an isolated shell scope so its
    /// step-local `env` and shell state don't leak into later sections and a
    /// failure aborts the wrapper. `env` is emitted via [`Shell::set_env_var`]
    /// for consistent quoting; the scope primitive is shell-specific.
    fn scope_section(
        &self,
        label: Option<&str>,
        env: &IndexMap<String, String>,
        body: &str,
    ) -> Result<String, std::io::Error>;

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
    use indexmap::IndexMap;
    use rattler_conda_types::Platform;
    use rattler_shell::shell::{self, Shell};

    /// True if any line equals `want` after trimming trailing whitespace.
    fn has_line(s: &str, want: &str) -> bool {
        s.lines().any(|l| l.trim_end() == want)
    }

    /// bash scopes a section in a bare subshell, emits the label comment, and
    /// quotes env via `set_env_var` (shlex). No errorlevel guard — `set -e`
    /// from the preamble handles failure.
    #[test]
    fn bash_scope_section_subshell_env_and_label() {
        let runner = native_runner(Platform::Linux64);
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "a b".to_string());
        let out = runner
            .scope_section(Some("uses: configure"), &env, "echo hi")
            .unwrap();
        assert!(out.contains("# === uses: configure ==="), "{out}");
        assert!(has_line(&out, "("), "missing subshell open:\n{out}");
        assert!(has_line(&out, ")"), "missing subshell close:\n{out}");
        assert!(
            out.contains("export FOO='a b'"),
            "env must be quoted:\n{out}"
        );
        assert!(out.contains("echo hi"), "{out}");
        assert!(!out.contains("errorlevel"), "bash needs no guard:\n{out}");
    }

    /// No label and empty env => just `( body )`.
    #[test]
    fn bash_scope_section_minimal() {
        let runner = native_runner(Platform::Linux64);
        let out = runner
            .scope_section(None, &IndexMap::new(), "echo hi")
            .unwrap();
        assert!(!out.contains("# ==="), "no label => no comment:\n{out}");
        assert!(has_line(&out, "("), "{out}");
        assert!(has_line(&out, ")"), "{out}");
    }

    /// cmd scopes via `setlocal`/`endlocal`, sets env via `@SET`, and always
    /// appends the errorlevel guard (required even for the last section).
    #[test]
    fn cmd_scope_section_setlocal_env_and_guard() {
        let runner = native_runner(Platform::Win64);
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let out = runner
            .scope_section(Some("step 1"), &env, "echo hi")
            .unwrap();
        assert!(out.contains("@rem === step 1 ==="), "{out}");
        assert!(has_line(&out, "setlocal"), "{out}");
        assert!(has_line(&out, "endlocal"), "{out}");
        assert!(
            has_line(&out, "if %errorlevel% neq 0 exit /b %errorlevel%"),
            "guard required even when last:\n{out}"
        );
        assert!(out.contains(r#"@SET "FOO=bar""#), "{out}");
    }

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
    fn bash_preamble_enables_tracing_after_activation() {
        let preamble =
            native_runner(Platform::Linux64).preamble(std::path::Path::new("build_env.sh"));
        let activation = preamble
            .find("source")
            .expect("preamble sources activation");
        let trace = preamble.find("set -x").expect("preamble enables tracing");
        // `set -x` must come after activation so the sourced environment setup
        // (which may expand secrets) is not traced (#2264).
        assert!(
            trace > activation,
            "set -x must follow activation, got:\n{preamble}"
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
