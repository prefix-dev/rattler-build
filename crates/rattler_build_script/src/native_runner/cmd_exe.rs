use rattler_shell::shell;

use crate::execution::ExecutionArgs;

use super::NativeShellRunner;

pub(crate) struct CmdExeNativeRunner;

impl NativeShellRunner for CmdExeNativeRunner {
    fn shell(&self) -> shell::ShellEnum {
        shell::CmdExe.into()
    }

    fn preamble(&self, activation_script_path: &std::path::Path) -> String {
        format!(
            r#"
@chcp 65001 > nul
@echo on
IF "%CONDA_BUILD%" == "" (
    @rem special behavior from conda-build for Windows
    call "{}"
)
@rem re-enable echo because the activation scripts might have messed with it
@echo on
"#,
            activation_script_path.to_string_lossy()
        )
    }

    fn command_to_run_script<'a>(&self, build_script_path: &'a str) -> Vec<&'a str> {
        vec!["cmd.exe", "/d", "/c", build_script_path]
    }

    fn replacements_template(&self) -> &'static str {
        "%((var))%"
    }

    fn supports_sandbox(&self) -> bool {
        false
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

        output.push_str("\nTo run the script manually, use the following command:\n");
        output.push_str(&format!(
            "  cd {:?} && ./conda_build.bat\n\n",
            args.work_dir
        ));
        output.push_str("To run commands interactively in the build environment:\n");
        output.push_str(&format!("  cd {:?} && call build_env.bat", args.work_dir));

        output
    }
}
