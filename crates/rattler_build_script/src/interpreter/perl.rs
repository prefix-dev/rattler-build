use rattler_conda_types::Platform;

use super::{InterpreterInvocation, InterpreterSearchScope};

pub struct PerlInvocation;

impl InterpreterInvocation for PerlInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["perl"]
    }

    fn search_scope(&self, _build_platform: &Platform) -> InterpreterSearchScope {
        InterpreterSearchScope::build_and_host_with_system_fallback()
    }

    fn extension(&self) -> &'static str {
        "pl"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
