use std::path::PathBuf;

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{interpreter::DEBUG_HELP, run_process_with_replacements, ExecutionArgs};

use super::{find_interpreter, Interpreter};

const BASH_PREAMBLE: &str = r#"#!/bin/bash
## Start of bash preamble
if [ -z ${CONDA_BUILD+x} ]; then
    source ((script_path))
fi
## End of preamble
"#;

pub(crate) struct BashInterpreter;

impl Interpreter for BashInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let script = self.get_script(&args, shell::Bash).unwrap();

        let build_env_path = args.work_dir.join("build_env.sh");
        let build_script_path = args.work_dir.join("conda_build.sh");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble = BASH_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
        let script = format!("{}\n{}", preamble, args.script.script());
        tokio::fs::write(&build_script_path, script).await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let mut cmd_args = vec!["bash", "-e"];
        if args.debug {
            cmd_args.push("-x");
        }
        cmd_args.push(&build_script_path_str);

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
            tracing::error!("Work directory: '{}'", args.work_dir.display());
            tracing::error!("{}", DEBUG_HELP);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Script failed".to_string(),
            ));
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
