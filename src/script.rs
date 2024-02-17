#![allow(missing_docs)]
use fs_err::File;
use indexmap::IndexMap;
use itertools::Itertools;
use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{ActivationError, ActivationVariables, Activator},
    shell::{self, Shell},
};
use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::Write as WriteFmt,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncBufReadExt as _;

use crate::{
    env_vars::{self},
    metadata::Output,
    recipe::parser::{Script, ScriptContent},
};

const BASH_PREAMBLE: &str = r#"
## Start of bash preamble
if [ -z ${CONDA_BUILD+x} ]; then
    source ((script_path))
fi
# enable debug mode for the rest of the script
set -x
## End of preamble
"#;

pub struct ExecutionArgs {
    pub script: String,
    pub env_vars: IndexMap<String, String>,
    pub secrets: IndexMap<String, String>,

    pub execution_platform: Platform,

    pub build_prefix: PathBuf,
    pub run_prefix: PathBuf,

    pub work_dir: PathBuf,
}

impl ExecutionArgs {
    /// Returns strings that should be replaced. The template argument can be used to specify
    /// a nice "variable" syntax, e.g. "$((var))" for bash or "%((var))%" for cmd.exe. The `var` part
    /// will be replaced with the actual variable name.
    pub fn replacements(&self, template: &str) -> HashMap<String, String> {
        let mut replacements = HashMap::new();
        replacements.insert(
            self.build_prefix.to_string_lossy().to_string(),
            template.replace("((var))", "BUILD_PREFIX"),
        );
        replacements.insert(
            self.run_prefix.to_string_lossy().to_string(),
            template.replace("((var))", "PREFIX"),
        );

        self.secrets.iter().for_each(|(_, v)| {
            replacements.insert(v.to_string(), "********".to_string());
        });

        replacements
    }
}

trait Interpreter {
    fn get_script<T: Shell + Copy>(
        &self,
        args: &ExecutionArgs,
        shell_type: T,
    ) -> Result<String, ActivationError> {
        let mut shell_script = shell::ShellScript::new(shell_type, Platform::current());
        for (k, v) in args.env_vars.iter() {
            shell_script.set_env_var(k, v);
        }
        let host_prefix_activator =
            Activator::from_path(&args.run_prefix, shell_type, args.execution_platform)?;

        let current_path = std::env::var("PATH")
            .ok()
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
        let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

        let activation_vars = ActivationVariables {
            conda_prefix,
            path: current_path,
            path_modification_behavior: Default::default(),
        };

        let host_activation = host_prefix_activator.activation(activation_vars)?;

        let build_prefix_activator =
            Activator::from_path(&args.build_prefix, shell_type, args.execution_platform)?;

        // We use the previous PATH and _no_ CONDA_PREFIX to stack the build
        // prefix on top of the host prefix
        let activation_vars = ActivationVariables {
            conda_prefix: None,
            path: Some(host_activation.path.clone()),
            path_modification_behavior: Default::default(),
        };

        let build_activation = build_prefix_activator.activation(activation_vars)?;

        writeln!(shell_script.contents, "{}", host_activation.script)?;
        writeln!(shell_script.contents, "{}", build_activation.script)?;

        Ok(shell_script.contents)
    }

    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error>;
}

struct BashInterpreter;

impl Interpreter for BashInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let script = self.get_script(&args, shell::Bash).unwrap();

        let build_env_path = args.work_dir.join("build_env.sh");
        let build_script_path = args.work_dir.join("conda_build.sh");

        // write the files and make sure they are closed/flushed
        {
            let mut file = File::create(&build_env_path)?;
            file.write_all(script.as_bytes())?;

            let mut exec_file = File::create(&build_script_path)?;
            exec_file.write_all(
                BASH_PREAMBLE
                    .replace("((script_path))", &build_env_path.to_string_lossy())
                    .as_bytes(),
            )?;
            exec_file.write_all(args.script.as_bytes())?;
        }

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let cmd_args = ["bash", "-e", &build_script_path_str];

        run_process_with_replacements(&cmd_args, &args.work_dir, &args.replacements("$((var))"))
            .await?;

        Ok(())
    }
}

struct CmdExeInterpreter;

impl Interpreter for CmdExeInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let script = self.get_script(&args, shell::CmdExe).unwrap();

        let build_env_path = args.work_dir.join("build_env.bat");
        let build_script_path = args.work_dir.join("conda_build.bat");

        // write the files and make sure they are closed/flushed
        {
            let mut file = File::create(&build_env_path)?;
            file.write_all(script.as_bytes())?;

            let mut exec_file = File::create(&build_script_path)?;
            exec_file.write_all(args.script.as_bytes())?;
        }

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let cmd_args = ["cmd.exe", "/d", "/c", &build_script_path_str];

        run_process_with_replacements(&cmd_args, &args.work_dir, &args.replacements("%((var))%"))
            .await?;

        Ok(())
    }
}

impl Script {
    fn get_contents(&self, recipe_dir: &Path) -> Result<String, std::io::Error> {
        let default_extension = if cfg!(windows) { "bat" } else { "sh" };

        let script_content = match self.contents() {
            // No script was specified, so we try to read the default script. If the file cannot be
            // found we return an empty string.
            ScriptContent::Default => {
                let recipe_file =
                    recipe_dir.join(Path::new("build").with_extension(default_extension));
                match std::fs::read_to_string(recipe_file) {
                    Err(err) if err.kind() == ErrorKind::NotFound => String::new(),
                    Err(e) => {
                        return Err(e);
                    }
                    Ok(content) => content,
                }
            }

            // The scripts path was explicitly specified. If the file cannot be found we error out.
            ScriptContent::Path(path) => {
                let path_with_ext = if path.extension().is_none() {
                    Cow::Owned(path.with_extension(default_extension))
                } else {
                    Cow::Borrowed(path.as_path())
                };
                let recipe_file = recipe_dir.join(path_with_ext);
                match std::fs::read_to_string(&recipe_file) {
                    Err(err) if err.kind() == ErrorKind::NotFound => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("recipe file {:?} does not exist", recipe_file.display()),
                        ));
                    }
                    Err(e) => {
                        return Err(e);
                    }
                    Ok(content) => content,
                }
            }
            // The scripts content was specified but it is still ambiguous whether it is a path or the
            // contents of the string. Try to read the file as a script but fall back to using the string
            // as the contents itself if the file is missing.
            ScriptContent::CommandOrPath(path) => {
                let content =
                    if !path.contains('\n') && (path.ends_with(".bat") || path.ends_with(".sh")) {
                        let recipe_file = recipe_dir.join(Path::new(path));
                        match std::fs::read_to_string(recipe_file) {
                            Err(err) if err.kind() == ErrorKind::NotFound => None,
                            Err(e) => {
                                return Err(e);
                            }
                            Ok(content) => Some(content),
                        }
                    } else {
                        None
                    };
                match content {
                    Some(content) => content,
                    None => path.to_owned(),
                }
            }
            ScriptContent::Commands(commands) => commands.iter().join("\n"),
            ScriptContent::Command(command) => command.to_owned(),
        };

        Ok(script_content)
    }

    pub async fn run_script(
        &self,
        env_vars: HashMap<String, String>,
        work_dir: &Path,
        recipe_dir: &Path,
        run_prefix: &Path,
        build_prefix: &Path,
    ) -> Result<(), std::io::Error> {
        let interpreter = self
            .interpreter()
            .unwrap_or(if cfg!(windows) { "cmd" } else { "bash" });

        let contents = self.get_contents(recipe_dir)?;

        let secrets = self
            .secrets()
            .iter()
            .filter_map(|k| {
                let secret = k.to_string();

                if let Ok(value) = std::env::var(&secret) {
                    Some((secret, value))
                } else {
                    tracing::warn!("Secret {} not found in environment", secret);
                    None
                }
            })
            .collect::<IndexMap<String, String>>();

        let env_vars = env_vars
            .into_iter()
            .chain(self.env().clone().into_iter())
            .collect::<IndexMap<String, String>>();

        let exec_args = ExecutionArgs {
            script: contents,
            env_vars,
            secrets,
            build_prefix: build_prefix.to_owned(),
            run_prefix: run_prefix.to_owned(),
            execution_platform: Platform::current(),
            work_dir: work_dir.to_owned(),
        };

        match interpreter {
            "bash" => BashInterpreter.run(exec_args).await?,
            "cmd" => CmdExeInterpreter.run(exec_args).await?,
            _ => unimplemented!(),
        };

        Ok(())
    }
}

impl Output {
    pub async fn run_build_script(&self) -> Result<(), std::io::Error> {
        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));

        self.recipe
            .build()
            .script()
            .run_script(
                env_vars,
                &self.build_configuration.directories.work_dir,
                &self.build_configuration.directories.recipe_dir,
                &self.build_configuration.directories.host_prefix,
                &self.build_configuration.directories.build_prefix,
            )
            .await?;

        Ok(())
    }
}

/// Spawns a process and replaces the given strings in the output with the given replacements.
/// This is used to replace the host prefix with $PREFIX and the build prefix with $BUILD_PREFIX
async fn run_process_with_replacements(
    args: &[&str],
    cwd: &Path,
    replacements: &HashMap<String, String>,
) -> Result<(), tokio::io::Error> {
    let mut command = tokio::process::Command::new(args[0]);
    command
        .current_dir(cwd)
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;

    let stdout = child.stdout.take().expect("Failed to take stdout");
    let stderr = child.stderr.take().expect("Failed to take stderr");

    let mut stdout_lines = tokio::io::BufReader::new(stdout).lines();
    let mut stderr_lines = tokio::io::BufReader::new(stderr).lines();

    loop {
        let line = tokio::select! {
            line = stdout_lines.next_line() => line,
            line = stderr_lines.next_line() => line,
            else => break,
        };

        match line {
            Ok(Some(line)) => {
                let filtered_line = replacements
                    .iter()
                    .fold(line, |acc, (from, to)| acc.replace(from, to));
                tracing::info!("{}", filtered_line);
            }
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("Error reading output: {:?}", e);
            }
        };
    }

    let status = child.wait().await.expect("Failed to wait on child");

    if !status.success() {
        return Err(tokio::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Command failed with status: {:?}", status),
        ));
    }

    Ok(())
}
