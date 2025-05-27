use std::path::PathBuf;

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{ExecutionArgs, run_process_with_replacements};

use super::{BASH_PREAMBLE, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct BashInterpreter;

fn print_debug_info(args: &ExecutionArgs) -> String {
    let mut output = String::new();

    if args.debug.is_enabled() {
        output.push_str("\nDebug mode enabled - not executing the script.\n\n");
    } else {
        output.push_str("\nScript execution failed.\n\n");
    }

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

impl Interpreter for BashInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let script = self.get_script(&args, shell::Bash).unwrap();

        let build_env_path = args.work_dir.join("build_env.sh");
        let build_script_path = args.work_dir.join("conda_build.sh");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble = BASH_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
        let script = format!("{}\n{}", preamble, args.script.script());
        tokio::fs::write(&build_script_path, script).await?;

        // Mark build_env.sh and conda_build.sh as executable
        #[cfg(unix)]
        {
            use std::{fs::Permissions, os::unix::fs::PermissionsExt};
            let permissions = Permissions::from_mode(0o755);
            tokio::fs::set_permissions(&build_script_path, permissions).await?;
        }

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let mut cmd_args = vec!["bash", "-e"];
        if args.debug.is_enabled() {
            cmd_args.push("-x");
        }
        cmd_args.push(&build_script_path_str);

        if args.debug.is_enabled() {
            return Err(InterpreterError::Debug(print_debug_info(&args)));
        }

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("$((var))"),
            args.sandbox_config.as_ref(),
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
        find_interpreter("bash", build_prefix, platform)
    }
}
