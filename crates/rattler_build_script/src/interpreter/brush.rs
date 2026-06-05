use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::execution::{ExecutionArgs, ResolvedScriptContents};

use super::{
    BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, InterpreterSearchScope,
    find_interpreter,
};

pub struct BrushInterpreter;

// Brush runs bash-compatible scripts. The native bash/cmd wrapper performs
// activation and then invokes brush on the user script as a child process.
impl Interpreter for BrushInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        // brush must be provided by the build environment, not the system PATH.
        let brush_path = find_interpreter(
            "brush",
            args.build_prefix.as_ref(),
            &args.execution_platform,
            InterpreterSearchScope::BuildPrefixOnly,
        )
        .ok()
        .flatten()
        .ok_or_else(|| InterpreterError::InterpreterNotFound("brush".to_string()))?;

        let brush_script = args.work_dir.join("conda_build_script.sh");
        tokio::fs::write(&brush_script, args.script.script()).await?;

        // Invoke the resolved brush from the wrapper so the build-env binary runs.
        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!("{:?} {:?}", brush_path, brush_script)),
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
        find_interpreter(
            "brush",
            build_prefix,
            platform,
            InterpreterSearchScope::BuildPrefixOnly,
        )
    }
}
