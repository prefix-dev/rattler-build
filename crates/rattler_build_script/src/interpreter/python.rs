use rattler_conda_types::Platform;

use super::InterpreterInvocation;

pub struct PythonInvocation;

impl InterpreterInvocation for PythonInvocation {
    fn executable_names(&self, _build_platform: &Platform) -> &'static [&'static str] {
        &["python"]
    }

    fn extension(&self) -> &'static str {
        "py"
    }

    fn args(&self, script_path: &std::path::Path) -> Vec<String> {
        vec![script_path.to_string_lossy().into_owned()]
    }
}
