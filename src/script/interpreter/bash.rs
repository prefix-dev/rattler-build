use std::{
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use rattler_conda_types::Platform;
use rattler_shell::shell;

use crate::script::{
    ExecutionArgs, ResolvedScriptContents, interpreter::DEBUG_HELP, run_process_with_replacements,
};

use super::{BASH_PREAMBLE, CmdExeInterpreter, Interpreter, find_interpreter};

// BaseBashIntercepreter is used to setup activative env,
// use `BashIntercepreter` to execute `build.script`
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
                to_posix_path_string(&bash_script)?.as_str()
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

fn to_posix_path_string(path_buf: &Path) -> Result<String, std::io::Error> {
    if cfg!(not(windows)) {
        return Ok(path_buf.to_string_lossy().into());
    }

    let mut posix_path = String::new();
    let mut first = true;

    for component in path_buf.components() {
        match component {
            Component::Prefix(prefix_comp) => match prefix_comp.kind() {
                std::path::Prefix::DeviceNS(_) => {
                    return Err(std::io::Error::new(
                        ErrorKind::Other,
                        format!("unspport file path {:?}", path_buf),
                    ));
                }
                std::path::Prefix::VerbatimUNC(_os_str_1, _os_str_2) => {
                    return Err(std::io::Error::new(
                        ErrorKind::Other,
                        format!("unspport file path {:?}", path_buf),
                    ));
                }
                std::path::Prefix::UNC(s1, s2) => {
                    posix_path.push_str("//");
                    posix_path.push_str(&s1.to_string_lossy());
                    posix_path.push('/');
                    posix_path.push_str(&s2.to_string_lossy());
                }
                std::path::Prefix::Verbatim(s) => {
                    posix_path.push('/');
                    posix_path.push_str(&s.to_string_lossy());
                }
                std::path::Prefix::VerbatimDisk(disk) => {
                    posix_path.push('/');
                    posix_path.push(disk.into());
                }
                // D: => /D
                std::path::Prefix::Disk(disk) => {
                    posix_path.push('/');
                    posix_path.push(disk.into());
                }
            },
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
        return Ok(String::new()); // Handle empty PathBuf
    }

    if (!posix_path.ends_with('/'))
        && (path_buf.to_string_lossy().ends_with(r"\") || path_buf.to_string_lossy().ends_with("/"))
    {
        posix_path.push('/');
    }

    Ok(posix_path)
}

#[cfg(test)]
#[cfg(windows)]
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
            (PathBuf::from(r"C:\"), "/C/"),
            (PathBuf::from(r"C:\foo\"), "/C/foo/"),
            (PathBuf::from(r"C:\foo\bar.txt"), "/C/foo/bar.txt"),
            (PathBuf::from(r"\\1.1.1.1\a"), "//1.1.1.1/a/"),
            (PathBuf::from(r"\\1.1.1.1\a\b"), "//1.1.1.1/a/b"),
            (PathBuf::from(r"\\1.1.1.1\a\b\"), "//1.1.1.1/a/b/"),
            (PathBuf::from(r"\\1.1.1.1\a\b\c"), "//1.1.1.1/a/b/c"),
            (PathBuf::from(r"\\1.1.1.1\a\b\c\"), "//1.1.1.1/a/b/c/"),
        ];

        for (input, expected) in cases {
            assert_eq!(to_posix_path_string(&input).unwrap(), expected);
        }
    }
}
