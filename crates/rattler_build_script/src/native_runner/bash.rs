use rattler_shell::shell;

use crate::execution::ExecutionArgs;

use super::NativeShellRunner;

pub(crate) struct BashNativeRunner;

impl NativeShellRunner for BashNativeRunner {
    fn shell(&self) -> shell::ShellEnum {
        shell::Bash::default().into()
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

    fn debug_info(&self, args: &ExecutionArgs) -> String {
        let mut output = String::new();

        output.push_str("\nScript execution failed.\n\n");
        output.push_str(&format!("  Work directory: {}\n", args.work_dir.display()));
        output.push_str(&format!("  Prefix: {}\n", args.run_prefix.display()));

        if let Some(build_prefix) = &args.build_prefix {
            output.push_str(&format!("  Build prefix: {}\n", build_prefix.display()));
        } else {
            output.push_str("  Build prefix: None\n");
        }

        output.push_str("\nTo run the script manually, use the following command:\n\n");
        output.push_str(&format!("  cd {:?} && ./conda_build.sh\n\n", args.work_dir));
        output.push_str("To run commands interactively in the build environment:\n\n");
        output.push_str(&format!("  cd {:?} && source build_env.sh", args.work_dir));

        output
    }
}
