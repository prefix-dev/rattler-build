use std::path::PathBuf;

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{ExecutionArgs, run_process_with_replacements};

use super::{CMDEXE_PREAMBLE, Interpreter, InterpreterError, find_interpreter};

fn print_debug_info(args: &ExecutionArgs) -> String {
    let mut output = String::new();
    if args.debug.is_enabled() {
        output.push_str("\nDebug mode enabled - not executing the script.\n\n");
    } else {
        output.push_str("\nScript execution failed.\n\n")
    }

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

pub(crate) struct CmdExeInterpreter;

impl Interpreter for CmdExeInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let script = self.get_script(&args, shell::CmdExe).unwrap();

        let build_env_path = args.work_dir.join("build_env.bat");
        let build_script_path = args.work_dir.join("conda_build.bat");

        tokio::fs::write(&build_env_path, script).await?;

        let build_script = format!(
            "{}\n{}",
            CMDEXE_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy()),
            args.script.script()
        );
        tokio::fs::write(
            &build_script_path,
            &build_script.replace('\n', "\r\n").as_bytes(),
        )
        .await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let cmd_args = ["cmd.exe", "/d", "/c", &build_script_path_str];

        if args.debug.is_enabled() {
            return Err(InterpreterError::Debug(print_debug_info(&args)));
        }

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("%((var))%"),
            None,
        )
        .await?;

        if !output.status.success() {
            let status_code = output.status.code().unwrap_or(1);
            tracing::error!("Script failed with status {}", status_code);
            tracing::error!("{}", print_debug_info(&args));
            return Err(InterpreterError::ExecutionFailed(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Script failed".to_string(),
            )));
        }

        Ok(())
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        // check if COMSPEC is set to cmd.exe
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if comspec.to_lowercase().contains("cmd.exe") {
                return Ok(Some(PathBuf::from(comspec)));
            }
        }

        // check if cmd.exe is in PATH
        find_interpreter("cmd", build_prefix, platform)
    }
}
