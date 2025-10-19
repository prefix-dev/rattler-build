use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::script::{ExecutionArgs, ResolvedScriptContents};

use super::{BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct NodeJsInterpreter;

// NodeJS interpreter calls either bash or cmd.exe interpreter for activation and then runs Node script
impl Interpreter for NodeJsInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let node_script = args.work_dir.join("conda_build_script.js");
        tokio::fs::write(&node_script, args.script.script()).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!("node {:?}", node_script)),
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
        find_interpreter("node", build_prefix, platform)
    }
}
