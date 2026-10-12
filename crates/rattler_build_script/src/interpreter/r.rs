use rattler_conda_types::Subdir;

use super::{InterpreterInvocation, InterpreterSearchScope};

pub struct RInvocation;

impl InterpreterInvocation for RInvocation {
    fn executable_names(&self, _build_platform: &Subdir) -> &'static [&'static str] {
        &["Rscript"]
    }

    fn search_scope(&self, _build_platform: &Subdir) -> InterpreterSearchScope {
        InterpreterSearchScope::build_and_host_with_system_fallback()
    }

    fn extension(&self) -> &'static str {
        "R"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
