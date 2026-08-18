//! Best-effort detection of environment variables referenced by build scripts.
//!
//! Recipes frequently consume variant variables directly from the environment
//! of their build script (e.g. `${TARGET}` in `build.sh`) without ever
//! referencing them in the recipe itself. This module scans script sources for
//! the most common environment variable access spellings of each supported
//! interpreter so that callers can cross-reference the found names with the
//! variant configuration.
//!
//! The scanners are intentionally *not* full parsers: they over-approximate
//! (e.g. they do not understand quoting or comments). That is fine for the
//! intended use case, because the resulting names are only ever intersected
//! with the keys of the variant configuration.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::{Script, ScriptContent};

/// The scripting language dialect used to scan for environment variable
/// references. Maps 1:1 to the interpreters supported by rattler-build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptDialect {
    /// POSIX-ish shells: `bash`, `sh`, `zsh`, and `brush`.
    Bash,
    /// Windows `cmd.exe` batch files.
    CmdExe,
    /// PowerShell (`powershell` / `pwsh`).
    PowerShell,
    /// Python.
    Python,
    /// NuShell.
    NuShell,
    /// Perl.
    Perl,
    /// R (`rscript`).
    R,
    /// Ruby.
    Ruby,
    /// Node.js.
    NodeJs,
}

impl ScriptDialect {
    /// Map a recipe-facing interpreter name to a dialect.
    pub fn from_interpreter(interpreter: &str) -> Option<Self> {
        match interpreter.to_lowercase().as_str() {
            "bash" | "sh" | "zsh" | "dash" | "brush" => Some(Self::Bash),
            "cmd" | "cmd.exe" => Some(Self::CmdExe),
            "powershell" | "pwsh" => Some(Self::PowerShell),
            "python" | "python3" => Some(Self::Python),
            "nushell" | "nu" => Some(Self::NuShell),
            "perl" => Some(Self::Perl),
            "rscript" | "r" => Some(Self::R),
            "ruby" => Some(Self::Ruby),
            "node" | "nodejs" => Some(Self::NodeJs),
            _ => None,
        }
    }

    /// Infer the dialect from a script file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        crate::determine_interpreter_from_path(path)
            .as_deref()
            .and_then(Self::from_interpreter)
    }

    /// The dialect of the default (native) shell for a platform.
    pub fn default_for_platform(is_windows: bool) -> Self {
        if is_windows { Self::CmdExe } else { Self::Bash }
    }

    /// Extract all environment variable names referenced in `content` using
    /// the common spellings of this dialect.
    pub fn extract_env_variables(&self, content: &str) -> BTreeSet<String> {
        let patterns: &[&LazyLock<Regex>] = match self {
            // `$VAR`, `${VAR}`, `${VAR:-default}`, `${VAR%suffix}`, `${#VAR}`,
            // `${!VAR}` — but not Jinja `${{ var }}`, `$1`, `$@` or `$(cmd)`.
            Self::Bash => &[&BASH_VAR],
            // `%VAR%`, `%VAR:a=b%` and delayed expansion `!VAR!`.
            Self::CmdExe => &[&CMD_PERCENT_VAR, &CMD_DELAYED_VAR],
            // `$env:VAR`, `${env:VAR}` and `[Environment]::GetEnvironmentVariable("VAR")`.
            Self::PowerShell => &[&POWERSHELL_ENV_VAR, &POWERSHELL_GETENV],
            // `os.environ["VAR"]`, `os.environ.get("VAR")` and `os.getenv("VAR")`.
            Self::Python => &[&PYTHON_ENVIRON, &PYTHON_GETENV],
            // `$env.VAR` and `$env."SOME VAR"`.
            Self::NuShell => &[&NUSHELL_ENV_VAR, &NUSHELL_ENV_QUOTED],
            // `$ENV{VAR}`, `$ENV{'VAR'}`.
            Self::Perl => &[&PERL_ENV_VAR],
            // `Sys.getenv("VAR")`.
            Self::R => &[&R_GETENV],
            // `ENV["VAR"]` and `ENV.fetch("VAR")`.
            Self::Ruby => &[&RUBY_ENV_VAR],
            // `process.env.VAR` and `process.env["VAR"]`.
            Self::NodeJs => &[&NODEJS_ENV_DOT, &NODEJS_ENV_INDEX],
        };

        let mut variables = BTreeSet::new();
        for pattern in patterns {
            for captures in pattern.captures_iter(content) {
                if let Some(name) = captures.get(1) {
                    variables.insert(name.as_str().to_string());
                }
            }
        }
        variables
    }
}

/// A valid environment variable identifier.
const IDENT: &str = "[A-Za-z_][A-Za-z0-9_]*";

macro_rules! lazy_regex {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(&$pattern.replace("{IDENT}", IDENT)).expect("valid regex"));
    };
}

// `${VAR...}` (also `${#VAR}` / `${!VAR}`) or plain `$VAR`. Jinja templates
// (`${{ var }}`) do not match because `{` is not a valid identifier start.
lazy_regex!(BASH_VAR, r"\$(?:\{[!#]?)?({IDENT})");
lazy_regex!(CMD_PERCENT_VAR, r"%({IDENT})(?::[^%\r\n]*)?%");
lazy_regex!(CMD_DELAYED_VAR, r"!({IDENT})!");
lazy_regex!(POWERSHELL_ENV_VAR, r"(?i)\$\{?env:({IDENT})\}?");
lazy_regex!(
    POWERSHELL_GETENV,
    r#"(?i)\[(?:System\.)?Environment\]\s*::\s*GetEnvironmentVariable\(\s*["']({IDENT})["']"#
);
lazy_regex!(
    PYTHON_ENVIRON,
    r#"environ\s*(?:\.\s*get\s*\(|\[)\s*["']({IDENT})["']"#
);
lazy_regex!(
    PYTHON_GETENV,
    r#"os\s*\.\s*getenv\s*\(\s*["']({IDENT})["']"#
);
lazy_regex!(NUSHELL_ENV_VAR, r"\$env\.({IDENT})");
lazy_regex!(NUSHELL_ENV_QUOTED, r#"\$env\."([^"]+)""#);
lazy_regex!(PERL_ENV_VAR, r#"\$ENV\{\s*["']?({IDENT})["']?\s*\}"#);
lazy_regex!(R_GETENV, r#"Sys\.getenv\(\s*["']({IDENT})["']"#);
lazy_regex!(
    RUBY_ENV_VAR,
    r#"ENV\s*(?:\[\s*|\.\s*fetch\s*\(\s*)["']({IDENT})["']"#
);
lazy_regex!(NODEJS_ENV_DOT, r"process\s*\.\s*env\s*\.\s*({IDENT})");
lazy_regex!(
    NODEJS_ENV_INDEX,
    r#"process\s*\.\s*env\s*\[\s*["']({IDENT})["']"#
);

/// Returns the script file extensions to try for a target platform, in
/// priority order (mirrors [`crate::platform_script_extensions`], but for an
/// explicit platform instead of the compile-time host).
pub fn script_extensions_for_platform(is_windows: bool) -> &'static [&'static str] {
    if is_windows { &["bat", "ps1"] } else { &["sh"] }
}

/// Resolve a script path the same way script execution does: relative paths
/// are anchored at the recipe directory and extension-less paths try the
/// platform extensions in order.
fn find_script_file(recipe_dir: &Path, extensions: &[&str], path: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        recipe_dir.join(path)
    };

    if path.extension().is_none() {
        extensions
            .iter()
            .map(|ext| path.with_extension(ext))
            .find(|p| p.is_file())
    } else if path.is_file() {
        Some(path)
    } else {
        None
    }
}

impl Script {
    /// Detect environment variables referenced by this script, using the
    /// common access spellings of the script's interpreter.
    ///
    /// This is a best-effort, read-only scan:
    /// - inline content is scanned directly (Jinja templates like
    ///   `${{ var }}` are ignored by the scanners),
    /// - file-backed scripts (including the default `build.sh` / `build.bat`
    ///   discovery) are read from `recipe_dir` when it is provided,
    /// - missing or unreadable files simply contribute no variables.
    ///
    /// The dialect is chosen from the explicit interpreter first, then from
    /// the script file extension, and falls back to the native shell of the
    /// target platform (`bash` on Unix, `cmd.exe` on Windows).
    pub fn detect_used_variables(
        &self,
        recipe_dir: Option<&Path>,
        is_windows: bool,
    ) -> BTreeSet<String> {
        let extensions = script_extensions_for_platform(is_windows);
        let explicit_dialect = self
            .interpreter
            .as_deref()
            .and_then(ScriptDialect::from_interpreter);
        let fallback_dialect =
            explicit_dialect.unwrap_or_else(|| ScriptDialect::default_for_platform(is_windows));

        let scan_file = |path: &Path| -> BTreeSet<String> {
            let Some(recipe_dir) = recipe_dir else {
                return BTreeSet::new();
            };
            let Some(resolved) = find_script_file(recipe_dir, extensions, path) else {
                return BTreeSet::new();
            };
            let Ok(content) = fs_err::read_to_string(&resolved) else {
                return BTreeSet::new();
            };
            let dialect = explicit_dialect
                .or_else(|| ScriptDialect::from_path(&resolved))
                .unwrap_or(fallback_dialect);
            dialect.extract_env_variables(&content)
        };

        match &self.content {
            ScriptContent::Default => scan_file(Path::new("build")),
            ScriptContent::Path(path) => scan_file(path),
            ScriptContent::CommandOrPath(command_or_path) => {
                // Single-line strings may reference a script file; multi-line
                // strings are always inline content.
                if !command_or_path.contains('\n') {
                    let from_file = scan_file(Path::new(command_or_path));
                    if !from_file.is_empty() {
                        return from_file;
                    }
                    // If the string names an existing but empty/unreadable
                    // file we may still fall through here; scanning the file
                    // name as inline content is harmless.
                }
                fallback_dialect.extract_env_variables(command_or_path)
            }
            ScriptContent::Commands(commands) => {
                let mut variables = BTreeSet::new();
                for command in commands {
                    variables.extend(fallback_dialect.extract_env_variables(command));
                }
                variables
            }
            ScriptContent::Command(command) => fallback_dialect.extract_env_variables(command),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(dialect: ScriptDialect, content: &str) -> Vec<String> {
        dialect.extract_env_variables(content).into_iter().collect()
    }

    #[test]
    fn bash_spellings() {
        let script = r#"
            #!/bin/bash
            cmake -DCMAKE_INSTALL_PREFIX=$PREFIX ..
            echo "target: ${TARGET}"
            export CFLAGS="${CFLAGS:-} -O2"
            suffix=${VERSION%%.*}
            len=${#NAME}
            indirect=${!REF}
        "#;
        assert_eq!(
            extract(ScriptDialect::Bash, script),
            ["CFLAGS", "NAME", "PREFIX", "REF", "TARGET", "VERSION"]
        );
    }

    #[test]
    fn bash_skips_positional_special_and_jinja() {
        let script = r#"
            echo $1 $@ $? $# $$
            echo ${{ jinja_var }}
            result=$(uname)
            math=$((3 + 4))
        "#;
        assert_eq!(extract(ScriptDialect::Bash, script), Vec::<String>::new());
    }

    #[test]
    fn cmd_spellings() {
        let script = r#"
            @echo off
            cmake -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% ..
            echo %TARGET%
            set "TRIMMED=%VERSION:.=_%"
            if "!DELAYED!" == "1" echo on
            for %%i in (a b c) do echo %%i
            nmake /f Makefile.vc %1
        "#;
        assert_eq!(
            extract(ScriptDialect::CmdExe, script),
            ["DELAYED", "LIBRARY_PREFIX", "TARGET", "VERSION"]
        );
    }

    #[test]
    fn powershell_spellings() {
        let script = r#"
            Write-Host $env:TARGET
            Write-Host ${env:LIBRARY_PREFIX}
            $v = [Environment]::GetEnvironmentVariable("PKG_VERSION")
            $w = [System.Environment]::GetEnvironmentVariable('PKG_NAME')
        "#;
        assert_eq!(
            extract(ScriptDialect::PowerShell, script),
            ["LIBRARY_PREFIX", "PKG_NAME", "PKG_VERSION", "TARGET"]
        );
    }

    #[test]
    fn python_spellings() {
        let script = r#"
            import os
            from os import environ
            target = os.environ["TARGET"]
            prefix = os.environ.get("PREFIX", "/opt")
            version = os.getenv('PKG_VERSION')
            name = environ['PKG_NAME']
        "#;
        assert_eq!(
            extract(ScriptDialect::Python, script),
            ["PKG_NAME", "PKG_VERSION", "PREFIX", "TARGET"]
        );
    }

    #[test]
    fn nushell_spellings() {
        let script = r#"
            echo $env.TARGET
            echo $env."MY VAR"
        "#;
        assert_eq!(
            extract(ScriptDialect::NuShell, script),
            ["MY VAR", "TARGET"]
        );
    }

    #[test]
    fn perl_spellings() {
        let script = r#"
            my $target = $ENV{TARGET};
            my $prefix = $ENV{'PREFIX'};
            my $name = $ENV{ "PKG_NAME" };
        "#;
        assert_eq!(
            extract(ScriptDialect::Perl, script),
            ["PKG_NAME", "PREFIX", "TARGET"]
        );
    }

    #[test]
    fn r_spellings() {
        let script = r#"
            target <- Sys.getenv("TARGET")
            prefix <- Sys.getenv('PREFIX', unset = "/opt")
        "#;
        assert_eq!(extract(ScriptDialect::R, script), ["PREFIX", "TARGET"]);
    }

    #[test]
    fn ruby_spellings() {
        let script = r#"
            target = ENV["TARGET"]
            prefix = ENV.fetch('PREFIX', '/opt')
        "#;
        assert_eq!(extract(ScriptDialect::Ruby, script), ["PREFIX", "TARGET"]);
    }

    #[test]
    fn nodejs_spellings() {
        let script = r#"
            const target = process.env.TARGET;
            const prefix = process.env["PREFIX"];
        "#;
        assert_eq!(extract(ScriptDialect::NodeJs, script), ["PREFIX", "TARGET"]);
    }

    #[test]
    fn interpreter_name_mapping() {
        assert_eq!(
            ScriptDialect::from_interpreter("brush"),
            Some(ScriptDialect::Bash)
        );
        assert_eq!(
            ScriptDialect::from_interpreter("nu"),
            Some(ScriptDialect::NuShell)
        );
        assert_eq!(
            ScriptDialect::from_interpreter("rscript"),
            Some(ScriptDialect::R)
        );
        assert_eq!(ScriptDialect::from_interpreter("unknown"), None);
    }

    #[test]
    fn default_script_discovery_scans_build_sh() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(
            dir.path().join("build.sh"),
            "cmake -DCMAKE_C_COMPILER_TARGET=${TARGET} ..\n",
        )
        .unwrap();

        let script = Script::default();
        let vars = script.detect_used_variables(Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["TARGET"]);

        // Without a recipe dir nothing is scanned.
        let vars = script.detect_used_variables(None, false);
        assert!(vars.is_empty());
    }

    #[test]
    fn default_script_discovery_scans_build_bat_on_windows_target() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(
            dir.path().join("build.bat"),
            "cmake -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% -DTARGET=%TARGET% ..\r\n",
        )
        .unwrap();

        let script = Script::default();
        let vars = script.detect_used_variables(Some(dir.path()), true);
        assert_eq!(
            vars.into_iter().collect::<Vec<_>>(),
            ["LIBRARY_PREFIX", "TARGET"]
        );
    }

    #[test]
    fn explicit_file_uses_extension_dialect() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(
            dir.path().join("install.py"),
            "import os\nprint(os.environ['TARGET'])\n",
        )
        .unwrap();

        let script = Script {
            content: ScriptContent::Path(PathBuf::from("install.py")),
            ..Default::default()
        };
        let vars = script.detect_used_variables(Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["TARGET"]);
    }

    #[test]
    fn command_or_path_resolves_files_and_falls_back_to_inline() {
        let dir = tempfile::tempdir().unwrap();
        fs_err::write(dir.path().join("build.sh"), "echo ${FROM_FILE}\n").unwrap();

        // Resolves to the file next to the recipe.
        let script = Script {
            content: ScriptContent::CommandOrPath("build.sh".to_string()),
            ..Default::default()
        };
        let vars = script.detect_used_variables(Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["FROM_FILE"]);

        // Not a file: treated as an inline command.
        let script = Script {
            content: ScriptContent::CommandOrPath("echo $INLINE_VAR".to_string()),
            ..Default::default()
        };
        let vars = script.detect_used_variables(Some(dir.path()), false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["INLINE_VAR"]);
    }

    #[test]
    fn inline_commands_use_explicit_interpreter() {
        let script = Script {
            interpreter: Some("python".to_string()),
            content: ScriptContent::Commands(vec![
                "import os".to_string(),
                "print(os.environ['TARGET'])".to_string(),
            ]),
            ..Default::default()
        };
        let vars = script.detect_used_variables(None, false);
        assert_eq!(vars.into_iter().collect::<Vec<_>>(), ["TARGET"]);
    }
}
