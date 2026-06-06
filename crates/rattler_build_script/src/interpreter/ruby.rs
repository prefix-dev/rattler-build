use rattler_conda_types::Platform;

use super::InterpreterInvocation;

pub struct RubyInvocation;

impl InterpreterInvocation for RubyInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["ruby"]
    }

    fn extension(&self) -> &'static str {
        "rb"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
