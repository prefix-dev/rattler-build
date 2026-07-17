use std::fmt::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use rattler_shell::shell::{self, Shell};

use super::{CommandSpec, NativeShellRunner, windows_machine_transition};
use crate::{ExecutionContext, PrefixLayout};

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

    fn command_to_run_script(
        &self,
        build_script_path: &Path,
        context: &ExecutionContext,
    ) -> CommandSpec {
        if let Some(machine) = windows_machine_transition(
            context.runtime().process_platform(),
            context.build().platform(),
        ) {
            let script_name = build_script_path
                .file_name()
                .expect("generated build script has a filename")
                .to_string_lossy();
            let command = format!(
                "start /b /wait /machine {} cmd.exe /d /c {} & exit /b !ERRORLEVEL!",
                machine.start_argument(),
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

    /// `setlocal`/`endlocal` scope plus an errorlevel guard. The guard is
    /// required even on the last section: falling off the end after `endlocal`
    /// exits 0 regardless of failure, but `endlocal` preserves the
    /// `%errorlevel%` value so the guard still catches it.
    fn scope_section(
        &self,
        label: Option<&str>,
        env: &IndexMap<String, String>,
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
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("endlocal\nif %errorlevel% neq 0 exit /b %errorlevel%");
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
