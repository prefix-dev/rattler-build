use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::script::{ExecutionArgs, ResolvedScriptContents};

use super::{BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct PowerShellInterpreter;

// PowerShell interpreter calls cmd.exe interpreter for activation and then runs PowerShell script
impl Interpreter for PowerShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let ps1_script = args.work_dir.join("conda_build_script.ps1");
        tokio::fs::write(&ps1_script, args.script.script()).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!(
                "pwsh -NoLogo -NoProfile {:?}",
                ps1_script
            )),
            ..args
        };

        if cfg!(windows) {
            CmdExeInterpreter.run(args).await
        } else {
            BashInterpreter.run(args).await
        }
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        find_interpreter("pwsh", build_prefix, platform)
    }
}
