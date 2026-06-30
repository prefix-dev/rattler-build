use std::path::{Path, PathBuf};

use rattler_conda_types::Platform;

use super::{InterpreterInvocation, InterpreterSearchScope};
use crate::runtime::RuntimeEnv;

pub struct CmdExeInvocation;

impl InterpreterInvocation for CmdExeInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["cmd"]
    }

    fn search_scope(&self, build_platform: &Platform) -> InterpreterSearchScope {
        if build_platform.is_windows() {
            InterpreterSearchScope::build_and_host_with_system_fallback()
        } else {
            InterpreterSearchScope::build_only()
        }
    }

    fn extension(&self) -> &'static str {
        "bat"
    }

    fn join_commands(&self, commands: &[String]) -> String {
        // `cmd` has no `set -e`, so propagate a non-zero exit between commands.
        commands
            .iter()
            .map(|c| format!("{c}\nif %errorlevel% neq 0 exit /b %errorlevel%"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn resolve_executable(
        &self,
        build_prefix: Option<&Path>,
        run_prefix: &Path,
        runtime: &RuntimeEnv,
    ) -> Result<PathBuf, super::InterpreterError> {
        let platform = runtime.platform();
        let scope = self.search_scope(&platform);
        if platform.is_windows()
            && scope.allows_system_fallback()
            && let Some(comspec) = runtime.var("COMSPEC")
            && comspec.to_lowercase().contains("cmd.exe")
        {
            return Ok(PathBuf::from(comspec));
        }

        super::find_interpreter("cmd", build_prefix, run_prefix, runtime, scope)
            .ok_or_else(|| super::InterpreterError::InterpreterNotFound("cmd".to_string()))
    }

    fn args(&self, script_path: &Path) -> Vec<String> {
        vec![
            "/d".to_string(),
            "/c".to_string(),
            script_path.to_string_lossy().into_owned(),
        ]
    }
}
