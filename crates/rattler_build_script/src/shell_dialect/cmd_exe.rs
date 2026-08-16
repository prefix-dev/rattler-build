use std::fmt::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use rattler_shell::shell::{self, Shell};

use super::{CommandSpec, ShellDialect};
use crate::{
    ExecutionContext, PrefixLayout,
    windows_machine::{WindowsMachine, windows_machine_transition},
};

pub(crate) struct CmdExeDialect;

impl ShellDialect for CmdExeDialect {
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

    fn command_to_run_script(
        &self,
        build_script_path: &Path,
        context: &ExecutionContext,
    ) -> CommandSpec {
        if let Some(machine) = windows_machine_transition(
            context.runtime().process_platform(),
            context.build().platform(),
        ) {
            // `start /machine` selects the architecture of the child `cmd.exe`.
            // It normally returns immediately, so `/wait` is required to obtain
            // the script's status. `cmd /c` otherwise returns the status of the
            // `start` command itself, hence the explicit delayed `ERRORLEVEL`
            // expansion and `exit /b` after the child finishes.
            //
            // The outer process runs in `work_dir`, so only the generated file
            // name is needed. Quote it when necessary so a changed or reused
            // filename containing whitespace remains a single argument.
            let script_name = build_script_path
                .file_name()
                .expect("generated build script has a filename")
                .to_string_lossy();
            let script_name = super::quote_arg(&self.shell(), &script_name);
            // `/machine x86` does not redirect an explicit `cmd.exe` lookup
            // from System32. Launch the x86 command interpreter from SysWOW64
            // directly. SystemRoot is conventionally an unspaced system path,
            // so keep it unquoted to avoid `start` treating it as a title. The
            // other architectures use `cmd.exe`, whose image selection is
            // handled by `/machine`.
            let child_cmd = match machine {
                WindowsMachine::X86 => r"%SystemRoot%\SysWOW64\cmd.exe",
                WindowsMachine::Amd64 | WindowsMachine::Arm64 => "cmd.exe",
            };
            let command = format!(
                "start /b /wait /machine {} {} /d /c {} & exit /b !ERRORLEVEL!",
                machine.start_argument(),
                child_cmd,
                script_name,
            );
            CommandSpec::new(
                "cmd.exe",
                [
                    "/d".to_string(),
                    "/v:on".to_string(),
                    "/c".to_string(),
                    command,
                ],
            )
        } else {
            CommandSpec::new(
                "cmd.exe",
                [
                    "/d".to_string(),
                    "/c".to_string(),
                    build_script_path.to_string_lossy().into_owned(),
                ],
            )
        }
    }

    fn replacements_template(&self) -> &'static str {
        "%((var))%"
    }

    fn supports_sandbox(&self) -> bool {
        false
    }

    fn native_section_script_command(&self, script_path: &Path) -> Option<Vec<String>> {
        // Activated build environments can replace PATH entirely. Resolve the
        // command processor before entering the wrapper so nested native
        // sections do not depend on `cmd.exe` remaining discoverable.
        let command_processor = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        Some(vec![
            command_processor,
            "/d".to_string(),
            "/c".to_string(),
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
            super::validate_env_assignment(key, value)?;
            shell
                .set_env_var(&mut out, key, value)
                .map_err(std::io::Error::other)?;
        }
        let cwd = cwd
            .map(|cwd| super::quote_arg(&self.shell(), &cwd.to_string_lossy()))
            .unwrap_or_else(|| ".".to_string());
        // `pushd` can misparse an unquoted path containing forward slashes as
        // command switches, even when the path has no spaces.
        let cwd = if cwd.starts_with('"') {
            cwd
        } else {
            format!("\"{cwd}\"")
        };
        // Use command chaining instead of inspecting `%errorlevel%`: a successful
        // `pushd` does not reliably clear an error left by environment activation.
        let _ = writeln!(out, "pushd {cwd} || exit /b 1");
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
    fn debug_info(&self, work_dir: &Path, context: &ExecutionContext) -> String {
        let mut output = String::new();

        output.push_str("\nScript execution failed.\n\n");
        output.push_str(&format!("  Work directory: {}\n", work_dir.display()));
        output.push_str(&format!("  Prefix: {}\n", context.host().path().display()));

        if context.layout() == PrefixLayout::Separate {
            output.push_str(&format!(
                "  Build prefix: {}\n",
                context.build().path().display()
            ));
        } else {
            output.push_str("  Build prefix: None\n");
        }

        let command = self.command_to_run_script(&work_dir.join("conda_build.bat"), context);
        output.push_str("\nTo run the script manually, use the following command:\n");
        output.push_str(&format!(
            "  cd {:?} && {} {}\n\n",
            work_dir,
            command.program,
            command.args.join(" ")
        ));
        output.push_str("To run commands interactively in the build environment:\n");
        output.push_str(&format!("  cd {:?} && call build_env.bat", work_dir));

        output
    }
}
