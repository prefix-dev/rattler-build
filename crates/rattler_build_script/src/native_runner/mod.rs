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

    /// Returns a native-shell command that invokes a section script file, if
    /// native sections must be run indirectly. `cmd.exe` uses this so
    /// `exit /b` exits only the called section script, not the whole wrapper.
    fn native_section_script_command(&self, _script_path: &Path) -> Option<Vec<String>> {
        None
    }

    /// Wraps a non-empty section body in an isolated shell scope so its
    /// step-local `env` and shell state don't leak into later sections and a
    /// failure aborts the wrapper. `env` is emitted via [`Shell::set_env_var`]
    /// for consistent quoting; the scope primitive is shell-specific.
    fn scope_section(
        &self,
        label: Option<&str>,
        env: &IndexMap<String, String>,
        cwd: Option<&Path>,
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

/// Validate an env assignment before emitting it into a shell wrapper.
pub(crate) fn validate_env_assignment(key: &str, value: &str) -> Result<(), std::io::Error> {
    let mut chars = key.chars();
    let valid_key = chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric());
    if !valid_key {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid environment variable name '{key}'; expected [A-Za-z_][A-Za-z0-9_]*"),
        ));
    }
    if value.contains(['\n', '\r']) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "environment variable '{key}' contains a newline, which cannot be represented safely in build scripts"
            ),
        ));
    }
    Ok(())
}

/// Quotes a single command argument for the given shell when it contains shell
/// metacharacters, whitespace, or is empty. `rattler_shell::Shell::run_command`
/// joins arguments with spaces without quoting, so a resolved interpreter,
/// script path, or `cwd` containing characters like spaces or `&` would
/// otherwise be split or interpreted by the shell. For cmd batch files, literal
/// `%` characters are also escaped to avoid environment-variable expansion.
pub(crate) fn quote_arg(shell: &ShellEnum, arg: &str) -> String {
    fn posix_needs_quotes(arg: &str) -> bool {
        arg.is_empty()
            || arg.chars().any(|c| {
                !(c.is_ascii_alphanumeric()
                    || matches!(c, '/' | '.' | '-' | '_' | ':' | '+' | '=' | '@' | '%'))
            })
    }

    fn cmd_needs_quotes(arg: &str) -> bool {
        arg.is_empty()
            || arg.chars().any(|c| {
                c.is_whitespace()
                    || matches!(c, '&' | '|' | '<' | '>' | '(' | ')' | '^' | ';' | ',' | '=')
            })
    }

    match shell {
        ShellEnum::CmdExe(_) => {
            let escaped = arg.replace('%', "%%");
            if cmd_needs_quotes(&escaped) {
                format!("\"{escaped}\"")
            } else {
                escaped
            }
        }
        _ if posix_needs_quotes(arg) => format!("'{}'", arg.replace('\'', r"'\''")),
        _ => arg.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{native_runner, quote_arg};
    use indexmap::IndexMap;
    use rattler_conda_types::Platform;
    use rattler_shell::shell::{self, Shell};

    /// bash scopes a section in a bare subshell, emits the label comment, and
    /// quotes env via `set_env_var` (shlex). No errorlevel guard — `set -e`
    /// from the preamble handles failure.
    #[test]
    fn bash_scope_section_subshell_env_and_label() {
        let runner = native_runner(Platform::Linux64);
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "a b".to_string());
        let out = runner
            .scope_section(Some("uses: configure"), &env, None, "echo hi")
            .unwrap();
        insta::assert_snapshot!(out, @r###"
# === uses: configure ===
(
export FOO='a b'
echo hi
)
"###);
    }

    /// No label and empty env => just `( body )`.
    #[test]
    fn bash_scope_section_minimal() {
        let runner = native_runner(Platform::Linux64);
        let out = runner
            .scope_section(None, &IndexMap::new(), None, "echo hi")
            .unwrap();
        insta::assert_snapshot!(out, @r###"
(
echo hi
)
"###);
    }

    /// cmd scopes env via `setlocal`/`endlocal`, cwd via `pushd`/`popd`, and
    /// appends an errorlevel guard (required even for the last section).
    #[test]
    fn cmd_scope_section_setlocal_env_and_guard() {
        let runner = native_runner(Platform::Win64);
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let out = runner
            .scope_section(Some("step 1"), &env, None, "echo hi")
            .unwrap();
        insta::assert_snapshot!(out, @r###"
@rem === step 1 ===
setlocal
@SET "FOO=bar"
pushd .
if %errorlevel% neq 0 exit /b %errorlevel%
echo hi
set "RB_SECTION_ERRORLEVEL=%errorlevel%"
popd
if %RB_SECTION_ERRORLEVEL% equ 0 if %errorlevel% neq 0 set "RB_SECTION_ERRORLEVEL=%errorlevel%"
endlocal & if %RB_SECTION_ERRORLEVEL% neq 0 exit /b %RB_SECTION_ERRORLEVEL%
"###);
    }

    #[test]
    fn cmd_scope_section_pushd_uses_cwd() {
        let runner = native_runner(Platform::Win64);
        let out = runner
            .scope_section(
                Some("step 1"),
                &IndexMap::new(),
                Some(std::path::Path::new(r"C:\some&dir")),
                "echo hi",
            )
            .unwrap();

        insta::assert_snapshot!(out, @r###"
@rem === step 1 ===
setlocal
pushd "C:\some&dir"
if %errorlevel% neq 0 exit /b %errorlevel%
echo hi
set "RB_SECTION_ERRORLEVEL=%errorlevel%"
popd
if %RB_SECTION_ERRORLEVEL% equ 0 if %errorlevel% neq 0 set "RB_SECTION_ERRORLEVEL=%errorlevel%"
endlocal & if %RB_SECTION_ERRORLEVEL% neq 0 exit /b %RB_SECTION_ERRORLEVEL%
"###);
    }

    #[test]
    fn scope_section_rejects_invalid_env_names() {
        let runner = native_runner(Platform::Linux64);
        let mut env = IndexMap::new();
        env.insert("BAD-NAME".to_string(), "value".to_string());

        let err = runner
            .scope_section(None, &env, None, "echo hi")
            .expect_err("invalid env name should fail");

        assert!(
            err.to_string()
                .contains("invalid environment variable name")
        );
    }

    #[test]
    fn cmd_scope_section_rejects_newline_env_values() {
        let runner = native_runner(Platform::Win64);
        let mut env = IndexMap::new();
        env.insert("FOO".to_string(), "safe\necho injected".to_string());

        let err = runner
            .scope_section(None, &env, None, "echo hi")
            .expect_err("newline env value should fail");

        assert!(err.to_string().contains("contains a newline"));
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
        // Whitespace and metacharacters are single-quoted for posix shells.
        assert_eq!(
            quote_arg(&bash, "/opt/my tools/node"),
            "'/opt/my tools/node'"
        );
        assert_eq!(quote_arg(&bash, "/tmp/a&b"), "'/tmp/a&b'");
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
        assert_eq!(quote_arg(&cmd, r"C:\tmp\a&b"), "\"C:\\tmp\\a&b\"");
        assert_eq!(quote_arg(&cmd, r"C:\tmp\a;b"), "\"C:\\tmp\\a;b\"");
        assert_eq!(quote_arg(&cmd, r"C:\tmp\a,b"), "\"C:\\tmp\\a,b\"");
        assert_eq!(quote_arg(&cmd, r"C:\tmp\a=b"), "\"C:\\tmp\\a=b\"");
        assert_eq!(
            quote_arg(&cmd, r"C:\tmp\%NO_SUCH_VAR%\script.bat"),
            r"C:\tmp\%%NO_SUCH_VAR%%\script.bat"
        );
        assert_eq!(
            quote_arg(&cmd, r"C:\tmp\%NO_SUCH_VAR% dir\script.bat"),
            r#""C:\tmp\%%NO_SUCH_VAR%% dir\script.bat""#
        );
    }
}
