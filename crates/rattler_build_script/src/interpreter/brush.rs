use rattler_conda_types::Platform;

use super::InterpreterInvocation;

pub struct BrushInvocation;

// Uses the default `build_only` scope: a system `brush` is never used, for
// reproducibility.
impl InterpreterInvocation for BrushInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["brush"]
    }

    fn extension(&self) -> &'static str {
        "sh"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
