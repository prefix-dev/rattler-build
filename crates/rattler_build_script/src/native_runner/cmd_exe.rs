use std::fmt::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use rattler_shell::shell::{self, Shell};

use super::NativeShellRunner;

pub(crate) struct CmdExeNativeRunner;

impl NativeShellRunner for CmdExeNativeRunner {
    fn shell(&self) -> shell::ShellEnum {
        shell::CmdExe.into()
    }

    fn default_interpreter(&self) -> &'static str {
        "cmd"
    }

    fn preamble(&self, activation_script_path: &std::path::Path) -> String {
        format!(
            r#"
@chcp 65001 > nul
@echo on
IF "%CONDA_BUILD%" == "" (
    @rem special behavior from conda-build for Windows
    call "{}"
)
@rem re-enable echo because the activation scripts might have messed with it
@echo on
"#,
            activation_script_path.to_string_lossy()
        )
    }

    fn command_to_run_script<'a>(&self, build_script_path: &'a str) -> Vec<&'a str> {
        vec!["cmd.exe", "/d", "/c", build_script_path]
    }

    fn replacements_template(&self) -> &'static str {
        "%((var))%"
    }

    fn supports_sandbox(&self) -> bool {
        false
    }

    fn native_section_script_command(&self, script_path: &Path) -> Option<Vec<String>> {
        Some(vec![
            "call".to_string(),
            script_path.to_string_lossy().into_owned(),
        ])
    }

    /// `setlocal`/`endlocal` scope environment changes, while `pushd`/`popd`
    /// restore the working directory after successful sections. The saved
    /// errorlevel keeps `popd`/`endlocal` from masking a failing body.
    fn scope_section(
        &self,
        label: Option<&str>,
        env: &IndexMap<String, String>,
        cwd: Option<&Path>,
        body: &str,
    ) -> Result<String, std::io::Error> {
        let shell = shell::CmdExe;
        let mut out = String::new();
        if let Some(label) = label {
            let _ = writeln!(out, "@rem === {label} ===");
        }
        out.push_str("setlocal\n");
        for (key, value) in env {
            shell
                .set_env_var(&mut out, key, value)
                .map_err(std::io::Error::other)?;
        }
        let cwd = cwd
            .map(|cwd| super::quote_arg(&self.shell(), &cwd.to_string_lossy()))
            .unwrap_or_else(|| ".".to_string());
        let _ = writeln!(out, "pushd {cwd}");
        out.push_str("if %errorlevel% neq 0 exit /b %errorlevel%\n");
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("set \"RB_SECTION_ERRORLEVEL=%errorlevel%\"\n");
        out.push_str("popd\n");
        out.push_str(
            "if %RB_SECTION_ERRORLEVEL% equ 0 if %errorlevel% neq 0 set \"RB_SECTION_ERRORLEVEL=%errorlevel%\"\n",
        );
        out.push_str("endlocal & if %RB_SECTION_ERRORLEVEL% neq 0 exit /b %RB_SECTION_ERRORLEVEL%");
        Ok(out)
    }

    /// Returns reproduction instructions for the failed cmd wrapper script.
    fn debug_info(
        &self,
        work_dir: &Path,
        run_prefix: &Path,
        build_prefix: Option<&Path>,
    ) -> String {
        let mut output = String::new();

        output.push_str("\nScript execution failed.\n\n");
        output.push_str(&format!("  Work directory: {}\n", work_dir.display()));
        output.push_str(&format!("  Prefix: {}\n", run_prefix.display()));

        if let Some(build_prefix) = build_prefix {
            output.push_str(&format!("  Build prefix: {}\n", build_prefix.display()));
        } else {
            output.push_str("  Build prefix: None\n");
        }

        output.push_str("\nTo run the script manually, use the following command:\n");
        output.push_str(&format!("  cd {:?} && ./conda_build.bat\n\n", work_dir));
        output.push_str("To run commands interactively in the build environment:\n");
        output.push_str(&format!("  cd {:?} && call build_env.bat", work_dir));

        output
    }
}
