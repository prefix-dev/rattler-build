use std::path::{Path, PathBuf};
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

/// Check if the given pwsh binary is PowerShell 7.4+.
fn is_pwsh_new_enough(pwsh_path: &Path) -> bool {
    let result: Option<bool> = (|| {
        let out =
            String::from_utf8(Command::new(pwsh_path).arg("-v").output().ok()?.stdout).ok()?;
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
    })();

    result.unwrap_or(false)
}

// PowerShell interpreter: writes a .ps1 script then delegates to cmd.exe (Windows) or bash (Unix)
// to run it via the pwsh/powershell command.
impl Interpreter for PowerShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        // Try to find pwsh in the build prefix or system PATH for version checking.
        // The actual command used in the script may rely on the activation script
        // (build_env.sh) to put pwsh on PATH at runtime.
        let pwsh_path =
            find_interpreter("pwsh", args.build_prefix.as_ref(), &args.execution_platform)
                .ok()
                .flatten();

        let (shell_cmd, new_enough) = match &pwsh_path {
            Some(path) => {
                let new_enough = is_pwsh_new_enough(path);
                (path.to_string_lossy().into_owned(), new_enough)
            }
            // Fall back to "pwsh" by name — the conda `powershell` package provides the
            // `pwsh` binary, and the activation scripts will put it on PATH.
            None => ("pwsh".to_owned(), false),
        };

        if !new_enough {
            tracing::warn!(
                "rattler-build requires PowerShell 7.4+, \
                 otherwise it will skip native command errors!"
            );
        }

        let ps1_script = args.work_dir.join("conda_build_script.ps1");
        let contents = POWERSHELL_PREAMBLE.to_owned() + args.script.script();
        tokio::fs::write(&ps1_script, contents).await?;

        // Quote the shell command if it contains spaces (e.g. "C:\Program Files\...")
        let quoted_shell_cmd = if shell_cmd.contains(' ') {
            format!("\"{}\"", shell_cmd)
        } else {
            shell_cmd
        };

        let args = ExecutionArgs {
            script: crate::execution::ResolvedScriptContents::Inline(format!(
                "{} -NoLogo -NoProfile {:?}",
                quoted_shell_cmd, ps1_script
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
