use std::path::PathBuf;
use std::process::Command;

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
        let mut shell_cmd = "pwsh";
        let mut new_enough = true;
        let result: Option<()> = which::which("pwsh").ok().and_then(|_| {
            let out =
                String::from_utf8(Command::new("pwsh").arg("-v").output().ok()?.stdout).ok()?;
            let ver = out
                .trim()
                .split(' ')
                .last()?
                .split('.')
                .collect::<Vec<&str>>();
            if ver.len() < 2 {
                return None;
            }

            let major = ver[0].parse::<i32>().ok()?;
            let minor = ver[1].parse::<i32>().ok()?;
            if major < 7 || (major == 7 && minor < 4) {
                new_enough = false;
            }

            return Some(());
        });
        if result.is_none() {
            shell_cmd = "powershell";
            new_enough = false;
        }
        if !new_enough {
            eprintln!(
                "Warning: rattler-build requires PowerShell 7.4+, otherwise it will skip native command errors!"
            );
        }

        let ps1_script = args.work_dir.join("conda_build_script.ps1");
        let contents = POWERSHELL_PREAMBLE.to_owned() + args.script.script();
        tokio::fs::write(&ps1_script, contents).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!(
                "{} -NoLogo -NoProfile {:?}",
                shell_cmd, ps1_script
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
