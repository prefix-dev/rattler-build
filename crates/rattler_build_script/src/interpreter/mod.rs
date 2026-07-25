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

use crate::ExecutionContext;

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

    /// Indicates that the recipe requested an interpreter that rattler-build
    /// does not support (e.g. a typo like `brus` instead of `brush`).
    #[error("unsupported interpreter '{0}'")]
    UnsupportedInterpreter(String),
}

/// Constructs the invocation behavior for a supported interpreter.
type InvocationFactory = fn() -> Box<dyn InterpreterInvocation>;

/// Recipe-facing interpreter names and their invocation constructors. This is
/// the single source of truth for which `build.script.interpreter` values are
/// supported; lookup and typo suggestions both derive from it.
const INTERPRETERS: &[(&str, InvocationFactory)] = &[
    ("bash", || Box::new(bash::BashInvocation)),
    ("cmd", || Box::new(cmd_exe::CmdExeInvocation)),
    ("brush", || Box::new(brush::BrushInvocation)),
    ("nushell", || Box::new(nushell::NuShellInvocation)),
    ("nu", || Box::new(nushell::NuShellInvocation)),
    ("python", || Box::new(python::PythonInvocation)),
    ("perl", || Box::new(perl::PerlInvocation)),
    ("rscript", || Box::new(r::RInvocation)),
    ("ruby", || Box::new(ruby::RubyInvocation)),
    ("node", || Box::new(nodejs::NodeJsInvocation)),
    ("nodejs", || Box::new(nodejs::NodeJsInvocation)),
    ("powershell", || Box::new(powershell::PowerShellInvocation)),
];

/// Returns the supported interpreter name most similar to `name`, if it is
/// similar enough — so typos get a suggestion, unrelated names do not.
/// The comparison is case-insensitive.
pub fn closest_interpreter(name: &str) -> Option<&'static str> {
    /// Minimum similarity to treat a name as a typo of an interpreter.
    /// Jaro-Winkler rather than plain Jaro so that prefix-sharing near-misses
    /// like `pwsh` -> `powershell` (exactly 0.8 under Jaro, and prone to
    /// falling just below it in floating point) clear the threshold.
    const SIMILARITY_THRESHOLD: f64 = 0.8;

    let name = name.to_lowercase();
    INTERPRETERS
        .iter()
        .map(|(candidate, _)| (strsim::jaro_winkler(&name, candidate), *candidate))
        .filter(|(similarity, _)| *similarity >= SIMILARITY_THRESHOLD)
        .max_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, candidate)| candidate)
}

/// Describes where to look for an interpreter executable: which environments to
/// search and whether to fall back to the system `PATH`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InterpreterSearchScope {
    /// Search the build environment (the build prefix, or the host prefix when
    /// build and host are merged).
    search_build: bool,
    /// Search the host (run) prefix.
    search_host: bool,
    /// Fall back to the system `PATH`.
    system_fallback: bool,
}

impl InterpreterSearchScope {
    /// Search only the build environment. For interpreters that must come from
    /// the build environment for reproducibility (e.g. `brush`).
    pub(crate) fn build_only() -> Self {
        Self {
            search_build: true,
            search_host: false,
            system_fallback: false,
        }
    }

    /// Search the build and host environments, then fall back to the system
    /// `PATH`. Mirrors the activated `PATH` a script actually runs under.
    pub(crate) fn build_and_host_with_system_fallback() -> Self {
        Self {
            search_build: true,
            search_host: true,
            system_fallback: true,
        }
    }

    /// Whether this scope falls back to the system `PATH`.
    pub(crate) fn allows_system_fallback(&self) -> bool {
        self.system_fallback
    }
}

pub(crate) fn find_interpreter(
    name: &str,
    context: &ExecutionContext,
    scope: InterpreterSearchScope,
) -> Option<PathBuf> {
    let runtime = context.runtime();
    let exe_name = format!("{}{}", name, runtime.exe_suffix());

    let mut search_path = Vec::new();
    if scope.search_build {
        search_path.extend(prefix_path_entries(
            context.build().path(),
            &context.build().platform(),
        ));
    }
    if scope.search_host {
        search_path.extend(prefix_path_entries(
            context.host().path(),
            &context.host().platform(),
        ));
    }
    if scope.system_fallback {
        search_path.extend(std::env::split_paths(runtime.path()));
    }

    if search_path.is_empty() {
        return None;
    }
    // Prefix paths use their configured platform conventions. System fallback
    // uses the rattler-build process environment and host filesystem rules.
    which::which_in_global(exe_name, std::env::join_paths(search_path).ok())
        .ok()?
        .next()
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
    /// Defaults to the build environment only, for reproducibility. Native
    /// shells and language interpreters override this to also search the host
    /// environment and the system `PATH`; `brush` keeps the default.
    fn search_scope(&self, _build_platform: &Platform) -> InterpreterSearchScope {
        InterpreterSearchScope::build_only()
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

    /// Assembles a list of recipe commands into a single script.
    ///
    /// The default joins with newlines; interpreters needing explicit error
    /// propagation between commands (`cmd`) override it.
    fn join_commands(&self, commands: &[String]) -> String {
        commands.join("\n")
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
    fn resolve_executable(&self, context: &ExecutionContext) -> Result<PathBuf, InterpreterError> {
        let platform = context.build().platform();
        let scope = self.search_scope(&platform);
        let mut unusable_candidate = None;

        for executable_name in self.executable_names(&platform) {
            match find_interpreter(executable_name, context, scope) {
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
            self.executable_names(&platform)
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
    INTERPRETERS
        .iter()
        .find(|(name, _)| *name == interpreter)
        .map(|(_, invocation)| invocation())
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

    /// Assembles a list of recipe commands into a single script.
    pub(crate) fn join_commands(&self, commands: &[String]) -> String {
        self.invocation.join_commands(commands)
    }

    /// Returns the argument values following the executable for the given script file.
    pub(crate) fn args(&self, script_path: &Path) -> Vec<String> {
        self.invocation.args(script_path)
    }

    /// Resolve the executable, remapping internal errors to the user-facing name.
    pub(crate) fn resolve_executable(
        &self,
        context: &ExecutionContext,
    ) -> Result<PathBuf, InterpreterError> {
        self.invocation
            .resolve_executable(context)
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
    use crate::{
        ExecutionContext, RuntimeEnv,
        execution::{ExecutionArgs, ResolvedScriptContents},
    };
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
            context: ExecutionContext::shared(
                RuntimeEnv::current(),
                run_prefix,
                Platform::current(),
                Platform::current(),
            ),
            work_dir,
            sandbox_config: None,
            env_isolation: crate::execution::EnvironmentIsolation::None,
        }
    }

    fn shared_context(runtime: RuntimeEnv, prefix: &Path) -> ExecutionContext {
        let platform = runtime.process_platform();
        ExecutionContext::shared(runtime, prefix, platform, platform)
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

    /// An interpreter that matches the wrapper shell (`cmd` on Windows, `bash`
    /// on Unix) is inlined into the wrapper rather than resolved from the build
    /// environment and re-invoked. This must succeed even with an empty prefix:
    /// `cmd` in particular is a system shell, not a conda-provided executable,
    /// so resolving it from the build environment used to fail with
    /// "interpreter 'cmd' was not found in the build environment".
    /// Regression test for the `file: build` -> `build.bat` path on Windows.
    #[tokio::test]
    async fn interpreter_matching_wrapper_shell_is_native_body() {
        let tmp = tempfile::tempdir().unwrap();
        // Empty prefix: no interpreter executable is resolvable from it.
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        // The recipe interpreter that equals the native wrapper shell.
        let native = if cfg!(windows) { "cmd" } else { "bash" };
        let marker = "echo wrapper-shell-body";

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline(marker.to_string()),
            Some(native),
        );

        crate::execution::generate_build_script(&args)
            .await
            .expect("native wrapper shell must not be resolved from the build environment");

        let build_script = fs::read_to_string(native_build_script_path(tmp.path())).unwrap();
        assert!(
            build_script.contains(marker),
            "wrapper should inline the script body, got:\n{build_script}"
        );
        // No separate interpreter script file is written for the native shell.
        assert!(!tmp.path().join("conda_build_script.bat").exists());
        assert!(!tmp.path().join("conda_build_script.sh").exists());
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

    /// A build-prefix-only interpreter (`brush`) errors when absent instead of
    /// falling back to a system copy (which `python` would).
    #[tokio::test]
    async fn build_prefix_only_interpreter_missing_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("echo missing".to_string()),
            Some("brush"),
        );

        let err = crate::execution::generate_build_script(&args)
            .await
            .unwrap_err();
        assert!(
            matches!(err, InterpreterError::InterpreterNotFound(ref name) if name == "brush"),
            "expected missing brush error, got {err:?}"
        );
    }

    /// Stub interpreter that exercises the candidate iteration and
    /// `is_usable_executable` rejection path of the default
    /// `resolve_executable`. It searches the build environment only and tries
    /// two executable names; the first name is always rejected by validation.
    struct RejectFirstStub;

    impl InterpreterInvocation for RejectFirstStub {
        fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
            &["stub_first", "stub_second"]
        }

        fn search_scope(&self, _build_platform: &Platform) -> InterpreterSearchScope {
            InterpreterSearchScope::build_only()
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
            .resolve_executable(&shared_context(RuntimeEnv::current(), &prefix))
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
            .resolve_executable(&shared_context(RuntimeEnv::current(), &prefix))
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
            .resolve_executable(&shared_context(RuntimeEnv::current(), &prefix))
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

    /// A `build_only` interpreter must not leak from the system PATH, while one
    /// with a system fallback finds an exe present only on PATH. The `PATH` is
    /// injected through `RuntimeEnv`, so the test does not touch the real process
    /// environment.
    #[test]
    fn system_fallback_scope_finds_path_only_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        // Place a fake exe in a separate dir that we expose only via the runtime
        // PATH, NOT in any prefix.
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

        let runtime = RuntimeEnv::for_test(Platform::current())
            .with_var("PATH", path_dir.to_string_lossy().into_owned());

        let context = shared_context(runtime, &prefix);
        let found_via_path = find_interpreter(
            "rb_path_only_tool",
            &context,
            InterpreterSearchScope::build_and_host_with_system_fallback(),
        );
        let found_build_only = find_interpreter(
            "rb_path_only_tool",
            &context,
            InterpreterSearchScope::build_only(),
        );

        assert!(
            found_via_path.is_some(),
            "system-fallback scope should find the exe on PATH"
        );
        assert_eq!(found_via_path.unwrap(), exe);
        assert!(
            found_build_only.is_none(),
            "build_only must not find an exe that lives only on PATH"
        );
    }

    /// The generated wrapper quotes a resolved interpreter path that contains
    /// spaces (the cmd.exe-on-Windows failure), so it survives the native shell.
    #[tokio::test]
    async fn generated_wrapper_quotes_spaced_interpreter_path() {
        let tmp = tempfile::tempdir().unwrap();
        // A prefix path containing a space, like `C:\Program Files\...`.
        let prefix = tmp.path().join("pre fix");
        fs::create_dir_all(&prefix).unwrap();
        let python = create_fake_executable(&prefix, "python");
        assert!(python.to_string_lossy().contains(' '));

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("print('hi')".to_string()),
            Some("python"),
        );
        crate::execution::generate_build_script(&args)
            .await
            .unwrap();

        let wrapper = fs::read_to_string(native_build_script_path(tmp.path())).unwrap();
        let path = python.to_string_lossy();
        let quoted = if cfg!(windows) {
            format!("\"{path}\"")
        } else {
            format!("'{path}'")
        };
        assert!(
            wrapper.contains(&quoted),
            "wrapper must quote the spaced interpreter path `{quoted}`, got:\n{wrapper}"
        );
    }

    /// An interpreter provided as a `host` dependency lives in the run prefix.
    /// A system-fallback scope searches it, while `build_only` does not.
    #[test]
    fn host_prefix_searched_only_with_system_fallback_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let build_prefix = tmp.path().join("build");
        let host_prefix = tmp.path().join("host");
        fs::create_dir_all(&build_prefix).unwrap();
        fs::create_dir_all(&host_prefix).unwrap();
        // The tool exists only in the host prefix.
        let tool = create_fake_executable(&host_prefix, "rb_host_tool");
        // Empty runtime PATH so resolution can only come from a prefix.
        let runtime = RuntimeEnv::for_test(Platform::current()).with_var("PATH", "");

        let context = ExecutionContext::separate(
            runtime,
            &build_prefix,
            Platform::current(),
            &host_prefix,
            Platform::current(),
        );
        let found = find_interpreter(
            "rb_host_tool",
            &context,
            InterpreterSearchScope::build_and_host_with_system_fallback(),
        );
        assert_eq!(found.as_deref(), Some(tool.as_path()));

        let build_only = find_interpreter(
            "rb_host_tool",
            &context,
            InterpreterSearchScope::build_only(),
        );
        assert!(
            build_only.is_none(),
            "build_only must not search the host prefix"
        );
    }

    /// PowerShell tries `pwsh` first then `powershell` on the windows branch.
    /// This documents the candidate order without resolving (the real invocation
    /// has a system fallback, so a system `pwsh` would leak into resolution).
    #[test]
    fn powershell_lists_pwsh_then_powershell_on_windows() {
        let names = super::powershell::PowerShellInvocation.executable_names(&Platform::Win64);
        assert_eq!(names, &["pwsh", "powershell"]);
    }

    /// When the first executable name is absent from the prefix, resolution falls
    /// through to a later name. Uses the `build_only` stub so the system PATH
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
            .resolve_executable(&shared_context(RuntimeEnv::current(), &prefix))
            .expect("second candidate should resolve when the first is absent");
        assert_eq!(resolved, second);
    }

    /// On Windows, cmd resolution special-cases `COMSPEC` when it points at
    /// `cmd.exe`, returning it verbatim. `COMSPEC` and the platform are injected
    /// through `RuntimeEnv`, so this runs on every host without touching the real
    /// environment.
    #[test]
    fn cmd_uses_comspec_special_case() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_cmd = tmp.path().join("system32").join("cmd.exe");
        fs::create_dir_all(fake_cmd.parent().unwrap()).unwrap();
        fs::write(&fake_cmd, "").unwrap();

        let runtime = RuntimeEnv::for_test(Platform::Win64)
            .with_var("COMSPEC", fake_cmd.to_string_lossy().into_owned());

        let resolved = super::cmd_exe::CmdExeInvocation
            .resolve_executable(&shared_context(runtime, tmp.path()));
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

    /// The interpreter table maps each recipe name once; a duplicate entry
    /// would shadow the later constructor.
    #[test]
    fn interpreter_table_has_no_duplicate_names() {
        let mut names: Vec<&str> = INTERPRETERS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate name in INTERPRETERS");
    }

    /// Typos within a small edit distance suggest the intended interpreter;
    /// unrelated names produce no suggestion.
    #[test]
    fn closest_interpreter_suggests_typos_only() {
        assert_eq!(closest_interpreter("brus"), Some("brush"));
        assert_eq!(closest_interpreter("pyton"), Some("python"));
        assert_eq!(closest_interpreter("powershel"), Some("powershell"));
        assert_eq!(closest_interpreter("pwsh"), Some("powershell"));
        assert_eq!(closest_interpreter("zsh"), None);
        assert_eq!(closest_interpreter("not-a-real-interp"), None);
    }

    /// The similarity comparison is case-insensitive.
    #[test]
    fn closest_interpreter_is_case_insensitive() {
        assert_eq!(closest_interpreter("Bash"), Some("bash"));
        assert_eq!(closest_interpreter("PYTHON"), Some("python"));
        assert_eq!(closest_interpreter("PowerShell"), Some("powershell"));
    }

    /// `brush` runs scripts under `set -euxo pipefail` semantics by default,
    /// passed as invocation flags ahead of the script path so both inline and
    /// file-backed scripts get them. `bash` keeps its plain invocation (its
    /// strict modes come from the wrapper preamble instead).
    #[test]
    fn brush_invocation_defaults_to_strict_mode_flags() {
        let script = Path::new("conda_build_script.sh");

        assert_eq!(
            super::brush::BrushInvocation.args(script),
            ["-euxo", "pipefail", "conda_build_script.sh"]
        );
        assert_eq!(
            super::bash::BashInvocation.args(script),
            ["conda_build_script.sh"]
        );
    }

    /// `cmd` propagates a non-zero exit between commands; others join plainly.
    #[test]
    fn join_commands_is_interpreter_specific() {
        let commands = vec!["echo Hello".to_string(), "echo World".to_string()];

        assert_eq!(
            super::cmd_exe::CmdExeInvocation.join_commands(&commands),
            "echo Hello\nif %errorlevel% neq 0 exit /b %errorlevel%\n\
             echo World\nif %errorlevel% neq 0 exit /b %errorlevel%"
        );
        // bash relies on `set -e` in the preamble, so a plain join is enough.
        assert_eq!(
            super::bash::BashInvocation.join_commands(&commands),
            "echo Hello\necho World"
        );
        // A language interpreter uses the default plain join too.
        assert_eq!(
            super::python::PythonInvocation.join_commands(&commands),
            "echo Hello\necho World"
        );
    }
}
