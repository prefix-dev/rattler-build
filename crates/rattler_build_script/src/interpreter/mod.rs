//! Support for specialized script interpreters.
//!
//! This module maps recipe `interpreter` values and inferred file extensions to
//! [`InterpreterInvocation`] implementations. It resolves interpreter executables
//! with [`find_interpreter`] and reports failures through [`InterpreterError`].
//! Native wrapper execution lives in `crate::native_runner`.

mod bash;
mod brush;
mod cmd_exe;
mod nodejs;
mod nushell;
mod perl;
mod powershell;
mod python;
mod r;
mod ruby;

use std::path::{Path, PathBuf};

use rattler_conda_types::Platform;
use rattler_shell::activation::prefix_path_entries;

/// Describes interpreter execution and lookup errors.
#[derive(Debug, thiserror::Error)]
pub enum InterpreterError {
    /// Indicates that script execution failed.
    #[error("IO Error: {0}")]
    ExecutionFailed(#[from] std::io::Error),

    /// Indicates that the interpreter executable could not be located.
    #[error("interpreter '{0}' was not found in the build environment")]
    InterpreterNotFound(String),

    /// Indicates that the interpreter executable was found but rejected by validation.
    #[error("interpreter '{interpreter}' was found but is not valid: {reason}")]
    InvalidInterpreter { interpreter: String, reason: String },
}

/// Defines where to look for an interpreter executable.
#[derive(Debug, Clone, Copy)]
pub enum InterpreterSearchScope {
    /// Searches through the build prefix, then the system PATH.
    PrefixThenSystemPath,
    /// Searches only the build prefix.
    BuildPrefixOnly,
}

pub(crate) fn find_interpreter(
    name: &str,
    build_prefix: Option<&PathBuf>,
    platform: &Platform,
    scope: InterpreterSearchScope,
) -> Result<Option<PathBuf>, which::Error> {
    let exe_name = format!("{}{}", name, std::env::consts::EXE_SUFFIX);

    // Build-prefix-only: search just the prefix bin entries, no PATH fallback.
    if let InterpreterSearchScope::BuildPrefixOnly = scope {
        let Some(build_prefix) = build_prefix else {
            return Ok(None);
        };
        let prefix_path = prefix_path_entries(build_prefix, platform);
        return Ok(
            which::which_in_global(exe_name, std::env::join_paths(prefix_path).ok())?.next(),
        );
    }

    let path = std::env::var("PATH").unwrap_or_default();
    if let Some(build_prefix) = build_prefix {
        let mut prepend_path = prefix_path_entries(build_prefix, platform)
            .into_iter()
            .collect::<Vec<_>>();
        prepend_path.extend(std::env::split_paths(&path));
        return Ok(
            which::which_in_global(exe_name, std::env::join_paths(prepend_path).ok())?.next(),
        );
    }

    Ok(which::which_in_global(exe_name, Some(path))?.next())
}

/// Describes how a specialized interpreter executes a script file.
pub(crate) trait InterpreterInvocation: Send + Sync {
    /// Returns executable names to try for this interpreter.
    ///
    /// Recipe-facing interpreter names may differ from executable names, for
    /// example `nushell` maps to `nu`.
    fn executable_names(&self, build_platform: &Platform) -> &'static [&'static str];

    /// Returns where the executable should be searched for.
    ///
    /// Implementations can opt into platform-specific system tools such as Unix
    /// `bash` or Windows `cmd`; language interpreters default to the build
    /// prefix to avoid host-tool leakage.
    fn search_scope(&self, _build_platform: &Platform) -> InterpreterSearchScope {
        InterpreterSearchScope::BuildPrefixOnly
    }

    /// Returns the default extension for files executed by this interpreter.
    ///
    /// The execution layer uses this for inline script blocks before invoking
    /// the interpreter from the native wrapper.
    fn extension(&self) -> &'static str;

    /// Returns the contents to write to files executed by this interpreter.
    ///
    /// Interpreters can add a small prologue around user code; most return the
    /// script unchanged.
    fn script_contents(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// Checks whether an executable candidate is usable for this interpreter.
    ///
    /// Implementations can reject a found executable and let lookup continue
    /// with later candidates.
    fn is_usable_executable(&self, _executable: &Path) -> Result<(), InterpreterError> {
        Ok(())
    }

    /// Returns an executable for this interpreter.
    ///
    /// The default implementation provides shared lookup behavior; interpreters
    /// can override it for platform-specific behavior.
    fn resolve_executable(
        &self,
        build_prefix: Option<&PathBuf>,
        build_platform: &Platform,
    ) -> Result<PathBuf, InterpreterError> {
        let scope = self.search_scope(build_platform);
        let mut unusable_candidate = None;

        for executable_name in self.executable_names(build_platform) {
            match find_interpreter(executable_name, build_prefix, build_platform, scope) {
                Ok(Some(path)) => match self.is_usable_executable(&path) {
                    Ok(()) => return Ok(path),
                    Err(err) => unusable_candidate = Some((path, err)),
                },
                Ok(None) | Err(_) => continue,
            }
        }

        if let Some((path, err)) = unusable_candidate {
            return Err(InterpreterError::InvalidInterpreter {
                interpreter: path.display().to_string(),
                reason: err.to_string(),
            });
        }

        Err(InterpreterError::InterpreterNotFound(
            self.executable_names(build_platform)
                .first()
                .copied()
                .unwrap_or("<unknown>")
                .to_string(),
        ))
    }

    /// Returns raw argument values after the executable.
    ///
    /// Each interpreter can provide its own flags and convention for receiving
    /// the script file path.
    fn args(&self, script_path: &Path) -> Vec<String>;
}

pub(crate) fn interpreter_invocation(interpreter: &str) -> Option<Box<dyn InterpreterInvocation>> {
    match interpreter {
        "bash" => Some(Box::new(bash::BashInvocation)),
        "cmd" => Some(Box::new(cmd_exe::CmdExeInvocation)),
        "brush" => Some(Box::new(brush::BrushInvocation)),
        "nushell" | "nu" => Some(Box::new(nushell::NuShellInvocation)),
        "python" => Some(Box::new(python::PythonInvocation)),
        "perl" => Some(Box::new(perl::PerlInvocation)),
        "rscript" => Some(Box::new(r::RInvocation)),
        "ruby" => Some(Box::new(ruby::RubyInvocation)),
        "node" | "nodejs" => Some(Box::new(nodejs::NodeJsInvocation)),
        "powershell" => Some(Box::new(powershell::PowerShellInvocation)),
        _ => None,
    }
}

pub(crate) fn resolve_interpreter_executable(
    build_prefix: Option<&PathBuf>,
    build_platform: &Platform,
    user_interpreter: &str,
    invocation: &dyn InterpreterInvocation,
) -> Result<PathBuf, InterpreterError> {
    invocation
        .resolve_executable(build_prefix, build_platform)
        .map_err(|err| match err {
            InterpreterError::InterpreterNotFound(_) => {
                InterpreterError::InterpreterNotFound(user_interpreter.to_string())
            }
            InterpreterError::InvalidInterpreter { reason, .. } => {
                InterpreterError::InvalidInterpreter {
                    interpreter: user_interpreter.to_string(),
                    reason,
                }
            }
            other => other,
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{ExecutionArgs, ResolvedScriptContents};
    use fs_err as fs;
    use indexmap::IndexMap;
    use rattler_conda_types::Platform;
    use rattler_shell::activation::prefix_path_entries;
    use std::path::{Path, PathBuf};

    fn execution_args(
        work_dir: PathBuf,
        run_prefix: PathBuf,
        script: ResolvedScriptContents,
        interpreter: Option<&str>,
    ) -> ExecutionArgs {
        ExecutionArgs {
            script,
            interpreter: interpreter.map(str::to_string),
            env_vars: IndexMap::new(),
            secrets: IndexMap::new(),
            execution_platform: Platform::current(),
            build_prefix: None,
            run_prefix,
            work_dir,
            sandbox_config: None,
            env_isolation: crate::execution::EnvironmentIsolation::None,
        }
    }

    fn native_build_script_path(work_dir: &Path) -> PathBuf {
        work_dir.join(if cfg!(windows) {
            "conda_build.bat"
        } else {
            "conda_build.sh"
        })
    }

    fn create_fake_executable(prefix: &Path, name: &str) -> PathBuf {
        let exe_name = format!("{}{}", name, std::env::consts::EXE_SUFFIX);
        let bin_dir = prefix_path_entries(&prefix.to_path_buf(), &Platform::current())
            .into_iter()
            .next()
            .expect("prefix has executable path entries");
        fs::create_dir_all(&bin_dir).unwrap();
        let exe = bin_dir.join(exe_name);
        fs::write(&exe, "").unwrap();
        #[cfg(unix)]
        {
            use std::{fs::Permissions, os::unix::fs::PermissionsExt};
            fs::set_permissions(&exe, Permissions::from_mode(0o755)).unwrap();
        }
        exe
    }

    #[tokio::test]
    async fn inline_without_interpreter_is_native_body() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("echo native".to_string()),
            None,
        );

        crate::execution::generate_build_script(&args)
            .await
            .unwrap();

        let build_script = fs::read_to_string(native_build_script_path(tmp.path())).unwrap();
        assert!(build_script.contains("echo native"));
        assert!(!tmp.path().join("conda_build_script.py").exists());
    }

    #[tokio::test]
    async fn explicit_interpreter_writes_script_file_and_invocation() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let python = create_fake_executable(&prefix, "python");

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("print('script file')".to_string()),
            Some("python"),
        );

        crate::execution::generate_build_script(&args)
            .await
            .unwrap();

        let script_file = tmp.path().join("conda_build_script.py");
        assert_eq!(
            fs::read_to_string(&script_file).unwrap(),
            "print('script file')"
        );

        let build_script = fs::read_to_string(native_build_script_path(tmp.path())).unwrap();
        assert!(build_script.contains(&python.to_string_lossy().to_string()));
        assert!(build_script.contains(&script_file.to_string_lossy().to_string()));
    }

    #[tokio::test]
    async fn inferred_file_interpreter_invokes_original_path() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        create_fake_executable(&prefix, "python");
        let source_script = tmp.path().join("build.py");

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Path(source_script.clone(), "print('from file')".to_string()),
            None,
        );

        crate::execution::generate_build_script(&args)
            .await
            .unwrap();

        assert!(!tmp.path().join("conda_build_script.py").exists());
        let build_script = fs::read_to_string(native_build_script_path(tmp.path())).unwrap();
        assert!(build_script.contains(&source_script.to_string_lossy().to_string()));
        assert!(!build_script.contains("print('from file')"));
    }

    #[tokio::test]
    async fn unknown_file_extension_without_interpreter_is_native_body() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Path(
                tmp.path().join("build.custom"),
                "echo custom".to_string(),
            ),
            None,
        );

        crate::execution::generate_build_script(&args)
            .await
            .unwrap();

        let build_script = fs::read_to_string(native_build_script_path(tmp.path())).unwrap();
        assert!(build_script.contains("echo custom"));
    }

    #[tokio::test]
    async fn build_prefix_only_interpreter_missing_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("print('missing')".to_string()),
            Some("python"),
        );

        let err = crate::execution::generate_build_script(&args)
            .await
            .unwrap_err();
        assert!(
            matches!(err, InterpreterError::InterpreterNotFound(ref name) if name == "python"),
            "expected missing python error, got {err:?}"
        );
    }
}
