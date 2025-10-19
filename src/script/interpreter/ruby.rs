use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::script::{ExecutionArgs, ResolvedScriptContents};

use super::{BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct RubyInterpreter;

// Ruby interpreter calls either bash or cmd.exe interpreter for activation and then runs Ruby script
impl Interpreter for RubyInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let ruby_script = args.work_dir.join("conda_build_script.rb");
        tokio::fs::write(&ruby_script, args.script.script()).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!("ruby {:?}", ruby_script)),
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
        find_interpreter("ruby", build_prefix, platform)
    }
}
