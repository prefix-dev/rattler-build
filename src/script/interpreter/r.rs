use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::script::{ExecutionArgs, ResolvedScriptContents};

use super::{find_interpreter, BashInterpreter, CmdExeInterpreter, Interpreter};

pub(crate) struct RInterpreter;

// R interpreter calls either bash or cmd.exe interpreter for activation and then runs R script
impl Interpreter for RInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let r_script = args.work_dir.join("conda_build_script.r");
        tokio::fs::write(&r_script, args.script.script()).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!("r {:?} --vanilla", r_script)),
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
        find_interpreter("r", build_prefix, platform)
    }
}
