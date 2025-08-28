use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::script::{ExecutionArgs, ResolvedScriptContents};

use super::{BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct RInterpreter;

// R interpreter calls either bash or cmd.exe interpreter for activation and then runs R script
impl Interpreter for RInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let script = args.script.script();
        let r_script = args.work_dir.join("conda_build_script.R");
        tokio::fs::write(&r_script, script).await?;
        let r_command = format!("Rscript {:?}", r_script);

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(r_command),
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
        find_interpreter("Rscript", build_prefix, platform)
    }
}
