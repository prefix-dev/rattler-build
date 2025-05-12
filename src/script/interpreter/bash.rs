use std::path::{Component, Path, PathBuf};

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{interpreter::DEBUG_HELP, run_process_with_replacements, ExecutionArgs, ResolvedScriptContents};

use super::{find_interpreter, CmdExeInterpreter, Interpreter, BASH_PREAMBLE};

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

fn to_posix_path_string(path_buf: &Path) -> String {
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
            }
            Component::RootDir => {
                if !posix_path.ends_with('/') {
                    posix_path.push('/');
                }
            }
            Component::CurDir => {
                if !first && !posix_path.ends_with('/') {
                    posix_path.push('/');
                }
                posix_path.push('.');
            }
            Component::ParentDir => {
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

    posix_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_posix_path_string() {
        let cases = vec![
            (PathBuf::from("/usr/local/bin"), "/usr/local/bin"),
            (
                PathBuf::from("relative/path/to/file"),
                "relative/path/to/file",
            ),
            (PathBuf::from(r"C:\foo\bar.txt"), "C:/foo/bar.txt"),
            (
                PathBuf::from(r"\\server\share\file.zip"),
                "//server/share/file.zip",
            ),
            (PathBuf::from(r"C:"), "C:"),
            (PathBuf::from(r"C:\"), "C:/"),
        ];

        for (input, expected) in cases {
            assert_eq!(
                to_posix_path_string(&input),
                expected,
                "Failed for input: {:?}",
                input
            );
        }
    }
}
