use std::path::{Component, PathBuf};

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{
    ExecutionArgs, ResolvedScriptContents, interpreter::DEBUG_HELP, run_process_with_replacements,
};

use super::{CmdExeInterpreter, Interpreter, find_interpreter};

const BASH_PREAMBLE: &str = r#"#!/bin/bash
## Start of bash preamble
if [ -z ${CONDA_BUILD+x} ]; then
    source ((script_path))
fi
# enable debug mode for the rest of the script
set -x
## End of preamble
"#;

// BaseBashIntercepreter is used to setup activative env,
//
pub(crate) struct BaseBashInterpreter;

impl Interpreter for BaseBashInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let script = self.get_script(&args, shell::Bash).unwrap();

        let build_env_path = args.work_dir.join("build_env.sh");
        let build_script_path = args.work_dir.join("conda_build.sh");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble = BASH_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
        let script = format!("{}\n{}", preamble, args.script.script());
        tokio::fs::write(&build_script_path, script).await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let mut cmd_args = vec!["bash", "-e"];
        if args.debug.is_enabled() {
            cmd_args.push("-x");
        }
        cmd_args.push(&build_script_path_str);

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("$((var))"),
            args.sandbox_config.as_ref(),
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
        find_interpreter("bash", build_prefix, platform)
    }
}

// BashInterpreter is used to execute user build script
pub(crate) struct BashInterpreter;
impl Interpreter for BashInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let bash_script = args.work_dir.join("conda_build_script.bash");
        tokio::fs::write(&bash_script, args.script.script()).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!(
                "bash {:?}",
                to_posix_path_string(&bash_script).as_str()
            )),
            ..args
        };

        if cfg!(windows) {
            CmdExeInterpreter.run(args).await
        } else {
            BaseBashInterpreter.run(args).await
        }
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        let base = BaseBashInterpreter {};
        return base.find_interpreter(build_prefix, platform).await;
    }
}

fn to_posix_path_string(path_buf: &PathBuf) -> String {
    let mut posix_path = String::new();
    let mut first = true;

    for component in path_buf.components() {
        match component {
            Component::Prefix(prefix_comp) => {
                // On Windows, this could be "C:", "\\?\C:", or a UNC prefix like "\\server\share".
                // For simplicity, we'll take its lossy string version.
                // A true POSIX representation of Windows drive letters or UNC paths
                // can be ambiguous (e.g., "/mnt/c/" or "/server/share").
                // This example will produce something like "C:" or "//server/share".
                posix_path.push_str(&prefix_comp.as_os_str().to_string_lossy());
                // If the prefix doesn't naturally end in a separator (like "C:"),
                // and there are more components, a separator will be added before the next component.
            }
            Component::RootDir => {
                // Ensure the path starts with a slash if it's an absolute path,
                // or if a prefix was present (e.g. "C:" becomes "C:/").
                if !posix_path.ends_with('/') {
                    posix_path.push('/');
                }
            }
            Component::CurDir => {
                // "."
                if !first && !posix_path.ends_with('/') {
                    posix_path.push('/');
                }
                posix_path.push('.');
            }
            Component::ParentDir => {
                // ".."
                if !first && !posix_path.ends_with('/') {
                    posix_path.push('/');
                }
                posix_path.push_str("..");
            }
            Component::Normal(path_segment) => {
                if !first && !posix_path.ends_with('/') {
                    posix_path.push('/');
                }
                posix_path.push_str(&path_segment.to_string_lossy());
            }
        }
        first = false;
    }

    if path_buf.as_os_str().is_empty() {
        return String::new(); // Handle empty PathBuf
    }

    // If the path was just a prefix (e.g. "C:") and no RootDir,
    // and the original path_buf didn't end with a separator,
    // the result might be just "C:". If it was "C:\", it would be "C:/".
    // If the path was relative and empty (e.g. PathBuf::new("")) it should be empty.
    // If the path was "." it should be "."

    posix_path
}
