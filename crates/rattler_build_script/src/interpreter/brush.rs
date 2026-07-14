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
        // Match the strictness the bash wrapper gets from its preamble: fail on
        // errors (`-e`), unset variables (`-u`) and pipeline failures
        // (`-o pipefail`), and trace commands (`-x`). Passing these at
        // invocation makes them the default for inline and file-backed scripts
        // alike, while a script can still opt out with `set +e` etc.
        vec![
            "-e".to_string(),
            "-u".to_string(),
            "-x".to_string(),
            "-o".to_string(),
            "pipefail".to_string(),
            script_path.to_string_lossy().into_owned(),
        ]
    }
}
