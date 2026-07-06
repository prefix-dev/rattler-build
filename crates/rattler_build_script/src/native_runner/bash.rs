use std::path::Path;

use rattler_shell::shell;

use super::NativeShellRunner;

pub(crate) struct BashNativeRunner;

impl NativeShellRunner for BashNativeRunner {
    fn shell(&self) -> shell::ShellEnum {
        shell::Bash::default().into()
    }

    fn default_interpreter(&self) -> &'static str {
        "bash"
    }

    fn preamble(&self, activation_script_path: &std::path::Path) -> String {
        format!(
            r#"#!/usr/bin/env bash
set -e
## Start of bash preamble
if [ -z ${{CONDA_BUILD+x}} ]; then
    source "{}"
fi
## End of preamble
# Trace each command as it runs so a failing line is visible (see #2264).
# Placed after activation so the sourced environment setup is not traced.
set -x
"#,
            activation_script_path.to_string_lossy()
        )
    }

    fn command_to_run_script<'a>(&self, build_script_path: &'a str) -> Vec<&'a str> {
        vec!["bash", build_script_path]
    }

    fn replacements_template(&self) -> &'static str {
        "$((var))"
    }

    /// Returns reproduction instructions for the failed bash wrapper script.
    fn debug_info(
        &self,
        work_dir: &Path,
        run_prefix: &Path,
        build_prefix: Option<&Path>,
    ) -> String {
        let mut output = String::new();

        output.push_str("\nScript execution failed.\n\n");
        output.push_str(&format!("  Work directory: {}\n", work_dir.display()));
        output.push_str(&format!("  Prefix: {}\n", run_prefix.display()));

        if let Some(build_prefix) = build_prefix {
            output.push_str(&format!("  Build prefix: {}\n", build_prefix.display()));
        } else {
            output.push_str("  Build prefix: None\n");
        }

        output.push_str("\nTo run the script manually, use the following command:\n\n");
        output.push_str(&format!("  cd {:?} && ./conda_build.sh\n\n", work_dir));
        output.push_str("To run commands interactively in the build environment:\n\n");
        output.push_str(&format!("  cd {:?} && source build_env.sh", work_dir));

        output
    }
}
