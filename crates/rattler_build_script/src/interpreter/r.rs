use rattler_conda_types::Platform;

use super::InterpreterInvocation;

pub struct RInvocation;

impl InterpreterInvocation for RInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["Rscript"]
    }

    fn extension(&self) -> &'static str {
        "R"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
