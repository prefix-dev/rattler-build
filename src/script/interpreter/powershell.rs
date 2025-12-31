use std::path::PathBuf;

use rattler_conda_types::Platform;

use crate::script::{ExecutionArgs, ResolvedScriptContents};

use super::{BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct PowerShellInterpreter;

const POWERSHELL_PREAMBLE: &str = r#"
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

foreach ($envVar in Get-ChildItem Env:) {
    if (-not (Test-Path -Path Variable:$($envVar.Name))) {
        Set-Variable -Name $envVar.Name -Value $envVar.Value
    }
}

"#;

// PowerShell interpreter calls cmd.exe interpreter for activation and then runs PowerShell script
impl Interpreter for PowerShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let ps1_script = args.work_dir.join("conda_build_script.ps1");
        let contents = POWERSHELL_PREAMBLE.to_owned() + args.script.script();
        tokio::fs::write(&ps1_script, contents).await?;

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
