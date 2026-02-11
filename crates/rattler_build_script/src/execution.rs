//! Script execution types and utilities.

use crate::sandbox::SandboxConfiguration;
use crate::script::{Script, ScriptContent};
use futures::TryStreamExt;
use indexmap::IndexMap;
use itertools::Itertools;
use rattler_conda_types::Platform;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt};
use tokio_util::bytes::BytesMut;
use tokio_util::codec::{Decoder, FramedRead};
use tokio_util::compat::FuturesAsyncReadCompatExt;

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

    /// Determine interpreter based on file extension from the path
    pub fn infer_interpreter(&self) -> Option<String> {
        self.path()
            .and_then(crate::script::determine_interpreter_from_path)
    }
}

/// Debug mode for script execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debug(bool);

impl Debug {
    /// Create a new Debug mode
    pub fn new(enabled: bool) -> Self {
        Self(enabled)
    }

    /// Check if debug mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.0
    }
}

impl From<bool> for Debug {
    fn from(enabled: bool) -> Self {
        Self(enabled)
    }
}

impl Script {
    /// Run the script with the given parameters
    ///
    /// This is a high-level convenience method that handles the full script execution flow:
    /// - Resolves script content (from file or inline)
    /// - Sets up environment variables and secrets
    /// - Configures the working directory
    /// - Renders Jinja templates if a renderer is provided
    /// - Executes the script in the appropriate interpreter
    #[allow(clippy::too_many_arguments)]
    pub async fn run_script<F>(
        &self,
        env_vars: HashMap<String, Option<String>>,
        work_dir: &Path,
        recipe_dir: &Path,
        run_prefix: &Path,
        build_prefix: Option<&PathBuf>,
        jinja_renderer: Option<F>,
        sandbox_config: Option<&SandboxConfiguration>,
        debug: Debug,
    ) -> Result<(), crate::InterpreterError>
    where
        F: Fn(&str) -> Result<String, String>,
    {
        // Determine the valid script extensions based on the available interpreters.
        let mut valid_script_extensions = Vec::new();
        if cfg!(windows) {
            valid_script_extensions.push("bat");
        } else {
            valid_script_extensions.push("sh");
        }

        let env_vars = env_vars
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .chain(self.env().clone().into_iter())
            .collect::<IndexMap<String, String>>();

        let contents =
            self.resolve_content(recipe_dir, jinja_renderer, &valid_script_extensions)?;

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

        // Determine the interpreter to use:
        // 1. Use explicitly specified interpreter if set
        // 2. Try to infer from the resolved script path (if it's a file)
        // 3. Finally fall back to platform default (bash/cmd)
        let inferred_interpreter = contents.infer_interpreter();
        let interpreter = if self.interpreter.is_some() {
            self.interpreter()
        } else if let Some(ref inferred) = inferred_interpreter {
            tracing::debug!("Inferred interpreter '{}' from script file path", inferred);
            inferred.as_str()
        } else {
            self.interpreter()
        };

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

        crate::execution::run_script(exec_args, interpreter).await?;

        Ok(())
    }

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

    /// Resolve the script content to actual script text
    ///
    /// If `jinja_renderer` is provided, it will be used to render inline scripts.
    /// The renderer function takes a template string and returns the rendered result.
    pub fn resolve_content<F>(
        &self,
        recipe_dir: &Path,
        jinja_renderer: Option<F>,
        extensions: &[&str],
    ) -> Result<ResolvedScriptContents, std::io::Error>
    where
        F: Fn(&str) -> Result<String, String>,
    {
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
                if self.interpreter() == "cmd" {
                    // add in an `if %errorlevel% neq 0` check
                    Ok(ResolvedScriptContents::Inline(
                        commands
                            .iter()
                            .map(|c| format!("{}\nif %errorlevel% neq 0 exit /b %errorlevel%", c))
                            .join("\n"),
                    ))
                } else {
                    Ok(ResolvedScriptContents::Inline(commands.iter().join("\n")))
                }
            }
            ScriptContent::Command(command) => {
                Ok(ResolvedScriptContents::Inline(command.to_owned()))
            }
        };

        // render jinja if it is an inline script
        if let Some(renderer) = jinja_renderer {
            match script_content? {
                ResolvedScriptContents::Inline(script) => {
                    let rendered = renderer(&script).map_err(|e| {
                        std::io::Error::other(format!(
                            "Failed to render jinja template in build `script`: {}",
                            e
                        ))
                    })?;
                    Ok(ResolvedScriptContents::Inline(rendered))
                }
                other => Ok(other),
            }
        } else {
            script_content
        }
    }
}

/// An AsyncRead wrapper that replaces carriage return (\r) bytes with newline (\n) bytes.
pub fn normalize_crlf<R: AsyncRead + Unpin>(reader: R) -> impl AsyncRead + Unpin {
    FramedRead::new(reader, CrLfNormalizer::default())
        .into_async_read()
        .compat()
}

/// Codec that normalizes CR and CRLF to LF
#[derive(Default)]
pub struct CrLfNormalizer {
    last_was_cr: bool,
}

impl Decoder for CrLfNormalizer {
    type Item = BytesMut;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut bytes = src.split_off(0);
        let mut read_index = 0;
        let mut write_index = 0;
        while read_index < bytes.len() {
            match bytes[read_index] {
                b'\r' => {
                    bytes[write_index] = b'\n';
                    write_index += 1;
                    self.last_was_cr = true;
                }
                b'\n' if self.last_was_cr => {
                    // Skip writing the newline if the last byte was a carriage return.
                    self.last_was_cr = false
                }
                b => {
                    bytes[write_index] = b;
                    write_index += 1;
                    self.last_was_cr = false;
                }
            }
            read_index += 1;
        }

        if write_index == 0 {
            Ok(None)
        } else {
            bytes.truncate(write_index);
            Ok(Some(bytes))
        }
    }
}

use crate::interpreter::{
    BASH_PREAMBLE, BashInterpreter, CMDEXE_PREAMBLE, CmdExeInterpreter, Interpreter,
    NodeJsInterpreter, NuShellInterpreter, PerlInterpreter, PythonInterpreter, RInterpreter,
    RubyInterpreter,
};
use rattler_shell::shell;

/// Run a script with the given execution arguments and interpreter
pub async fn run_script(
    exec_args: ExecutionArgs,
    interpreter: &str,
) -> Result<(), crate::InterpreterError> {
    match interpreter {
        "nushell" | "nu" => NuShellInterpreter.run(exec_args).await?,
        "bash" => BashInterpreter.run(exec_args).await?,
        "cmd" => CmdExeInterpreter.run(exec_args).await?,
        "python" => PythonInterpreter.run(exec_args).await?,
        "perl" => PerlInterpreter.run(exec_args).await?,
        "rscript" => RInterpreter.run(exec_args).await?,
        "ruby" => RubyInterpreter.run(exec_args).await?,
        "node" | "nodejs" => NodeJsInterpreter.run(exec_args).await?,
        _ => {
            return Err(
                std::io::Error::other(format!("Unsupported interpreter: {}", interpreter)).into(),
            );
        }
    };

    Ok(())
}

/// Create build script files without executing them
pub async fn create_build_script(exec_args: ExecutionArgs) -> Result<(), std::io::Error> {
    let interpreter = if cfg!(windows) { "cmd" } else { "bash" };
    let work_dir = &exec_args.work_dir;

    if interpreter == "bash" {
        let script = BashInterpreter.get_script(&exec_args, shell::Bash).unwrap();
        let build_env_path = work_dir.join("build_env.sh");
        let build_script_path = work_dir.join("conda_build.sh");

        tokio::fs::write(&build_env_path, script).await?;

        let preamble = BASH_PREAMBLE.replace("((script_path))", &build_env_path.to_string_lossy());
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

/// Find the rattler-sandbox executable in PATH
fn find_rattler_sandbox() -> Option<PathBuf> {
    which::which("rattler-sandbox").ok()
}

/// Spawns a process and replaces the given strings in the output with the given replacements.
/// This is used to replace the host prefix with $PREFIX and the build prefix with $BUILD_PREFIX
pub async fn run_process_with_replacements(
    args: &[&str],
    cwd: &Path,
    replacements: &HashMap<String, String>,
    sandbox_config: Option<&SandboxConfiguration>,
) -> Result<std::process::Output, std::io::Error> {
    // Create or open the build log file
    let log_file_path = cwd.join("conda_build.log");
    let mut log_file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .await?;
    let mut command = if let Some(sandbox_config) = sandbox_config {
        tracing::info!("{}", sandbox_config);

        // Try to find rattler-sandbox executable
        if let Some(sandbox_exe) = find_rattler_sandbox() {
            let mut cmd = tokio::process::Command::new(sandbox_exe);

            // Add sandbox configuration arguments
            let sandbox_args = sandbox_config.with_cwd(cwd).to_args();
            cmd.args(&sandbox_args);

            // Add the actual command to execute (as positional arguments)
            cmd.arg(args[0]);
            cmd.args(&args[1..]);

            cmd
        } else {
            tracing::error!("rattler-sandbox executable not found in PATH");
            tracing::error!("Please install it by running: pixi global install rattler-sandbox");
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "rattler-sandbox executable not found. Please install it with: pixi global install rattler-sandbox",
            ));
        }
    } else {
        tokio::process::Command::new(args[0])
    };

    command
        .current_dir(cwd)
        // when using `pixi global install bash` the current work dir
        // causes some strange issues that are fixed when setting the `PWD`
        .env("PWD", cwd)
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;

    let stdout = child.stdout.take().expect("Failed to take stdout");
    let stderr = child.stderr.take().expect("Failed to take stderr");

    let stdout_wrapped = normalize_crlf(stdout);
    let stderr_wrapped = normalize_crlf(stderr);

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

                // Write to log file
                if let Err(e) = log_file.write_all(filtered_line.as_bytes()).await {
                    tracing::warn!("Failed to write to build log: {:?}", e);
                }
                if let Err(e) = log_file.write_all(b"\n").await {
                    tracing::warn!("Failed to write newline to build log: {:?}", e);
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

    // Flush and close the log file
    if let Err(e) = log_file.flush().await {
        tracing::warn!("Failed to flush build log: {:?}", e);
    }

    Ok(std::process::Output {
        status,
        stdout: stdout_log.into_bytes(),
        stderr: stderr_log.into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::bytes::BytesMut;

    #[test]
    fn test_cmd_errorlevel_injected() {
        use crate::script::{Script, ScriptContent};
        let commands = vec!["echo Hello".to_string(), "echo World".to_string()];
        let script = Script {
            content: ScriptContent::Commands(commands.clone()),
            interpreter: None,
            env: IndexMap::new(),
            secrets: Vec::new(),
            cwd: None,
            content_explicit: false,
        };

        // Use dummy paths for recipe_dir and extensions
        let recipe_dir = std::path::Path::new(".");
        let extensions = &["bat"];

        let resolved = script
            .resolve_content(
                recipe_dir,
                None::<fn(&str) -> Result<String, String>>,
                extensions,
            )
            .unwrap();

        if cfg!(windows) {
            let expected = "echo Hello\nif %errorlevel% neq 0 exit /b %errorlevel%\necho World\nif %errorlevel% neq 0 exit /b %errorlevel%";
            match resolved {
                ResolvedScriptContents::Inline(s) => assert_eq!(s, expected),
                _ => panic!("Expected Inline variant"),
            }
        } else {
            let expected = "echo Hello\necho World";
            match resolved {
                ResolvedScriptContents::Inline(s) => assert_eq!(s, expected),
                _ => panic!("Expected Inline variant"),
            }
        }
    }

    #[test]
    fn test_crlf_normalizer_no_crlf() {
        let mut normalizer = CrLfNormalizer::default();
        let mut buffer = BytesMut::from("test string with no CR or LF");

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "test string with no CR or LF");

        let eof_result = normalizer.decode_eof(&mut BytesMut::new()).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_crlf() {
        let mut normalizer = CrLfNormalizer::default();
        let mut buffer = BytesMut::from("line1\r\nline2\r\nline3");

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "line1\nline2\nline3");

        let eof_result = normalizer.decode_eof(&mut BytesMut::new()).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_cr_only() {
        let mut normalizer = CrLfNormalizer::default();
        let mut buffer = BytesMut::from("line1\rline2\rline3");

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "line1\nline2\nline3");

        let eof_result = normalizer.decode_eof(&mut BytesMut::new()).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_cr_at_end() {
        let mut normalizer = CrLfNormalizer::default();
        let mut buffer = BytesMut::from("line1\r");

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "line1\n");
        assert!(normalizer.last_was_cr);

        let eof_result = normalizer.decode_eof(&mut BytesMut::new()).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_split_crlf() {
        let mut normalizer = CrLfNormalizer::default();

        // decoder gets the \r until final part of the buffer so that it doesnt try to solve it as none
        let mut buffer1 = BytesMut::from("line1\r");
        let result1 = normalizer.decode(&mut buffer1).unwrap();
        assert!(result1.is_some());
        assert_eq!(result1.unwrap(), "line1\n");
        assert!(normalizer.last_was_cr);

        let mut buffer2 = BytesMut::from("\nline2");
        let result2 = normalizer.decode(&mut buffer2).unwrap();
        assert!(result2.is_some());
        assert_eq!(result2.unwrap(), "line2");

        let eof_result = normalizer.decode_eof(&mut BytesMut::new()).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_multiple_cr_at_end() {
        let mut normalizer = CrLfNormalizer::default();
        let mut buffer = BytesMut::from("line1\r\r\r");

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "line1\n\n\n");
        assert!(normalizer.last_was_cr);

        let eof_result = normalizer.decode_eof(&mut BytesMut::new()).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_empty_buffer() {
        let mut normalizer = CrLfNormalizer::default();
        let mut buffer = BytesMut::new();

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_none());

        let eof_result = normalizer.decode_eof(&mut buffer).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_crlf_normalizer_with_pending_cr_and_empty_buffer() {
        let mut normalizer = CrLfNormalizer { last_was_cr: true };
        let mut buffer = BytesMut::new();

        let result = normalizer.decode(&mut buffer).unwrap();
        assert!(result.is_none());

        let eof_result = normalizer.decode_eof(&mut buffer).unwrap();
        assert!(eof_result.is_none());
    }

    #[test]
    fn test_infer_interpreter_from_resolved_contents() {
        use std::path::PathBuf;

        let resolved_path =
            ResolvedScriptContents::Path(PathBuf::from("build.py"), "print('hello')".to_string());
        assert_eq!(
            resolved_path.infer_interpreter(),
            Some("python".to_string())
        );

        let resolved_inline = ResolvedScriptContents::Inline("echo 'hello'".to_string());
        assert_eq!(resolved_inline.infer_interpreter(), None);

        let resolved_missing = ResolvedScriptContents::Missing;
        assert_eq!(resolved_missing.infer_interpreter(), None);
    }
}
