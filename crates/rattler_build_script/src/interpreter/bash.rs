use rattler_conda_types::Platform;

use super::{InterpreterInvocation, InterpreterSearchScope};

pub struct BashInvocation;

impl InterpreterInvocation for BashInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["bash"]
    }

    fn search_scope(&self, build_platform: &Platform) -> InterpreterSearchScope {
        if build_platform.is_unix() {
            InterpreterSearchScope::PrefixThenSystemPath
        } else {
            InterpreterSearchScope::BuildPrefixOnly
        }
    }

    fn extension(&self) -> &'static str {
        "sh"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
