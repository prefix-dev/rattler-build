use std::path::{Path, PathBuf};

use rattler_conda_types::Platform;

use super::{InterpreterInvocation, InterpreterSearchScope};

pub struct CmdExeInvocation;

impl InterpreterInvocation for CmdExeInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["cmd"]
    }

    fn search_scope(&self, build_platform: &Platform) -> InterpreterSearchScope {
        if build_platform.is_windows() {
            InterpreterSearchScope::PrefixThenSystemPath
        } else {
            InterpreterSearchScope::BuildPrefixOnly
        }
    }

    fn extension(&self) -> &'static str {
        "bat"
    }

    fn resolve_executable(
        &self,
        build_prefix: Option<&PathBuf>,
        build_platform: &Platform,
    ) -> Result<PathBuf, super::InterpreterError> {
        if build_platform.is_windows()
            && let InterpreterSearchScope::PrefixThenSystemPath = self.search_scope(build_platform)
            && let Ok(comspec) = std::env::var("COMSPEC")
            && comspec.to_lowercase().contains("cmd.exe")
        {
            return Ok(PathBuf::from(comspec));
        }

        super::find_interpreter(
            "cmd",
            build_prefix,
            build_platform,
            self.search_scope(build_platform),
        )
        .ok()
        .flatten()
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
