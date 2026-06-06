use std::path::Path;
use std::process::Command;

use rattler_conda_types::Platform;

use super::{InterpreterError, InterpreterInvocation, InterpreterSearchScope};

pub struct PowerShellInvocation;

const POWERSHELL_PREAMBLE: &str = r#"
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

foreach ($envVar in Get-ChildItem Env:) {
    if (-not (Test-Path -Path Variable:$($envVar.Name))) {
        Set-Variable -Name $envVar.Name -Value $envVar.Value
    }
}

"#;

/// Returns whether the given pwsh binary is PowerShell 7.4+.
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

impl InterpreterInvocation for PowerShellInvocation {
    fn executable_names(&self, build_platform: &Platform) -> &'static [&'static str] {
        if build_platform.is_windows() {
            &["pwsh", "powershell"]
        } else {
            &["pwsh"]
        }
    }

    fn search_scope(&self, build_platform: &Platform) -> InterpreterSearchScope {
        if build_platform.is_windows() {
            InterpreterSearchScope::PrefixThenSystemPath
        } else {
            InterpreterSearchScope::BuildPrefixOnly
        }
    }

    fn extension(&self) -> &'static str {
        "ps1"
    }

    fn script_contents(&self, raw: &str) -> String {
        POWERSHELL_PREAMBLE.to_owned() + raw
    }

    fn is_usable_executable(&self, executable: &Path) -> Result<(), InterpreterError> {
        if !is_pwsh_new_enough(executable) {
            tracing::warn!(
                "rattler-build requires PowerShell 7.4+, \
                 otherwise it will skip native command errors!"
            );
        }
        Ok(())
    }

    fn args(&self, script_path: &Path) -> Vec<String> {
        vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            script_path.to_string_lossy().into_owned(),
        ]
    }
}
