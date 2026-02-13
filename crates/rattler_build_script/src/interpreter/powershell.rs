use std::path::PathBuf;
use std::process::Command;

use rattler_conda_types::Platform;

use crate::execution::ExecutionArgs;

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

/// Check if pwsh (PowerShell 7+) is available and determine its version.
/// Returns (shell_command, is_new_enough).
fn detect_powershell() -> (&'static str, bool) {
    let result: Option<bool> = which::which("pwsh").ok().and_then(|_| {
        let out = String::from_utf8(Command::new("pwsh").arg("-v").output().ok()?.stdout).ok()?;
        let ver = out
            .trim()
            .split(' ')
            .next_back()?
            .split('.')
            .collect::<Vec<&str>>();
        if ver.len() < 2 {
            return None;
        }

        let major = ver[0].parse::<i32>().ok()?;
        let minor = ver[1].parse::<i32>().ok()?;
        Some(major > 7 || (major == 7 && minor >= 4))
    });

    match result {
        Some(new_enough) => ("pwsh", new_enough),
        None => ("powershell", false),
    }
}

// PowerShell interpreter: writes a .ps1 script then delegates to cmd.exe (Windows) or bash (Unix)
// to run it via the pwsh/powershell command.
impl Interpreter for PowerShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let (shell_cmd, new_enough) = detect_powershell();

        if !new_enough {
            eprintln!(
                "Warning: rattler-build requires PowerShell 7.4+, \
                 otherwise it will skip native command errors!"
            );
        }

        let ps1_script = args.work_dir.join("conda_build_script.ps1");
        let contents = POWERSHELL_PREAMBLE.to_owned() + args.script.script();
        tokio::fs::write(&ps1_script, contents).await?;

        let args = ExecutionArgs {
            script: crate::execution::ResolvedScriptContents::Inline(format!(
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
