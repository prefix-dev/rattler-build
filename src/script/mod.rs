//! Module for running scripts in different interpreters.
mod interpreter;
mod sandbox;
pub use sandbox::{SandboxArguments, SandboxConfiguration};

use crate::script::interpreter::Interpreter;
use indexmap::IndexMap;
use interpreter::{
    BashInterpreter, CmdExeInterpreter, NuShellInterpreter, PerlInterpreter, PythonInterpreter,
    BASH_PREAMBLE, CMDEXE_PREAMBLE,
};
use itertools::Itertools;
use minijinja::Value;
use rattler_conda_types::Platform;
use rattler_shell::shell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
};
use tokio::io::{AsyncBufReadExt, AsyncRead, ReadBuf};

use crate::{
    env_vars::{self},
    metadata::{Debug, Output},
    recipe::{
        parser::{Script, ScriptContent},
        Jinja,
    },
};

/// Arguments for executing a script in a given interpreter.
#[derive(Debug)]
pub struct ExecutionArgs {
    /// Contents of the script to execute
    pub script: ResolvedScriptContents,
    /// Environment variables to set before executing the script
    pub env_vars: IndexMap<String, String>,
    /// Secrets to set as env vars and replace in the output
    pub secrets: IndexMap<String, String>,

    /// The platform on which the script should be executed
    pub execution_platform: Platform,

    /// The build prefix that should contain the interpreter to use
    pub build_prefix: Option<PathBuf>,
    /// The prefix to use for the script execution
    pub run_prefix: PathBuf,

    /// The working directory (`cwd`) in which the script should execute
    pub work_dir: PathBuf,

    /// The sandbox configuration to use for the script execution
    pub sandbox_config: Option<SandboxConfiguration>,

    /// Whether to enable debug output
    pub debug: Debug,
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

/// The resolved contents of a script.
#[derive(Debug)]
pub enum ResolvedScriptContents {
    /// The script contents as loaded from a file (path, contents)
    Path(PathBuf, String),
    /// The script contents from an inline YAML string
    Inline(String),
    /// There are no script contents
    Missing,
}

impl ResolvedScriptContents {
    /// Get the script contents as a string
    pub fn script(&self) -> &str {
        match self {
            ResolvedScriptContents::Path(_, script) => script,
            ResolvedScriptContents::Inline(script) => script,
            ResolvedScriptContents::Missing => "",
        }
    }

    /// Get the path to the script file (if it was loaded from a file)
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

    /// Run the script with the given parameters
    #[allow(clippy::too_many_arguments)]
    pub async fn run_script(
        &self,
        env_vars: HashMap<String, Option<String>>,
        work_dir: &Path,
        recipe_dir: &Path,
        run_prefix: &Path,
        build_prefix: Option<&PathBuf>,
        mut jinja_config: Option<Jinja>,
        sandbox_config: Option<&SandboxConfiguration>,
        debug: Debug,
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
            sandbox_config: sandbox_config.cloned(),
            debug,
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
            "perl" => PerlInterpreter.run(exec_args).await?,
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
                let key_upper = k.normalize().to_uppercase();
                if !languages.contains(key_upper.as_str()) {
                    Some((k.normalize(), Some(v.to_string())))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Helper method to prepare build script execution arguments
    async fn prepare_build_script(&self) -> Result<ExecutionArgs, std::io::Error> {
        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));
        env_vars.extend(self.env_vars_from_variant());

        let selector_config = self.build_configuration.selector_config();
        let jinja = Jinja::new(selector_config.clone()).with_context(&self.recipe.context);

        let build_prefix = if self.recipe.build().merge_build_and_host_envs() {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        let work_dir = &self.build_configuration.directories.work_dir;
        Ok(ExecutionArgs {
            script: self.recipe.build().script().resolve_content(
                &self.build_configuration.directories.recipe_dir,
                Some(jinja.clone()),
                if cfg!(windows) { &["bat"] } else { &["sh"] },
            )?,
            env_vars: env_vars
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| (k, v)))
                .collect(),
            secrets: IndexMap::new(),
            build_prefix: build_prefix.map(|p| p.to_owned()),
            run_prefix: host_prefix,
            execution_platform: Platform::current(),
            work_dir: work_dir.clone(),
            sandbox_config: self.build_configuration.sandbox_config().cloned(),
            debug: self.build_configuration.debug,
        })
    }

    /// Run the build script for the output as defined in the recipe's build section.
    ///
    /// This method executes the build script with the configured environment variables,
    /// working directory, and other build settings. The script execution respects the
    /// configured interpreter (bash/cmd/nushell) and sandbox settings.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if:
    /// - The script file cannot be read or found
    /// - The script execution fails
    /// - The interpreter is not supported or not available
    pub async fn run_build_script(&self) -> Result<(), std::io::Error> {
        let span = tracing::info_span!("Running build script");
        let _enter = span.enter();

        let exec_args = self.prepare_build_script().await?;
        let build_prefix = if self.recipe.build().merge_build_and_host_envs() {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        self.recipe
            .build()
            .script()
            .run_script(
                exec_args
                    .env_vars
                    .into_iter()
                    .map(|(k, v)| (k, Some(v)))
                    .collect(),
                &self.build_configuration.directories.work_dir,
                &self.build_configuration.directories.recipe_dir,
                &self.build_configuration.directories.host_prefix,
                build_prefix,
                Some(
                    Jinja::new(self.build_configuration.selector_config())
                        .with_context(&self.recipe.context),
                ),
                self.build_configuration.sandbox_config(),
                self.build_configuration.debug,
            )
            .await?;

        Ok(())
    }

    /// Create the build script files without executing them.
    ///
    /// This method generates the build script and environment setup files in the working
    /// directory but does not execute them. This is useful for debugging or when you want
    /// to inspect or modify the scripts before running them manually.
    ///
    /// The method creates two files:
    /// - A build environment setup file (`build_env.sh`/`build_env.bat`)
    /// - The main build script file (`conda_build.sh`/`conda_build.bat`)
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if:
    /// - The script file cannot be read or found
    /// - The script files cannot be written to the working directory
    pub async fn create_build_script(&self) -> Result<(), std::io::Error> {
        let span = tracing::info_span!("Creating build script");
        let _enter = span.enter();

        let exec_args = self.prepare_build_script().await?;
        let interpreter = if cfg!(windows) { "cmd" } else { "bash" };
        let work_dir = &self.build_configuration.directories.work_dir;

        if interpreter == "bash" {
            let script = BashInterpreter.get_script(&exec_args, shell::Bash).unwrap();
            let build_env_path = work_dir.join("build_env.sh");
            let build_script_path = work_dir.join("conda_build.sh");

            tokio::fs::write(&build_env_path, script).await?;

            let preamble =
                BASH_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
            let script = format!("{}\n{}", preamble, exec_args.script.script());
            tokio::fs::write(&build_script_path, script).await?;

            tracing::info!("Build script created at {}", build_script_path.display());
        } else if interpreter == "cmd" {
            let script = CmdExeInterpreter
                .get_script(&exec_args, shell::CmdExe)
                .unwrap();
            let build_env_path = work_dir.join("build_env.bat");
            let build_script_path = work_dir.join("conda_build.bat");

            tokio::fs::write(&build_env_path, script).await?;

            let build_script = format!(
                "{}\n{}",
                CMDEXE_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy()),
                exec_args.script.script()
            );
            tokio::fs::write(
                &build_script_path,
                &build_script.replace('\n', "\r\n").as_bytes(),
            )
            .await?;

            tracing::info!("Build script created at {}", build_script_path.display());
        }

        Ok(())
    }
}

/// An AsyncRead wrapper that replaces carriage return (\r) bytes with newline (\n) bytes.
struct CarriageReturnToNewline<R: AsyncRead + Unpin> {
    inner: R,
}

impl<R: AsyncRead + Unpin> AsyncRead for CarriageReturnToNewline<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let initial_filled = buf.filled().len();

        // Poll the inner reader
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                // Get a mutable slice of the *newly* filled part of the buffer
                let newly_filled = &mut buf.filled_mut()[initial_filled..];
                let len = newly_filled.len();

                if len == 0 {
                    return Poll::Ready(Ok(()));
                }

                // Fast path for small buffers
                if len <= 32 {
                    let has_cr = newly_filled.iter().take(len).any(|&byte| byte == b'\r');

                    if !has_cr {
                        return Poll::Ready(Ok(()));
                    }
                }

                let mut i = 0;
                while i < len {
                    let chunk_end = std::cmp::min(i + 64, len);
                    let chunk_has_cr = newly_filled
                        .iter()
                        .skip(i)
                        .take(chunk_end - i)
                        .any(|&byte| byte == b'\r');

                    if !chunk_has_cr {
                        i = chunk_end;
                        continue;
                    }

                    while i < chunk_end {
                        if newly_filled[i] == b'\r'
                            && (i + 1 >= len || newly_filled[i + 1] != b'\n')
                        {
                            newly_filled[i] = b'\n';
                        }
                        i += 1;
                    }
                }

                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Spawns a process and replaces the given strings in the output with the given replacements.
/// This is used to replace the host prefix with $PREFIX and the build prefix with $BUILD_PREFIX
async fn run_process_with_replacements(
    args: &[&str],
    cwd: &Path,
    replacements: &HashMap<String, String>,
    sandbox_config: Option<&SandboxConfiguration>,
) -> Result<std::process::Output, std::io::Error> {
    let mut command = if let Some(sandbox_config) = sandbox_config {
        #[cfg(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            target_os = "macos"
        ))]
        {
            tracing::info!("{}", sandbox_config);
            rattler_sandbox::tokio::sandboxed_command(
                args[0],
                &sandbox_config.with_cwd(cwd).exceptions(),
            )
        }

        // If the platform is not supported, log a warning and run the command without sandboxing
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            target_os = "macos"
        )))]
        {
            tracing::warn!("Sandboxing is not supported on this platform");
            // mark variable as used
            let _ = sandbox_config;
            tokio::process::Command::new(args[0])
        }
    } else {
        tokio::process::Command::new(args[0])
    };

    command
        .current_dir(cwd)
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;

    let stdout = child.stdout.take().expect("Failed to take stdout");
    let stderr = child.stderr.take().expect("Failed to take stderr");

    let stdout_wrapped = CarriageReturnToNewline { inner: stdout };
    let stderr_wrapped = CarriageReturnToNewline { inner: stderr };

    let mut stdout_lines = tokio::io::BufReader::new(stdout_wrapped).lines();
    let mut stderr_lines = tokio::io::BufReader::new(stderr_wrapped).lines();

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
