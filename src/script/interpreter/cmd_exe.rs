use std::path::PathBuf;

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{interpreter::DEBUG_HELP, run_process_with_replacements, ExecutionArgs};

use super::{find_interpreter, Interpreter};

const CMDEXE_PREAMBLE: &str = r#"
@chcp 65001 > nul
@echo on
IF "%CONDA_BUILD%" == "" (
    @rem special behavior from conda-build for Windows
    call ((script_path))
)
@rem re-enable echo because the activation scripts might have messed with it
@echo on
"#;

pub(crate) struct CmdExeInterpreter;

impl Interpreter for CmdExeInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let script = self.get_script(&args, shell::CmdExe).unwrap();

        let build_env_path = args.work_dir.join("build_env.bat");
        let build_script_path = args.work_dir.join("conda_build.bat");

        tokio::fs::write(&build_env_path, script).await?;

        let build_script = format!(
            "{}\n{}",
            CMDEXE_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy()),
            args.script.script()
        );
        tokio::fs::write(
            &build_script_path,
            &build_script.replace('\n', "\r\n").as_bytes(),
        )
        .await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let cmd_args = ["cmd.exe", "/d", "/c", &build_script_path_str];

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("%((var))%"),
            None,
        )
        .await?;

        if !output.status.success() {
            let status_code = output.status.code().unwrap_or(1);
            tracing::error!("Script failed with status {}", status_code);
            tracing::error!("Work directory: '{}'", args.work_dir.display());
            tracing::error!("{}", DEBUG_HELP);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Script failed".to_string(),
            ));
        }

        Ok(())
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        // check if COMSPEC is set to cmd.exe
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if comspec.to_lowercase().contains("cmd.exe") {
                return Ok(Some(PathBuf::from(comspec)));
            }
        }

        // check if cmd.exe is in PATH
        find_interpreter("cmd", build_prefix, platform)
    }
}
