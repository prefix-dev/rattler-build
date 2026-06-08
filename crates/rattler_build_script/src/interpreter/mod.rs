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
) -> Option<PathBuf> {
    let exe_name = format!("{}{}", name, std::env::consts::EXE_SUFFIX);

    // Build-prefix-only: search just the prefix bin entries, no PATH fallback.
    if let InterpreterSearchScope::BuildPrefixOnly = scope {
        let build_prefix = build_prefix?;
        let prefix_path = prefix_path_entries(build_prefix, platform);
        return which::which_in_global(exe_name, std::env::join_paths(prefix_path).ok())
            .ok()?
            .next();
    }

    let path = std::env::var("PATH").unwrap_or_default();
    if let Some(build_prefix) = build_prefix {
        let mut prepend_path = prefix_path_entries(build_prefix, platform)
            .into_iter()
            .collect::<Vec<_>>();
        prepend_path.extend(std::env::split_paths(&path));
        return which::which_in_global(exe_name, std::env::join_paths(prepend_path).ok())
            .ok()?
            .next();
    }

    which::which_in_global(exe_name, Some(path)).ok()?.next()
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
                Some(path) => match self.is_usable_executable(&path) {
                    Ok(()) => return Ok(path),
                    Err(err) => unusable_candidate = Some((path, err)),
                },
                None => continue,
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

fn interpreter_invocation(interpreter: &str) -> Option<Box<dyn InterpreterInvocation>> {
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

/// An interpreter selected from a recipe, pairing the user-facing name with its invocation behavior.
pub(crate) struct SelectedInterpreter {
    user_name: String,
    invocation: Box<dyn InterpreterInvocation>,
}

impl SelectedInterpreter {
    /// Look up the interpreter by its recipe name, returning None if unsupported.
    pub(crate) fn from_recipe_name(name: &str) -> Option<Self> {
        interpreter_invocation(name).map(|invocation| Self {
            user_name: name.to_string(),
            invocation,
        })
    }

    /// Returns the default file extension for scripts run by this interpreter.
    pub(crate) fn extension(&self) -> &'static str {
        self.invocation.extension()
    }

    /// Returns the contents to write to files executed by this interpreter.
    pub(crate) fn script_contents(&self, raw: &str) -> String {
        self.invocation.script_contents(raw)
    }

    /// Returns the argument values following the executable for the given script file.
    pub(crate) fn args(&self, script_path: &Path) -> Vec<String> {
        self.invocation.args(script_path)
    }

    /// Resolve the executable, remapping internal errors to the user-facing name.
    pub(crate) fn resolve_executable(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<PathBuf, InterpreterError> {
        self.invocation
            .resolve_executable(build_prefix, platform)
            .map_err(|err| match err {
                InterpreterError::InterpreterNotFound(_) => {
                    InterpreterError::InterpreterNotFound(self.user_name.clone())
                }
                InterpreterError::InvalidInterpreter { reason, .. } => {
                    InterpreterError::InvalidInterpreter {
                        interpreter: self.user_name.clone(),
                        reason,
                    }
                }
                other => other,
            })
    }
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
        let bin_dir = prefix_path_entries(prefix, &Platform::current())
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

    /// Stub interpreter that exercises the candidate iteration and
    /// `is_usable_executable` rejection path of the default
    /// `resolve_executable`. It searches the build prefix only and tries two
    /// executable names; the first name is always rejected by validation.
    struct RejectFirstStub;

    impl InterpreterInvocation for RejectFirstStub {
        fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
            &["stub_first", "stub_second"]
        }

        fn search_scope(&self, _build_platform: &Platform) -> InterpreterSearchScope {
            InterpreterSearchScope::BuildPrefixOnly
        }

        fn extension(&self) -> &'static str {
            "stub"
        }

        fn is_usable_executable(&self, executable: &Path) -> Result<(), InterpreterError> {
            // Reject any candidate whose file stem is `stub_first`.
            let stem = executable
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem == "stub_first" {
                Err(InterpreterError::InvalidInterpreter {
                    interpreter: executable.display().to_string(),
                    reason: "rejected by test stub".to_string(),
                })
            } else {
                Ok(())
            }
        }

        fn args(&self, script_path: &Path) -> Vec<String> {
            vec![script_path.to_string_lossy().into_owned()]
        }
    }

    /// Case A: the first candidate is rejected by `is_usable_executable`, so
    /// lookup continues and returns the second (usable) candidate.
    #[test]
    fn rejected_first_candidate_falls_through_to_second() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        create_fake_executable(&prefix, "stub_first");
        let second = create_fake_executable(&prefix, "stub_second");

        let stub = RejectFirstStub;
        let resolved = stub
            .resolve_executable(Some(&prefix), &Platform::current())
            .expect("second candidate should resolve");
        assert_eq!(resolved, second);
    }

    /// Case B: the only available candidate is rejected, so
    /// `resolve_executable` reports `InvalidInterpreter`. Going through
    /// `SelectedInterpreter` is not possible for a test-only stub, so the
    /// recipe-name remapping is asserted by exercising the same `map_err`
    /// behavior directly via a real factory interpreter below.
    #[test]
    fn only_candidate_rejected_yields_invalid_interpreter() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let first = create_fake_executable(&prefix, "stub_first");

        let stub = RejectFirstStub;
        let err = stub
            .resolve_executable(Some(&prefix), &Platform::current())
            .expect_err("only candidate is rejected");
        match err {
            InterpreterError::InvalidInterpreter { interpreter, .. } => {
                // The raw error carries the executable path, not a user-facing name.
                assert_eq!(interpreter, first.display().to_string());
            }
            other => panic!("expected InvalidInterpreter, got {other:?}"),
        }
    }

    /// Wrapping a `SelectedInterpreter` around an invocation remaps the
    /// `InvalidInterpreter.interpreter` field from the executable path to the
    /// user-facing recipe name. We exercise this with the powershell factory
    /// interpreter, whose `is_usable_executable` never errors, so we instead
    /// assert the remapping logic via a directly-constructed SelectedInterpreter
    /// whose invocation always rejects.
    #[test]
    fn selected_interpreter_remaps_invalid_to_user_name() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        create_fake_executable(&prefix, "stub_first");

        let selected = SelectedInterpreter {
            user_name: "my-recipe-interp".to_string(),
            invocation: Box::new(RejectFirstStub),
        };

        let err = selected
            .resolve_executable(Some(&prefix), &Platform::current())
            .expect_err("only candidate is rejected");
        match err {
            InterpreterError::InvalidInterpreter {
                interpreter,
                reason,
            } => {
                assert_eq!(interpreter, "my-recipe-interp");
                assert!(reason.contains("rejected by test stub"), "reason: {reason}");
            }
            other => panic!("expected InvalidInterpreter, got {other:?}"),
        }
    }

    /// Language interpreters must not leak from the system PATH: `find_interpreter`
    /// with `PrefixThenSystemPath` finds an exe present only on PATH, while
    /// `BuildPrefixOnly` does not.
    #[test]
    #[serial_test::serial]
    fn search_scope_path_fallback_vs_prefix_only() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        // Place a fake exe in a separate dir that we prepend onto PATH, NOT in
        // the build prefix.
        let path_dir = tmp.path().join("path_only");
        fs::create_dir_all(&path_dir).unwrap();
        let exe_name = format!("rb_path_only_tool{}", std::env::consts::EXE_SUFFIX);
        let exe = path_dir.join(&exe_name);
        fs::write(&exe, "").unwrap();
        #[cfg(unix)]
        {
            use std::{fs::Permissions, os::unix::fs::PermissionsExt};
            fs::set_permissions(&exe, Permissions::from_mode(0o755)).unwrap();
        }

        let original_path = std::env::var_os("PATH");
        let mut new_paths = vec![path_dir.clone()];
        if let Some(orig) = &original_path {
            new_paths.extend(std::env::split_paths(orig));
        }
        // SAFETY: env mutation is serialized via #[serial] and restored below.
        unsafe {
            std::env::set_var("PATH", std::env::join_paths(&new_paths).unwrap());
        }

        let platform = Platform::current();
        let found_via_path = find_interpreter(
            "rb_path_only_tool",
            Some(&prefix),
            &platform,
            InterpreterSearchScope::PrefixThenSystemPath,
        );
        let found_prefix_only = find_interpreter(
            "rb_path_only_tool",
            Some(&prefix),
            &platform,
            InterpreterSearchScope::BuildPrefixOnly,
        );

        // Restore PATH before asserting so a failure does not corrupt later tests.
        // SAFETY: env mutation is serialized via #[serial].
        unsafe {
            match original_path {
                Some(orig) => std::env::set_var("PATH", orig),
                None => std::env::remove_var("PATH"),
            }
        }

        assert!(
            found_via_path.is_some(),
            "PrefixThenSystemPath should find the exe on PATH"
        );
        assert_eq!(found_via_path.unwrap(), exe);
        assert!(
            found_prefix_only.is_none(),
            "BuildPrefixOnly must not find an exe that lives only on PATH"
        );
    }

    /// PowerShell tries `pwsh` first then `powershell` on the windows branch.
    /// This documents the candidate order without resolving (the real invocation
    /// uses `PrefixThenSystemPath`, so a system `pwsh` would leak into resolution).
    #[test]
    fn powershell_lists_pwsh_then_powershell_on_windows() {
        let names = super::powershell::PowerShellInvocation.executable_names(&Platform::Win64);
        assert_eq!(names, &["pwsh", "powershell"]);
    }

    /// When the first executable name is absent from the prefix, resolution falls
    /// through to a later name. Uses the `BuildPrefixOnly` stub so the system PATH
    /// cannot leak a real `stub_first`/`stub_second` into the result. This covers
    /// the `find_interpreter == None` continue branch (distinct from the
    /// `is_usable_executable` rejection branch above), mirroring how PowerShell
    /// falls through from `pwsh` to `powershell`.
    #[test]
    fn missing_first_candidate_falls_through_to_second() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        // Create only the second candidate; `stub_first` is never found on disk.
        let second = create_fake_executable(&prefix, "stub_second");

        let resolved = RejectFirstStub
            .resolve_executable(Some(&prefix), &Platform::current())
            .expect("second candidate should resolve when the first is absent");
        assert_eq!(resolved, second);
    }

    /// On Windows, cmd resolution special-cases `COMSPEC` when it points at
    /// `cmd.exe`, returning it verbatim.
    #[cfg(windows)]
    #[test]
    #[serial_test::serial]
    fn cmd_uses_comspec_special_case() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_cmd = tmp.path().join("system32").join("cmd.exe");
        fs::create_dir_all(fake_cmd.parent().unwrap()).unwrap();
        fs::write(&fake_cmd, "").unwrap();

        let original = std::env::var_os("COMSPEC");
        // SAFETY: env mutation is serialized via #[serial] and restored below.
        unsafe {
            std::env::set_var("COMSPEC", &fake_cmd);
        }

        let resolved = super::cmd_exe::CmdExeInvocation.resolve_executable(None, &Platform::Win64);

        // SAFETY: env mutation is serialized via #[serial].
        unsafe {
            match original {
                Some(orig) => std::env::set_var("COMSPEC", orig),
                None => std::env::remove_var("COMSPEC"),
            }
        }

        assert_eq!(resolved.unwrap(), fake_cmd);
    }

    /// Recipe-name factory mapping to file extensions.
    #[test]
    fn factory_maps_recipe_names_to_extensions() {
        assert_eq!(
            SelectedInterpreter::from_recipe_name("nushell")
                .unwrap()
                .extension(),
            "nu"
        );
        assert_eq!(
            SelectedInterpreter::from_recipe_name("nu")
                .unwrap()
                .extension(),
            "nu"
        );
        assert_eq!(
            SelectedInterpreter::from_recipe_name("brush")
                .unwrap()
                .extension(),
            "sh"
        );
        assert_eq!(
            SelectedInterpreter::from_recipe_name("python")
                .unwrap()
                .extension(),
            "py"
        );
    }
}
