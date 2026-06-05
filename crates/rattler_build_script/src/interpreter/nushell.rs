use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::execution::{ExecutionArgs, ResolvedScriptContents};

use super::{
    BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, InterpreterSearchScope,
    find_interpreter,
};

pub struct NuShellInterpreter;

// NuShell scripts are executed by invoking `nu` on the user script. The native
// bash/cmd wrapper performs activation and then invokes nu on the user script
// as a child process.
impl Interpreter for NuShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        // nu must be provided by the build environment, not the system PATH.
        let nu_path = find_interpreter(
            "nu",
            args.build_prefix.as_ref(),
            &args.execution_platform,
            InterpreterSearchScope::BuildPrefixOnly,
        )
        .ok()
        .flatten()
        .ok_or_else(|| InterpreterError::InterpreterNotFound("nu".to_string()))?;

        let nu_script = args.work_dir.join("conda_build_script.nu");
        tokio::fs::write(&nu_script, args.script.script()).await?;

        // Invoke the resolved nu from the wrapper so the build-env binary runs.
        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!("{:?} {:?}", nu_path, nu_script)),
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
            "nu",
            build_prefix,
            platform,
            InterpreterSearchScope::BuildPrefixOnly,
        )
    }
}
