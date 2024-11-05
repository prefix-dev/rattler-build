#![allow(missing_docs)]
use indexmap::IndexMap;
use itertools::Itertools;
use minijinja::Value;
use rattler_conda_types::Platform;
use rattler_shell::activation::{prefix_path_entries, PathModificationBehavior};
use rattler_shell::shell::{NuShell, ShellEnum};
use rattler_shell::{
    activation::{ActivationError, ActivationVariables, Activator},
    shell::{self, Shell},
};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Error;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncBufReadExt as _;

use crate::{
    env_vars::{self},
    metadata::Output,
    recipe::{
        parser::{Script, ScriptContent},
        Jinja,
    },
};

const BASH_PREAMBLE: &str = r#"#!/bin/bash
## Start of bash preamble
if [ -z ${CONDA_BUILD+x} ]; then
    source ((script_path))
fi
# enable debug mode for the rest of the script
set -x
## End of preamble
"#;

const DEBUG_HELP : &str  = "To debug the build, run it manually in the work directory (execute the `./conda_build.sh` or `conda_build.bat` script)";

#[derive(Debug)]
pub struct ExecutionArgs {
    pub script: ResolvedScriptContents,
    pub env_vars: IndexMap<String, String>,
    pub secrets: IndexMap<String, String>,

    pub execution_platform: Platform,

    pub build_prefix: Option<PathBuf>,
    pub run_prefix: PathBuf,

    pub work_dir: PathBuf,
}

impl ExecutionArgs {
    /// Returns strings that should be replaced. The template argument can be used to specify
    /// a nice "variable" syntax, e.g. "$((var))" for bash or "%((var))%" for cmd.exe. The `var` part
    /// will be replaced with the actual variable name.
    pub fn replacements(&self, template: &str) -> HashMap<String, String> {
        let mut replacements = HashMap::new();
        if let Some(build_prefix) = &self.build_prefix {
            replacements.insert(
                build_prefix.display().to_string(),
                template.replace("((var))", "BUILD_PREFIX"),
            );
        };
        replacements.insert(
            self.run_prefix.display().to_string(),
            template.replace("((var))", "PREFIX"),
        );

        replacements.insert(
            self.work_dir.display().to_string(),
            template.replace("((var))", "SRC_DIR"),
        );

        // if the paths contain `\` then also replace the forward slash variants
        for (k, v) in replacements.clone() {
            if k.contains('\\') {
                replacements.insert(k.replace('\\', "/"), v.clone());
            }
        }

        self.secrets.iter().for_each(|(_, v)| {
            replacements.insert(v.to_string(), "********".to_string());
        });

        replacements
    }
}

fn find_interpreter(
    name: &str,
    build_prefix: Option<&PathBuf>,
    platform: &Platform,
) -> Result<Option<PathBuf>, which::Error> {
    let exe_name = format!("{}{}", name, std::env::consts::EXE_SUFFIX);

    let path = std::env::var("PATH").unwrap_or_default();
    if let Some(build_prefix) = build_prefix {
        let mut prepend_path = prefix_path_entries(build_prefix, platform)
            .into_iter()
            .collect::<Vec<_>>();
        prepend_path.extend(std::env::split_paths(&path));
        return Ok(
            which::which_in_global(exe_name, std::env::join_paths(prepend_path).ok())?.next(),
        );
    }

    Ok(which::which_in_global(exe_name, Some(path))?.next())
}

trait Interpreter {
    fn get_script<T: Shell + Copy + 'static>(
        &self,
        args: &ExecutionArgs,
        shell_type: T,
    ) -> Result<String, ActivationError> {
        let mut shell_script = shell::ShellScript::new(shell_type, Platform::current());
        for (k, v) in args.env_vars.iter() {
            shell_script.set_env_var(k, v)?;
        }
        let host_prefix_activator =
            Activator::from_path(&args.run_prefix, shell_type, args.execution_platform)?;

        let current_path = std::env::var(shell_type.path_var(&args.execution_platform))
            .ok()
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
        let conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

        let activation_vars = ActivationVariables {
            conda_prefix,
            path: current_path,
            path_modification_behavior: Default::default(),
        };

        let host_activation = host_prefix_activator.activation(activation_vars)?;

        if let Some(build_prefix) = &args.build_prefix {
            let build_prefix_activator =
                Activator::from_path(build_prefix, shell_type, args.execution_platform)?;

            let activation_vars = ActivationVariables {
                conda_prefix: None,
                path: Some(host_activation.path.clone()),
                path_modification_behavior: Default::default(),
            };

            let build_activation = build_prefix_activator.activation(activation_vars)?;
            shell_script.append_script(&host_activation.script);
            shell_script.append_script(&build_activation.script);
        } else {
            shell_script.append_script(&host_activation.script);
        }

        Ok(shell_script.contents()?)
    }

    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error>;

    #[allow(dead_code)]
    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error>;
}

struct BashInterpreter;

impl Interpreter for BashInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let script = self.get_script(&args, shell::Bash).unwrap();

        let build_env_path = args.work_dir.join("build_env.sh");
        let build_script_path = args.work_dir.join("conda_build.sh");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble = BASH_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
        let script = format!("{}\n{}", preamble, args.script.script());
        tokio::fs::write(&build_script_path, script).await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();
        let cmd_args = ["bash", "-e", &build_script_path_str];

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("$((var))"),
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

struct NuShellInterpreter;

const NUSHELL_PREAMBLE: &str = r#"
## Start of bash preamble
if not ("CONDA_BUILD" in $env) {
    source-env ((script_path))
}

## End of preamble
"#;

impl Interpreter for NuShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), Error> {
        let host_shell_type = ShellEnum::default();
        let nushell = ShellEnum::NuShell(Default::default());

        // Create a map of environment variables to pass to the shell script
        let mut activation_variables: HashMap<_, _> = HashMap::from_iter(args.env_vars.clone());

        // Read some of the current environment variables
        let current_path = std::env::var(nushell.path_var(&args.execution_platform))
            .map(|p| std::env::split_paths(&p).collect_vec())
            .ok();
        let current_conda_prefix = std::env::var("CONDA_PREFIX").ok().map(|p| p.into());

        // Run the activation script for the host environment.
        let activation_vars = ActivationVariables {
            conda_prefix: current_conda_prefix,
            path: current_path,
            path_modification_behavior: PathModificationBehavior::default(),
        };

        let host_prefix_activator = Activator::from_path(
            &args.run_prefix,
            host_shell_type.clone(),
            args.execution_platform,
        )
        .unwrap();

        let host_activation_variables = host_prefix_activator
            .run_activation(activation_vars, None)
            .unwrap();

        // Overwrite the current environment variables with the one from the activated host environment.
        activation_variables.extend(host_activation_variables);

        // If there is a build environment run the activation script for that environment and extend
        // the activation variables with the new environment variables.
        if let Some(build_prefix) = &args.build_prefix {
            let build_prefix_activator =
                Activator::from_path(build_prefix, host_shell_type, args.execution_platform)
                    .unwrap();

            let activation_vars = ActivationVariables {
                conda_prefix: None,
                path: activation_variables
                    .get(nushell.path_var(&args.execution_platform))
                    .map(|path| std::env::split_paths(&path).collect()),
                path_modification_behavior: PathModificationBehavior::default(),
            };

            let build_activation = build_prefix_activator
                .run_activation(activation_vars, None)
                .unwrap();

            activation_variables.extend(build_activation);
        }

        // Construct a shell script with the activation variables.
        let mut shell_script = shell::ShellScript::new(NuShell, Platform::current());
        for (k, v) in activation_variables.iter() {
            shell_script.set_env_var(k, v).unwrap();
        }
        let script = shell_script
            .contents()
            .expect("failed to construct shell script");

        let build_env_path = args.work_dir.join("build_env.nu");
        let build_script_path = args.work_dir.join("conda_build.nu");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble =
            NUSHELL_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
        let script = format!("{}\n{}", preamble, args.script.script());
        tokio::fs::write(&build_script_path, script).await?;

        let build_script_path_str = build_script_path.to_string_lossy().to_string();

        let nu_path =
            match find_interpreter("nu", args.build_prefix.as_ref(), &args.execution_platform) {
                Ok(Some(path)) => path,
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "NuShell executable not found in PATH",
                    ));
                }
            }
            .to_string_lossy()
            .to_string();

        let cmd_args = [nu_path.as_str(), build_script_path_str.as_str()];

        let output = run_process_with_replacements(
            &cmd_args,
            &args.work_dir,
            &args.replacements("$((var))"),
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
        find_interpreter("nu", build_prefix, platform)
    }
}

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

struct CmdExeInterpreter;

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

struct PythonInterpreter;

// python interpreter calls either bash or cmd.exe interpreter for activation and then runs python script
impl Interpreter for PythonInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), std::io::Error> {
        let py_script = args.work_dir.join("conda_build_script.py");
        tokio::fs::write(&py_script, args.script.script()).await?;

        let args = ExecutionArgs {
            script: ResolvedScriptContents::Inline(format!("python {:?}", py_script)),
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
        find_interpreter("python", build_prefix, platform)
    }
}

#[derive(Debug)]
pub enum ResolvedScriptContents {
    Path(PathBuf, String),
    Inline(String),
    Missing,
}

impl ResolvedScriptContents {
    pub fn script(&self) -> &str {
        match self {
            ResolvedScriptContents::Path(_, script) => script,
            ResolvedScriptContents::Inline(script) => script,
            ResolvedScriptContents::Missing => "",
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            ResolvedScriptContents::Path(path, _) => Some(path),
            _ => None,
        }
    }
}

impl Script {
    fn find_file(&self, recipe_dir: &Path, extensions: &[&str], path: &Path) -> Option<PathBuf> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            recipe_dir.join(path)
        };

        if path.extension().is_none() {
            extensions
                .iter()
                .map(|ext| path.with_extension(ext))
                .find(|p| p.is_file())
        } else if path.is_file() {
            Some(path)
        } else {
            None
        }
    }

    pub(crate) fn resolve_content(
        &self,
        recipe_dir: &Path,
        jinja_context: Option<Jinja>,
        extensions: &[&str],
    ) -> Result<ResolvedScriptContents, std::io::Error> {
        let script_content = match self.contents() {
            // No script was specified, so we try to read the default script. If the file cannot be
            // found we return an empty string.
            ScriptContent::Default => {
                let recipe_file = self.find_file(recipe_dir, extensions, Path::new("build"));
                if let Some(recipe_file) = recipe_file {
                    match fs_err::read_to_string(&recipe_file) {
                        Err(e) => Err(e),
                        Ok(content) => Ok(ResolvedScriptContents::Path(recipe_file, content)),
                    }
                } else {
                    Ok(ResolvedScriptContents::Missing)
                }
            }

            // The scripts path was explicitly specified. If the file cannot be found we error out.
            ScriptContent::Path(path) => {
                let recipe_file = self.find_file(recipe_dir, extensions, path);
                if let Some(recipe_file) = recipe_file {
                    match fs_err::read_to_string(&recipe_file) {
                        Err(e) => Err(e),
                        Ok(content) => Ok(ResolvedScriptContents::Path(recipe_file, content)),
                    }
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("could not resolve recipe file {:?}", path.display()),
                    ))
                }
            }
            // The scripts content was specified but it is still ambiguous whether it is a path or the
            // contents of the string. Try to read the file as a script but fall back to using the string
            // as the contents itself if the file is missing.
            ScriptContent::CommandOrPath(path) => {
                if path.contains('\n') {
                    Ok(ResolvedScriptContents::Inline(path.clone()))
                } else {
                    let resolved_path = self.find_file(recipe_dir, extensions, Path::new(path));
                    if let Some(resolved_path) = resolved_path {
                        match fs_err::read_to_string(&resolved_path) {
                            Err(e) => Err(e),
                            Ok(content) => Ok(ResolvedScriptContents::Path(resolved_path, content)),
                        }
                    } else {
                        Ok(ResolvedScriptContents::Inline(path.clone()))
                    }
                }
            }
            ScriptContent::Commands(commands) => {
                Ok(ResolvedScriptContents::Inline(commands.iter().join("\n")))
            }
            ScriptContent::Command(command) => {
                Ok(ResolvedScriptContents::Inline(command.to_owned()))
            }
        };

        // render jinja if it is an inline script
        if let Some(jinja_context) = jinja_context {
            match script_content? {
                ResolvedScriptContents::Inline(script) => {
                    let rendered = jinja_context.render_str(&script).map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to render jinja template in build `script`: {}", e),
                        )
                    })?;
                    Ok(ResolvedScriptContents::Inline(rendered))
                }
                other => Ok(other),
            }
        } else {
            script_content
        }
    }

    pub async fn run_script(
        &self,
        env_vars: HashMap<String, Option<String>>,
        work_dir: &Path,
        recipe_dir: &Path,
        run_prefix: &Path,
        build_prefix: Option<&PathBuf>,
        mut jinja_config: Option<Jinja<'_>>,
    ) -> Result<(), std::io::Error> {
        // TODO: This is a bit of an out and about way to determine whether or
        //  not nushell is available. It would be best to run the activation
        //  of the environment and see if nu is on the path, but hat is a
        //  pretty expensive operation. So instead we just check if the nu
        //  executable is in a known place.
        let nushell_path = format!("bin/nu{}", std::env::consts::EXE_SUFFIX);
        let has_nushell = build_prefix
            .map(|p| p.join(nushell_path))
            .map(|p| p.is_file())
            .unwrap_or(false);
        if has_nushell {
            tracing::debug!("Nushell is available to run build scripts");
        }

        // Determine the user defined interpreter.
        let mut interpreter =
            self.interpreter()
                .unwrap_or(if cfg!(windows) { "cmd" } else { "bash" });
        let interpreter_is_nushell = interpreter == "nushell" || interpreter == "nu";

        // Determine the valid script extensions based on the available interpreters.
        let mut valid_script_extensions = Vec::new();
        if cfg!(windows) {
            valid_script_extensions.push("bat");
        } else {
            valid_script_extensions.push("sh");
        }
        if has_nushell || interpreter_is_nushell {
            valid_script_extensions.push("nu");
        }

        let env_vars = env_vars
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .chain(self.env().clone().into_iter())
            .collect::<IndexMap<String, String>>();

        // Get the contents of the script.
        for (k, v) in &env_vars {
            jinja_config.as_mut().map(|jinja| {
                jinja
                    .context_mut()
                    .insert(k.clone(), Value::from_safe_string(v.clone()))
            });
        }

        let contents = self.resolve_content(recipe_dir, jinja_config, &valid_script_extensions)?;

        // Select a different interpreter if the script is a nushell script.
        if contents
            .path()
            .and_then(|p| p.extension())
            .and_then(OsStr::to_str)
            == Some("nu")
            && !(interpreter == "nushell" || interpreter == "nu")
        {
            tracing::info!("Using nushell interpreter for script");
            interpreter = "nushell";
        }

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

        let work_dir = if let Some(cwd) = self.cwd.as_ref() {
            run_prefix.join(cwd)
        } else {
            work_dir.to_owned()
        };

        tracing::debug!("Running script in {}", work_dir.display());

        let exec_args = ExecutionArgs {
            script: contents,
            env_vars,
            secrets,
            build_prefix: build_prefix.map(|p| p.to_owned()),
            run_prefix: run_prefix.to_owned(),
            execution_platform: Platform::current(),
            work_dir,
        };

        match interpreter {
            "nushell" | "nu" => {
                if !has_nushell {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Nushell is not installed, did you add `nushell` to the build dependencies?".to_string(),
                    ));
                }
                NuShellInterpreter.run(exec_args).await?
            }
            "bash" => BashInterpreter.run(exec_args).await?,
            "cmd" => CmdExeInterpreter.run(exec_args).await?,
            "python" => PythonInterpreter.run(exec_args).await?,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Unsupported interpreter: {}", interpreter),
                ))
            }
        };

        Ok(())
    }
}

impl Output {
    /// Add environment variables from the variant to the environment variables.
    fn env_vars_from_variant(&self) -> HashMap<String, Option<String>> {
        let languages: HashSet<&str> = HashSet::from(["PERL", "LUA", "R", "NUMPY", "PYTHON"]);
        self.variant()
            .iter()
            .filter_map(|(k, v)| {
                let key_upper = k.to_uppercase();
                if !languages.contains(key_upper.as_str()) {
                    Some((k.replace('-', "_"), Some(v.to_string())))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn run_build_script(&self) -> Result<(), std::io::Error> {
        let span = tracing::info_span!("Running build script");
        let _enter = span.enter();

        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));
        env_vars.extend(self.env_vars_from_variant());

        let selector_config = self.build_configuration.selector_config();
        let mut jinja = Jinja::new(selector_config.clone());
        for (k, v) in self.recipe.context.iter() {
            jinja
                .context_mut()
                .insert(k.clone(), Value::from_safe_string(v.clone()));
        }

        self.recipe
            .build()
            .script()
            .run_script(
                env_vars,
                &self.build_configuration.directories.work_dir,
                &self.build_configuration.directories.recipe_dir,
                &self.build_configuration.directories.host_prefix,
                Some(&self.build_configuration.directories.build_prefix),
                Some(jinja),
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
) -> Result<std::process::Output, std::io::Error> {
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

    let mut stdout_log = String::new();
    let mut stderr_log = String::new();
    let mut closed = (false, false);

    loop {
        let (line, is_stderr) = tokio::select! {
            line = stdout_lines.next_line() => (line, false),
            line = stderr_lines.next_line() => (line, true),
            else => break,
        };

        match line {
            Ok(Some(line)) => {
                let filtered_line = replacements
                    .iter()
                    .fold(line, |acc, (from, to)| acc.replace(from, to));

                if is_stderr {
                    stderr_log.push_str(&filtered_line);
                    stderr_log.push('\n');
                } else {
                    stdout_log.push_str(&filtered_line);
                    stdout_log.push('\n');
                }

                tracing::info!("{}", filtered_line);
            }
            Ok(None) if !is_stderr => closed.0 = true,
            Ok(None) if is_stderr => closed.1 = true,
            Ok(None) => unreachable!(),
            Err(e) => {
                tracing::warn!("Error reading output: {:?}", e);
                break;
            }
        };
        // make sure we close the loop when both stdout and stderr are closed
        if closed == (true, true) {
            break;
        }
    }

    let status = child.wait().await?;

    Ok(std::process::Output {
        status,
        stdout: stdout_log.into_bytes(),
        stderr: stderr_log.into_bytes(),
    })
}
