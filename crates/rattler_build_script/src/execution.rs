//! Script execution types and utilities.
//!
//! This module resolves script contents, generates build scripts with
//! [`generate_build_script`], executes them with [`run_script`], and provides
//! subprocess output handling via [`run_process_with_replacements`].

use crate::sandbox::SandboxConfiguration;
use crate::script::{Script, ScriptContent};
use crate::{
    execution_context::ExecutionContext, runner::resolve_process_env, runtime::RuntimeEnv,
};
use fs_err as fs;
use futures::TryStreamExt;
use indexmap::IndexMap;
use rattler_shell::shell::Shell;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt};
use tokio_util::bytes::BytesMut;
use tokio_util::codec::{Decoder, FramedRead};
use tokio_util::compat::FuturesAsyncReadCompatExt;

/// Controls how the build subprocess environment is constructed.
///
/// This determines which host environment variables are visible to build scripts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnvironmentIsolation {
    /// Clean environment with only explicitly set build variables and a minimal
    /// passthrough whitelist (SSL certs, SSH agent, proxies). Locale is
    /// normalized to `C.UTF-8`, HOME to the work directory, and USER to
    /// `"rattler"`. Maximum reproducibility.
    #[default]
    Strict,
    /// Match conda-build behavior: forward `CFLAGS`, `CXXFLAGS`, `LDFLAGS`,
    /// `MAKEFLAGS`, `LANG`, `LC_ALL`, and `HOME` from the host. Does not
    /// normalize USER, SHELL, EDITOR, or TERM.
    CondaBuild,
    /// Inherit the entire host environment. Build variables are set on top.
    /// Least reproducible but useful for debugging.
    None,
}

impl fmt::Display for EnvironmentIsolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::CondaBuild => write!(f, "conda-build"),
            Self::None => write!(f, "none"),
        }
    }
}

impl std::str::FromStr for EnvironmentIsolation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "strict" => Ok(Self::Strict),
            "conda-build" => Ok(Self::CondaBuild),
            "none" => Ok(Self::None),
            _ => Err(format!(
                "unknown environment isolation mode '{}', expected 'strict', 'conda-build', or 'none'",
                s
            )),
        }
    }
}

/// Arguments for executing a script in a given interpreter.
#[derive(Debug)]
pub struct ExecutionArgs {
    /// The ordered sections the build wrapper is composed of. Each section runs
    /// in its own scope with its own interpreter and step-local `env`.
    pub sections: Vec<BuildScriptSection>,
    /// Environment variables to set before executing the script
    pub env_vars: IndexMap<String, String>,
    /// Secrets to set as env vars and replace in the output
    pub secrets: IndexMap<String, String>,

    /// Process and platform-aware build and host prefixes for this execution.
    pub context: ExecutionContext,

    /// The working directory (`cwd`) in which the script should execute
    pub work_dir: PathBuf,

    /// The sandbox configuration to use for the script execution
    pub sandbox_config: Option<SandboxConfiguration>,

    /// The environment isolation mode
    pub env_isolation: EnvironmentIsolation,
}

impl ExecutionArgs {
    /// Returns strings that should be replaced. The template argument can be used to specify
    /// a nice "variable" syntax, e.g. "$((var))" for bash or "%((var))%" for cmd.exe. The `var` part
    /// will be replaced with the actual variable name.
    pub(crate) fn replacements(&self, template: &str) -> HashMap<String, String> {
        let mut replacements = HashMap::new();
        replacements.insert(
            self.context.build().path().display().to_string(),
            template.replace("((var))", "BUILD_PREFIX"),
        );
        replacements.insert(
            self.context.host().path().display().to_string(),
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
    /// A list of inline commands; the interpreter assembles them at generation
    /// time (so kept as a list, not joined here).
    Commands(Vec<String>),
    /// There are no script contents
    Missing,
}

impl ResolvedScriptContents {
    /// The script contents as text (a command list is plainly newline-joined;
    /// interpreter-specific assembly happens at generation time).
    pub fn script(&self) -> Cow<'_, str> {
        match self {
            ResolvedScriptContents::Path(_, script) => Cow::Borrowed(script),
            ResolvedScriptContents::Inline(script) => Cow::Borrowed(script),
            ResolvedScriptContents::Commands(commands) => Cow::Owned(commands.join("\n")),
            ResolvedScriptContents::Missing => Cow::Borrowed(""),
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
    pub(crate) fn infer_interpreter(&self) -> Option<String> {
        self.path()
            .and_then(crate::script::determine_interpreter_from_path)
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
        context: ExecutionContext,
        jinja_renderer: Option<F>,
        sandbox_config: Option<&SandboxConfiguration>,
        env_isolation: EnvironmentIsolation,
    ) -> Result<(), crate::InterpreterError>
    where
        F: Fn(&str) -> Result<String, String>,
    {
        let env_vars = env_vars
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .chain(self.env().clone())
            .collect::<IndexMap<String, String>>();

        let contents = self.resolve_content(
            recipe_dir,
            jinja_renderer,
            crate::platform_script_extensions(),
        )?;

        let runtime = context.runtime();

        let secrets = self
            .secrets()
            .iter()
            .filter_map(|k| {
                let secret = k.to_string();

                if let Some(value) = runtime.var(&secret) {
                    Some((secret, value.to_string()))
                } else {
                    tracing::warn!("Secret {} not found in environment", secret);
                    None
                }
            })
            .collect::<IndexMap<String, String>>();

        let section_cwd = self.cwd.as_ref().map(|cwd| context.host().path().join(cwd));
        let work_dir = work_dir.to_owned();

        tracing::debug!("Running script in {}", work_dir.display());

        let exec_args = ExecutionArgs {
            sections: vec![BuildScriptSection {
                interpreter: self.interpreter.clone(),
                content: contents,
                env: IndexMap::new(),
                cwd: section_cwd,
                label: None,
            }],
            env_vars,
            secrets,
            context,
            work_dir,
            sandbox_config: sandbox_config.cloned(),
            env_isolation,
        };

        crate::execution::run_script(exec_args).await?;

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
                    match fs::read_to_string(&recipe_file) {
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
                    match fs::read_to_string(&recipe_file) {
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
                        match fs::read_to_string(&resolved_path) {
                            Err(e) => Err(e),
                            Ok(content) => Ok(ResolvedScriptContents::Path(resolved_path, content)),
                        }
                    } else {
                        Ok(ResolvedScriptContents::Inline(path.clone()))
                    }
                }
            }
            // Keep the list; the interpreter assembles it in `generate_build_script`.
            ScriptContent::Commands(commands) => {
                Ok(ResolvedScriptContents::Commands(commands.clone()))
            }
            ScriptContent::Command(command) => {
                Ok(ResolvedScriptContents::Inline(command.to_owned()))
            }
        };

        // Render jinja for inline content, each command individually; file-backed
        // scripts are not rendered.
        if let Some(renderer) = jinja_renderer {
            let render = |script: &str| -> Result<String, std::io::Error> {
                renderer(script).map_err(|e| {
                    std::io::Error::other(format!(
                        "Failed to render jinja template in build script content: {}",
                        e
                    ))
                })
            };
            match script_content? {
                ResolvedScriptContents::Inline(script) => {
                    Ok(ResolvedScriptContents::Inline(render(&script)?))
                }
                ResolvedScriptContents::Commands(commands) => {
                    let rendered = commands
                        .iter()
                        .map(|c| render(c))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(ResolvedScriptContents::Commands(rendered))
                }
                other => Ok(other),
            }
        } else {
            script_content
        }
    }
}

/// An AsyncRead wrapper that replaces carriage return (\r) bytes with newline (\n) bytes.
pub(crate) fn normalize_crlf<R: AsyncRead + Send + Unpin>(
    reader: R,
) -> impl AsyncRead + Send + Unpin {
    FramedRead::new(reader, CrLfNormalizer::default())
        .into_async_read()
        .compat()
}

/// One CRLF-normalized line of subprocess output, tagged with its stream.
#[derive(Debug)]
pub(crate) struct OutputLine {
    pub(crate) text: String,
    pub(crate) is_stderr: bool,
}

type NormalizedLines = tokio::io::Lines<tokio::io::BufReader<Box<dyn AsyncRead + Send + Unpin>>>;

/// A spawned script process streaming CRLF-normalized output lines.
pub(crate) struct SpawnedProcess {
    child: tokio::process::Child,
    stdout: NormalizedLines,
    stderr: NormalizedLines,
    stdout_closed: bool,
    stderr_closed: bool,
}

/// Spawns `program` with `args` in `cwd` with exactly `env` as the child
/// environment plus `PWD` set to `cwd`. stdin is null; stdout/stderr are piped
/// and CRLF-normalized.
pub(crate) fn spawn_process(
    program: &OsStr,
    args: &[String],
    cwd: &Path,
    env: &IndexMap<String, String>,
) -> io::Result<SpawnedProcess> {
    let mut command = tokio::process::Command::new(program);
    command
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        // when using `pixi global install bash` the current work dir
        // causes some strange issues that are fixed when setting the `PWD`
        .env("PWD", cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;
    let stdout: Box<dyn AsyncRead + Send + Unpin> = Box::new(normalize_crlf(
        child.stdout.take().expect("Failed to take stdout"),
    ));
    let stderr: Box<dyn AsyncRead + Send + Unpin> = Box::new(normalize_crlf(
        child.stderr.take().expect("Failed to take stderr"),
    ));

    Ok(SpawnedProcess {
        child,
        stdout: tokio::io::BufReader::new(stdout).lines(),
        stderr: tokio::io::BufReader::new(stderr).lines(),
        stdout_closed: false,
        stderr_closed: false,
    })
}

impl SpawnedProcess {
    /// Returns the next line from either stream, or `None` once both close.
    pub(crate) async fn next_line(&mut self) -> Option<io::Result<OutputLine>> {
        loop {
            tokio::select! {
                line = self.stdout.next_line(), if !self.stdout_closed => match line {
                    Ok(Some(text)) => return Some(Ok(OutputLine { text, is_stderr: false })),
                    Ok(None) => self.stdout_closed = true,
                    Err(error) => return Some(Err(error)),
                },
                line = self.stderr.next_line(), if !self.stderr_closed => match line {
                    Ok(Some(text)) => return Some(Ok(OutputLine { text, is_stderr: true })),
                    Ok(None) => self.stderr_closed = true,
                    Err(error) => return Some(Err(error)),
                },
                else => return None,
            }
        }
    }

    /// Discards all remaining output from both streams.
    pub(crate) async fn drain_output(&mut self) -> io::Result<()> {
        let mut stdout_sink = tokio::io::sink();
        let mut stderr_sink = tokio::io::sink();
        let (stdout_result, stderr_result) = tokio::join!(
            tokio::io::copy(self.stdout.get_mut(), &mut stdout_sink),
            tokio::io::copy(self.stderr.get_mut(), &mut stderr_sink),
        );
        self.stdout_closed = true;
        self.stderr_closed = true;
        stdout_result?;
        stderr_result?;
        Ok(())
    }

    /// Waits for the process to exit. Call after draining its output.
    pub(crate) async fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }
}

/// Codec that normalizes CR and CRLF to LF
#[derive(Default)]
pub(crate) struct CrLfNormalizer {
    pub(crate) last_was_cr: bool,
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

/// An owned build-wrapper section carried on [`ExecutionArgs::sections`].
///
/// One per build step (or one for a plain `build.script`), with its resolved
/// content, explicit interpreter, step-local `env`, optional `cwd`, and label.
/// It is borrowed into a [`ScriptSection`] during wrapper generation.
#[derive(Debug)]
pub struct BuildScriptSection {
    /// Explicit interpreter for this section, or `None` to fall back to the
    /// wrapper shell.
    pub interpreter: Option<String>,
    /// Resolved content for this section.
    pub content: ResolvedScriptContents,
    /// Environment variables scoped to this section only.
    pub env: IndexMap<String, String>,
    /// Optional working directory for this section.
    pub cwd: Option<PathBuf>,
    /// Optional annotation rendered as a boundary comment above the section.
    pub label: Option<String>,
}

/// One unit of a generated build wrapper: content run in a single interpreter,
/// with optional step-local `env`, optional `cwd`, and a boundary-comment label.
///
/// `env` is scoped to the section (see `ShellDialect::scope_section`) and is
/// distinct from [`ExecutionArgs::env_vars`], the whole-build environment. The
/// wrapper is built from one [`ScriptSection`] per [`ExecutionArgs::sections`] entry.
pub(crate) struct ScriptSection<'a> {
    /// Explicit interpreter, or `None` to infer from a file-backed path and
    /// otherwise fall back to the wrapper shell.
    pub interpreter: Option<&'a str>,
    /// Resolved content for this section.
    pub content: &'a ResolvedScriptContents,
    /// Environment variables scoped to this section only.
    pub env: &'a IndexMap<String, String>,
    /// Optional working directory for this section.
    pub cwd: Option<&'a Path>,
    /// Optional annotation rendered as a boundary comment above the section.
    pub label: Option<&'a str>,
}

/// A section's place in the wrapper. Used only for naming the interpreter script
/// file so the single-section case keeps its historic name.
#[derive(Clone, Copy)]
struct SectionIndex {
    position: usize,
    total: usize,
}

/// Names the file an interpreter section's content is written to. A sole section
/// keeps the historic `conda_build_script.<ext>` name (asserted by tests and
/// referenced by the debugging docs); multiple sections are numbered.
fn section_script_filename(extension: &str, index: SectionIndex) -> String {
    if index.total == 1 {
        format!("conda_build_script.{extension}")
    } else {
        format!("conda_build_step{}.{extension}", index.position)
    }
}

/// Returns the path to the generated native build wrapper script.
///
/// The wrapper sources the activation script, then runs the ordered
/// [`ExecutionArgs::sections`], each wrapped in an isolated scope (see
/// `scope_section`). Sections with no interpreter, or with the native wrapper
/// shell itself (`cmd` on Windows, `bash` on Unix), are appended directly to the
/// wrapper; sections with a specialized interpreter are written to script files
/// and invoked via the resolved interpreter.
pub(crate) async fn generate_build_script(
    args: &ExecutionArgs,
) -> Result<PathBuf, crate::InterpreterError> {
    let dialect = crate::shell_dialect::shell_dialect(args.context.runtime().process_platform());
    let shell = dialect.shell();

    let script_extension = shell.extension();
    let activation_script_path = args.work_dir.join(format!("build_env.{script_extension}"));
    let build_script_path = args
        .work_dir
        .join(format!("conda_build.{script_extension}"));

    let activation_script = crate::activation::activation_script(args, shell.clone())
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    tokio::fs::write(
        &activation_script_path,
        crate::shell_dialect::write_shell_script(shell.clone(), &activation_script)?,
    )
    .await?;

    let sections: Vec<ScriptSection> = args
        .sections
        .iter()
        .map(|section| ScriptSection {
            interpreter: section.interpreter.as_deref(),
            content: &section.content,
            env: &section.env,
            cwd: section.cwd.as_deref(),
            label: section.label.as_deref(),
        })
        .collect();

    let total = sections.len();
    let mut fragments = Vec::with_capacity(total);
    for (position, section) in sections.iter().enumerate() {
        let body = build_section_body(
            args,
            dialect.as_ref(),
            &shell,
            section,
            SectionIndex { position, total },
        )
        .await?;
        // Drop empty sections: an empty bash subshell `()` is a syntax error,
        // and no-script recipes stay preamble-only.
        if body.trim().is_empty() {
            continue;
        }
        fragments.push(dialect.scope_section(section.label, section.env, section.cwd, &body)?);
    }

    let build_script = format!(
        "{}\n{}",
        dialect.preamble(&activation_script_path),
        fragments.join("\n"),
    );
    tokio::fs::write(
        &build_script_path,
        crate::shell_dialect::write_shell_script(shell, &build_script)?,
    )
    .await?;

    #[cfg(unix)]
    {
        if build_script_path.extension().and_then(|e| e.to_str()) == Some("sh") {
            use std::{fs::Permissions, os::unix::fs::PermissionsExt};
            let permissions = Permissions::from_mode(0o755);
            tokio::fs::set_permissions(&build_script_path, permissions).await?;
        }
    }

    Ok(build_script_path)
}

/// Builds the raw (unscoped) wrapper body for one section: native code when no
/// interpreter applies, otherwise an invocation of the resolved interpreter.
async fn build_section_body(
    args: &ExecutionArgs,
    dialect: &dyn crate::shell_dialect::ShellDialect,
    shell: &rattler_shell::shell::ShellEnum,
    section: &ScriptSection<'_>,
    index: SectionIndex,
) -> Result<String, crate::InterpreterError> {
    // Inference runs after resolution: only file-backed scripts infer from their
    // path; inline scripts use the explicit interpreter or stay native code.
    let explicit_or_inferred = section
        .interpreter
        .map(str::to_string)
        .or_else(|| section.content.infer_interpreter());

    // No interpreter specified: default to the wrapper shell.
    let interpreter_name = explicit_or_inferred
        .clone()
        .unwrap_or_else(|| dialect.default_interpreter().to_string());
    let interpreter = crate::interpreter::SelectedInterpreter::from_recipe_name(&interpreter_name)
        .ok_or_else(|| crate::InterpreterError::UnsupportedInterpreter(interpreter_name.clone()))?;

    // Whether the content needs a specialized interpreter invocation. An
    // interpreter that matches the wrapper shell itself (`cmd` on Windows,
    // `bash` on Unix) is *not* specialized: the wrapper is already executed by
    // that shell, so its body is inlined directly rather than resolving the
    // interpreter executable from the build environment and re-invoking it.
    // This matters most for `cmd`, which is a system shell rather than a
    // conda-provided executable and would otherwise fail to resolve.
    let needs_specialized_interpreter = explicit_or_inferred
        .as_deref()
        .is_some_and(|name| name != dialect.default_interpreter());

    // Assemble the rendered content; the interpreter joins a command list.
    let script_text = match section.content {
        ResolvedScriptContents::Commands(commands) => interpreter.join_commands(commands),
        ResolvedScriptContents::Inline(script) => script.clone(),
        ResolvedScriptContents::Path(_, script) => script.clone(),
        ResolvedScriptContents::Missing => String::new(),
    };

    if !needs_specialized_interpreter {
        // No interpreter, or one that matches the wrapper shell: the content is
        // native wrapper code. Most shells can inline it directly, but cmd.exe
        // needs call indirection so `exit /b` exits only this section instead
        // of terminating the whole wrapper.
        if let Some(native_command) = dialect.native_section_script_command(
            &args
                .work_dir
                .join(section_script_filename(shell.extension(), index)),
        ) && !script_text.trim().is_empty()
        {
            let script_path = args
                .work_dir
                .join(section_script_filename(shell.extension(), index));
            tokio::fs::write(
                &script_path,
                crate::shell_dialect::write_shell_script(shell.clone(), &script_text)?,
            )
            .await?;
            let quoted = native_command
                .iter()
                .map(|arg| crate::shell_dialect::quote_arg(shell, arg))
                .collect::<Vec<_>>();
            let command_refs = quoted.iter().map(String::as_str).collect::<Vec<_>>();
            let mut body = String::new();
            shell
                .run_command(&mut body, command_refs)
                .map_err(std::io::Error::other)?;
            return Ok(body);
        }

        return Ok(script_text);
    }

    // Specialized interpreter: invoke a script file (the original path, or one
    // written next to the wrapper).
    let script_path = match section.content {
        ResolvedScriptContents::Path(path, _) => path.clone(),
        _ => {
            let path = args
                .work_dir
                .join(section_script_filename(interpreter.extension(), index));
            tokio::fs::write(&path, interpreter.script_contents(&script_text)).await?;
            path
        }
    };

    // Resolve from the activated environment (build/host prefix, then PATH).
    let executable = interpreter.resolve_executable(&args.context)?;

    // Quote so a prefix or script path with spaces survives the native shell.
    let mut command = vec![executable.to_string_lossy().into_owned()];
    command.extend(interpreter.args(&script_path));
    let quoted = command
        .iter()
        .map(|arg| crate::shell_dialect::quote_arg(shell, arg))
        .collect::<Vec<_>>();
    let command_refs = quoted.iter().map(String::as_str).collect::<Vec<_>>();
    let mut body = String::new();
    shell
        .run_command(&mut body, command_refs)
        .map_err(std::io::Error::other)?;
    Ok(body)
}

/// Runs a script with the given execution arguments.
///
/// Most callers use [`Script::run_script`], which builds the [`ExecutionArgs`]
/// from a single script. This lower-level entry point runs a pre-built
/// `ExecutionArgs` directly and is used when composing multiple sections.
pub async fn run_script(exec_args: ExecutionArgs) -> Result<(), crate::InterpreterError> {
    let dialect =
        crate::shell_dialect::shell_dialect(exec_args.context.runtime().process_platform());
    let build_script_path = generate_build_script(&exec_args).await?;
    let command_spec = dialect.command_to_run_script(&build_script_path, &exec_args.context);

    let process_env = resolve_process_env(
        exec_args.env_isolation,
        &exec_args.env_vars,
        &exec_args.secrets,
        exec_args.context.runtime(),
    );

    let output = crate::execution::run_process_with_replacements(
        &command_spec,
        &exec_args.work_dir,
        &exec_args.replacements(dialect.replacements_template()),
        &process_env,
        if dialect.supports_sandbox() {
            exec_args.sandbox_config.as_ref()
        } else {
            None
        },
        exec_args.context.runtime(),
    )
    .await?;

    if !output.status.success() {
        let status_code = output.status.code().unwrap_or(1);
        let debug_info = dialect.debug_info(&exec_args.work_dir, &exec_args.context);
        tracing::error!("Script failed with status {}", status_code);
        tracing::error!("{}", debug_info);
        return Err(crate::InterpreterError::ExecutionFailed(
            std::io::Error::other(format!(
                "Script failed with status {}{}",
                status_code, debug_info
            )),
        ));
    }

    Ok(())
}

/// Creates build script files without executing them.
pub async fn create_build_script(exec_args: ExecutionArgs) -> Result<(), std::io::Error> {
    let build_script_path = generate_build_script(&exec_args)
        .await
        .map_err(|err| match err {
            crate::InterpreterError::ExecutionFailed(err) => err,
            crate::InterpreterError::InterpreterNotFound(interpreter) => std::io::Error::other(
                format!("interpreter '{interpreter}' was not found in the build environment"),
            ),
            crate::InterpreterError::InvalidInterpreter {
                interpreter,
                reason,
            } => std::io::Error::other(format!(
                "interpreter '{interpreter}' was found but is not valid: {reason}"
            )),
            crate::InterpreterError::UnsupportedInterpreter(interpreter) => {
                let suggestion = crate::interpreter::closest_interpreter(&interpreter)
                    .map(|s| format!(". Did you mean `{s}`?"))
                    .unwrap_or_default();
                std::io::Error::other(format!(
                    "unsupported interpreter '{interpreter}'{suggestion}"
                ))
            }
        })?;

    tracing::info!("Build script created at {}", build_script_path.display());
    Ok(())
}

/// Finds the rattler-sandbox executable on the runtime `PATH`.
fn find_rattler_sandbox(runtime: &RuntimeEnv) -> Option<PathBuf> {
    which::which_in_global("rattler-sandbox", Some(runtime.path()))
        .ok()?
        .next()
}

/// Spawns a process and replaces the given strings in the output with the given replacements.
/// This is used to replace the host prefix with $PREFIX and the build prefix with $BUILD_PREFIX
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_process_with_replacements(
    command_spec: &crate::shell_dialect::CommandSpec,
    cwd: &Path,
    replacements: &HashMap<String, String>,
    process_env: &IndexMap<String, String>,
    sandbox_config: Option<&SandboxConfiguration>,
    runtime: &RuntimeEnv,
) -> Result<std::process::Output, std::io::Error> {
    // Create or open the build log file
    let log_file_path = cwd.join("conda_build.log");
    let mut log_file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .await?;
    let (program, process_args) = if let Some(sandbox_config) = sandbox_config {
        tracing::info!("{}", sandbox_config);

        // Try to find rattler-sandbox executable
        if let Some(sandbox_exe) = find_rattler_sandbox(runtime) {
            let mut sandbox_args = sandbox_config.with_cwd(cwd).to_args();

            // Add the actual command to execute as positional arguments.
            sandbox_args.push(command_spec.program.clone());
            sandbox_args.extend(command_spec.args.iter().cloned());

            (sandbox_exe.into_os_string(), sandbox_args)
        } else {
            tracing::error!("rattler-sandbox executable not found in PATH");
            tracing::error!("Please install it by running: pixi global install rattler-sandbox");
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "rattler-sandbox executable not found. Please install it with: pixi global install rattler-sandbox",
            ));
        }
    } else {
        (
            OsString::from(&command_spec.program),
            command_spec.args.clone(),
        )
    };

    let mut process = spawn_process(&program, &process_args, cwd, process_env)?;

    let mut stdout_log = String::new();
    let mut stderr_log = String::new();
    while let Some(line) = process.next_line().await {
        match line {
            Ok(line) => {
                let filtered_line = replacements
                    .iter()
                    .fold(line.text, |acc, (from, to)| acc.replace(from, to));

                if line.is_stderr {
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
            Err(e) => {
                tracing::warn!("Error reading output: {:?}", e);
                break;
            }
        }
    }

    let status = process.wait().await?;

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
    use crate::ExecutionContext;
    use rattler_conda_types::Platform;
    use tokio_util::bytes::BytesMut;

    /// `CONDA_BUILD=1` must live inside the sourced activation script so that
    /// nested shells inherit it while the outer subprocess starts without it.
    #[test]
    fn test_conda_build_marker_written_into_build_env_script() {
        use rattler_shell::shell;

        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs_err::create_dir_all(&prefix).unwrap();

        let args = ExecutionArgs {
            sections: Vec::new(),
            env_vars: IndexMap::new(),
            secrets: IndexMap::new(),
            context: ExecutionContext::shared(
                RuntimeEnv::for_test(Platform::current()),
                prefix,
                Platform::current(),
                Platform::current(),
            ),
            work_dir: tmp.path().to_path_buf(),
            sandbox_config: None,
            env_isolation: EnvironmentIsolation::None,
        };

        let script = crate::activation::activation_script(&args, shell::Bash::default()).unwrap();
        assert!(
            script.contains("CONDA_BUILD") && script.contains("1"),
            "build_env.sh must set CONDA_BUILD=1 for nested-shell re-entrancy, got:\n{script}"
        );
    }

    /// The outer subprocess must start without `CONDA_BUILD` set, otherwise
    /// the preamble skips sourcing the activation script.
    #[test]
    fn test_conda_build_not_leaked_to_subprocess_in_none_mode() {
        let env_vars = IndexMap::new();
        let secrets = IndexMap::new();

        let process_env = resolve_process_env(
            EnvironmentIsolation::None,
            &env_vars,
            &secrets,
            &RuntimeEnv::for_test(Platform::current()),
        );

        assert!(
            !process_env.contains_key("CONDA_BUILD"),
            "CONDA_BUILD must not be set on the outer subprocess"
        );
    }

    /// `resolve_content` keeps a command list as `Commands`, unjoined and
    /// without shell-specific error handling (the interpreter's job).
    #[test]
    fn test_commands_resolved_as_list() {
        use crate::script::{Script, ScriptContent};
        let commands = vec!["echo Hello".to_string(), "echo World".to_string()];
        let script = Script {
            content: ScriptContent::Commands(commands.clone()),
            interpreter: None,
            env: IndexMap::new(),
            secrets: Vec::new(),
            cwd: None,
            sandbox: None,
            content_explicit: false,
        };

        let resolved = script
            .resolve_content(
                std::path::Path::new("."),
                None::<fn(&str) -> Result<String, String>>,
                &["bat"],
            )
            .unwrap();

        match resolved {
            ResolvedScriptContents::Commands(c) => assert_eq!(c, commands),
            other => panic!("expected Commands variant, got {other:?}"),
        }
    }

    /// A command list is jinja-rendered per command in `resolve_content`.
    #[test]
    fn test_command_list_rendered_per_command() {
        use crate::script::{Script, ScriptContent};
        let script = Script {
            content: ScriptContent::Commands(vec![
                "echo MARK one".to_string(),
                "echo MARK two".to_string(),
            ]),
            ..Script::default()
        };
        let renderer = |s: &str| -> Result<String, String> { Ok(s.replace("MARK", "rendered")) };

        let resolved = script
            .resolve_content(std::path::Path::new("."), Some(renderer), &["sh"])
            .unwrap();

        match resolved {
            ResolvedScriptContents::Commands(c) => {
                assert_eq!(c, vec!["echo rendered one", "echo rendered two"]);
            }
            other => panic!("expected Commands variant, got {other:?}"),
        }
    }

    /// Unified path: a command list with no interpreter, on a Windows runtime,
    /// is assembled by `cmd` (errorlevel) into the generated `.bat`, on any host.
    #[tokio::test]
    async fn test_command_list_errorlevel_in_generated_cmd_wrapper() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = ExecutionArgs {
            sections: vec![BuildScriptSection {
                interpreter: None,
                content: ResolvedScriptContents::Commands(vec![
                    "echo Hello".to_string(),
                    "echo World".to_string(),
                ]),
                env: IndexMap::new(),
                cwd: None,
                label: None,
            }],
            env_vars: IndexMap::new(),
            secrets: IndexMap::new(),
            context: ExecutionContext::shared(
                RuntimeEnv::for_test(Platform::Win64),
                prefix,
                Platform::Win64,
                Platform::Win64,
            ),
            work_dir: tmp.path().to_path_buf(),
            sandbox_config: None,
            env_isolation: EnvironmentIsolation::None,
        };

        crate::execution::generate_build_script(&args)
            .await
            .unwrap();

        let script = fs::read_to_string(tmp.path().join("conda_build_script.bat")).unwrap();
        assert!(
            script.contains("if %errorlevel% neq 0 exit /b %errorlevel%"),
            "cmd section script must propagate errors between commands, got:\n{script}"
        );
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

    /// Regression test for <https://github.com/prefix-dev/rattler-build/issues/2199>
    ///
    /// Extension order in `resolve_content` determines which file is picked
    /// when both `.sh` and `.bat` exist. `platform_script_extensions()` must
    /// select the platform-appropriate one.
    #[test]
    fn test_script_extension_resolution_respects_order() {
        use std::path::PathBuf;

        use crate::script::{Script, ScriptContent};

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("test-script.sh"), "#!/bin/bash\necho hello").unwrap();
        fs::write(dir.path().join("test-script.bat"), "@echo off\necho hello").unwrap();

        let resolve = |content: ScriptContent, exts: &[&str]| -> PathBuf {
            let script = Script {
                content,
                ..Script::default()
            };
            match script
                .resolve_content(dir.path(), None::<fn(&str) -> Result<String, String>>, exts)
                .unwrap()
            {
                ResolvedScriptContents::Path(path, _) => path,
                other => panic!("expected Path variant, got {:?}", other),
            }
        };

        // Extension list order controls which file wins
        let path_content = || ScriptContent::Path(PathBuf::from("test-script"));
        assert_eq!(
            resolve(path_content(), &["sh", "bat"]).extension().unwrap(),
            "sh"
        );
        assert_eq!(
            resolve(path_content(), &["bat", "sh"]).extension().unwrap(),
            "bat"
        );

        // CommandOrPath variant behaves the same
        let cop_content = || ScriptContent::CommandOrPath("test-script".into());
        assert_eq!(resolve(cop_content(), &["sh"]).extension().unwrap(), "sh");
        assert_eq!(resolve(cop_content(), &["bat"]).extension().unwrap(), "bat");

        // platform_script_extensions() picks the right one for the current platform
        let ext = resolve(path_content(), crate::platform_script_extensions())
            .extension()
            .unwrap()
            .to_owned();
        assert_eq!(ext, if cfg!(windows) { "bat" } else { "sh" });
    }

    use rattler_shell::activation::prefix_path_entries;

    /// Mirrors the interpreter-module test helper: places a 0-byte executable in
    /// the prefix's bin directory and returns its path.
    fn create_fake_executable(prefix: &Path, name: &str) -> PathBuf {
        let exe_name = format!("{}{}", name, std::env::consts::EXE_SUFFIX);
        let bin_dir = prefix_path_entries(prefix, &Platform::current())
            .into_iter()
            .next()
            .expect("prefix has executable path entries");
        fs::create_dir_all(&bin_dir).unwrap();
        let exe = bin_dir.join(exe_name);
        fs::write(&exe, "").unwrap();
        #[cfg(unix)]
        {
            use std::{fs::Permissions, os::unix::fs::PermissionsExt};
            fs::set_permissions(&exe, Permissions::from_mode(0o755)).unwrap();
        }
        exe
    }

    fn execution_args(
        work_dir: PathBuf,
        run_prefix: PathBuf,
        script: ResolvedScriptContents,
        interpreter: Option<&str>,
    ) -> ExecutionArgs {
        ExecutionArgs {
            sections: vec![BuildScriptSection {
                interpreter: interpreter.map(str::to_string),
                content: script,
                env: IndexMap::new(),
                cwd: None,
                label: None,
            }],
            env_vars: IndexMap::new(),
            secrets: IndexMap::new(),
            context: ExecutionContext::shared(
                RuntimeEnv::current(),
                run_prefix,
                Platform::current(),
                Platform::current(),
            ),
            work_dir,
            sandbox_config: None,
            env_isolation: EnvironmentIsolation::None,
        }
    }

    /// In Strict mode the subprocess env is cleared, only the passthrough
    /// whitelist is forwarded from the host, and explicit env_vars + secrets are
    /// applied on top. The host vars are injected through `RuntimeEnv`, so the
    /// test does not touch the real process environment.
    #[test]
    fn test_strict_env_clear_and_passthrough_whitelist() {
        let runtime = RuntimeEnv::for_test(Platform::current())
            .with_var("RB_TEST_RANDOM_VAR", "should-not-leak")
            .with_var("SSL_CERT_FILE", "/host/cacert.pem");

        let mut env_vars = IndexMap::new();
        env_vars.insert("EXPLICIT_VAR".to_string(), "explicit".to_string());
        let mut secrets = IndexMap::new();
        secrets.insert("SECRET_VAR".to_string(), "secret".to_string());

        let resolve = |isolation: EnvironmentIsolation| {
            resolve_process_env(isolation, &env_vars, &secrets, &runtime)
        };

        let strict = resolve(EnvironmentIsolation::Strict);
        assert!(
            !strict.contains_key("RB_TEST_RANDOM_VAR"),
            "non-whitelisted host var must be absent in Strict mode"
        );
        assert_eq!(
            strict.get("SSL_CERT_FILE").map(String::as_str),
            Some("/host/cacert.pem"),
            "whitelisted host var must be passed through"
        );
        assert_eq!(
            strict.get("EXPLICIT_VAR").map(String::as_str),
            Some("explicit")
        );
        assert_eq!(strict.get("SECRET_VAR").map(String::as_str), Some("secret"));

        // CondaBuild also clears the env and applies the same whitelist.
        let conda_build = resolve(EnvironmentIsolation::CondaBuild);
        assert!(
            !conda_build.contains_key("RB_TEST_RANDOM_VAR"),
            "non-whitelisted host var must be absent in CondaBuild mode"
        );
        assert_eq!(
            conda_build.get("SSL_CERT_FILE").map(String::as_str),
            Some("/host/cacert.pem")
        );
    }

    #[test]
    fn test_none_env_uses_injected_runtime_with_explicit_overrides() {
        let runtime = RuntimeEnv::for_test(Platform::Linux64)
            .with_var("RUNTIME_ONLY", "runtime")
            .with_var("OVERLAY", "runtime");
        let mut env_vars = IndexMap::new();
        env_vars.insert("OVERLAY".to_string(), "explicit".to_string());
        env_vars.insert("EXPLICIT_ONLY".to_string(), "explicit".to_string());
        let mut secrets = IndexMap::new();
        secrets.insert("SECRET_ONLY".to_string(), "secret".to_string());

        let process_env =
            resolve_process_env(EnvironmentIsolation::None, &env_vars, &secrets, &runtime);

        assert_eq!(
            process_env.get("RUNTIME_ONLY").map(String::as_str),
            Some("runtime")
        );
        assert_eq!(
            process_env.get("OVERLAY").map(String::as_str),
            Some("explicit")
        );
        assert_eq!(
            process_env.get("EXPLICIT_ONLY").map(String::as_str),
            Some("explicit")
        );
        assert!(
            !process_env.contains_key("SECRET_ONLY"),
            "None mode relies on secrets captured in RuntimeEnv"
        );
    }

    #[test]
    fn test_platform_passthrough_follows_runtime_platform() {
        let env_vars = IndexMap::new();
        let secrets = IndexMap::new();
        let runtime = RuntimeEnv::for_test(Platform::Win64).with_var("SYSTEMROOT", "C:\\Windows");

        let windows_env =
            resolve_process_env(EnvironmentIsolation::Strict, &env_vars, &secrets, &runtime);
        assert_eq!(
            windows_env.get("SYSTEMROOT").map(String::as_str),
            Some("C:\\Windows")
        );

        let linux_env = resolve_process_env(
            EnvironmentIsolation::Strict,
            &env_vars,
            &secrets,
            &runtime.with_process_platform(Platform::Linux64),
        );
        assert!(!linux_env.contains_key("SYSTEMROOT"));
    }

    #[cfg(windows)]
    fn command_that_writes_stdout_and_stderr() -> (OsString, Vec<String>) {
        (
            OsString::from("cmd"),
            vec![
                "/d".to_string(),
                "/c".to_string(),
                "echo stdout & echo stderr 1>&2".to_string(),
            ],
        )
    }

    #[cfg(not(windows))]
    fn command_that_writes_stdout_and_stderr() -> (OsString, Vec<String>) {
        (
            OsString::from("sh"),
            vec![
                "-c".to_string(),
                "printf 'stdout\\r\\n'; printf 'stderr\\r\\n' >&2".to_string(),
            ],
        )
    }

    #[cfg(windows)]
    fn command_that_lists_environment() -> (OsString, Vec<String>) {
        (
            OsString::from("cmd"),
            vec!["/d".to_string(), "/c".to_string(), "set".to_string()],
        )
    }

    #[cfg(not(windows))]
    fn command_that_lists_environment() -> (OsString, Vec<String>) {
        (OsString::from("env"), Vec::new())
    }

    #[tokio::test]
    async fn test_spawn_process_streams_normalized_tagged_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let env = IndexMap::new();
        let (program, args) = command_that_writes_stdout_and_stderr();
        let mut process = spawn_process(&program, &args, tmp.path(), &env).unwrap();
        let mut lines = Vec::new();

        while let Some(line) = process.next_line().await {
            let line = line.unwrap();
            lines.push((line.text, line.is_stderr));
        }

        assert!(process.wait().await.unwrap().success());
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().any(|(text, is_stderr)| {
            text.trim() == "stdout" && !*is_stderr && !text.contains('\r')
        }));
        assert!(lines.iter().any(|(text, is_stderr)| {
            text.trim() == "stderr" && *is_stderr && !text.contains('\r')
        }));
    }

    #[tokio::test]
    async fn test_spawn_process_applies_exact_environment() {
        assert!(
            std::env::var_os("PATH").is_some(),
            "the test requires PATH in the parent environment"
        );

        let tmp = tempfile::tempdir().unwrap();
        let mut env = IndexMap::new();
        env.insert("RB_SPAWN_PROCESS_ENV".to_string(), "passed".to_string());
        let (program, args) = command_that_lists_environment();
        let mut process = spawn_process(&program, &args, tmp.path(), &env).unwrap();
        let mut lines = Vec::new();

        while let Some(line) = process.next_line().await {
            lines.push(line.unwrap().text);
        }

        assert!(process.wait().await.unwrap().success());
        assert!(
            lines
                .iter()
                .any(|line| line == "RB_SPAWN_PROCESS_ENV=passed"),
            "explicit environment variable must be passed to the child"
        );
        assert!(
            lines
                .iter()
                .any(|line| { line == &format!("PWD={}", tmp.path().to_string_lossy()) }),
            "the child PWD must be the requested working directory"
        );
        assert!(
            !lines.iter().any(|line| {
                line.split_once('=')
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            }),
            "parent PATH must not leak into the child"
        );
    }

    #[tokio::test]
    async fn test_run_process_with_replacements_keeps_output_handling_host_side() {
        let tmp = tempfile::tempdir().unwrap();
        let process_env = IndexMap::new();
        let mut replacements = HashMap::new();
        replacements.insert("RAW_PREFIX".to_string(), "$PREFIX".to_string());
        replacements.insert("raw-secret".to_string(), "***".to_string());

        #[cfg(windows)]
        let command = [
            "cmd".to_string(),
            "/d".to_string(),
            "/c".to_string(),
            "echo RAW_PREFIX raw-secret & echo RAW_PREFIX raw-secret 1>&2".to_string(),
        ];
        #[cfg(not(windows))]
        let command = [
            "sh".to_string(),
            "-c".to_string(),
            "printf 'RAW_PREFIX raw-secret\\n'; printf 'RAW_PREFIX raw-secret\\n' >&2".to_string(),
        ];
        let command_spec = crate::shell_dialect::CommandSpec {
            program: command[0].clone(),
            args: command[1..].to_vec(),
        };

        let output = run_process_with_replacements(
            &command_spec,
            tmp.path(),
            &replacements,
            &process_env,
            None,
            &RuntimeEnv::for_test(Platform::current()),
        )
        .await
        .unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        let log = fs::read_to_string(tmp.path().join("conda_build.log")).unwrap();
        for content in [&stdout, &stderr, &log] {
            assert!(content.contains("$PREFIX ***"), "got:\n{content}");
            assert!(!content.contains("RAW_PREFIX"), "got:\n{content}");
            assert!(!content.contains("raw-secret"), "got:\n{content}");
        }
    }

    /// The PowerShell prologue is written verbatim into the generated script
    /// file for an inline body.
    #[tokio::test]
    async fn test_powershell_prologue_written_into_script_file() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        // The fake pwsh is 0 bytes; `is_pwsh_new_enough` will fail to parse a
        // version and only warn, so resolution still succeeds.
        create_fake_executable(&prefix, "pwsh");

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("Write-Output 'hi'".to_string()),
            Some("powershell"),
        );

        generate_build_script(&args).await.unwrap();

        let script_file = tmp.path().join("conda_build_script.ps1");
        let contents = fs::read_to_string(&script_file).unwrap();
        assert!(
            contents.contains("$ErrorActionPreference = 'Stop'"),
            "missing ErrorActionPreference, got:\n{contents}"
        );
        assert!(
            contents.contains("$PSNativeCommandUseErrorActionPreference"),
            "missing PSNativeCommandUseErrorActionPreference, got:\n{contents}"
        );
        assert!(
            contents.contains("Write-Output 'hi'"),
            "user body must be appended after the prologue"
        );
    }

    /// `create_build_script` maps `InterpreterNotFound` to an io::Error whose
    /// message mentions the build environment. `brush` is build-prefix-only, so
    /// it errors when absent (unlike `python`, which falls back to `PATH`).
    #[tokio::test]
    async fn test_create_build_script_missing_interpreter_error() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("echo hi".to_string()),
            Some("brush"),
        );

        let err = create_build_script(args).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("was not found in the build environment"),
            "unexpected error: {err}"
        );
    }

    /// An unsupported interpreter name surfaces as an `unsupported interpreter`
    /// io::Error, with a "did you mean" suggestion for near-misses only.
    #[tokio::test]
    async fn test_create_build_script_unsupported_interpreter_error() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let unsupported_error = |interpreter: &str| {
            let args = execution_args(
                tmp.path().to_path_buf(),
                tmp.path().join("prefix"),
                ResolvedScriptContents::Inline("noop".to_string()),
                Some(interpreter),
            );
            async { create_build_script(args).await.unwrap_err().to_string() }
        };

        let message = unsupported_error("not-a-real-interp").await;
        assert!(
            message.contains("unsupported interpreter 'not-a-real-interp'"),
            "unexpected error: {message}"
        );
        assert!(
            !message.contains("Did you mean"),
            "no suggestion expected for an unrelated name: {message}"
        );

        let message = unsupported_error("brus").await;
        assert!(
            message.contains("Did you mean `brush`?"),
            "unexpected error: {message}"
        );
    }

    /// A typo in the recipe `interpreter` (issue #2530) surfaces as
    /// `UnsupportedInterpreter` instead of a generic execution failure.
    #[tokio::test]
    async fn test_generate_build_script_interpreter_typo_error() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();

        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("echo \"Hello from brush!\"".to_string()),
            Some("brus"),
        );

        let err = generate_build_script(&args).await.unwrap_err();
        assert!(
            matches!(err, crate::InterpreterError::UnsupportedInterpreter(ref name) if name == "brus"),
            "expected UnsupportedInterpreter, got {err:?}"
        );
    }

    /// `EnvironmentIsolation` round-trips between `FromStr` and `Display`, and an
    /// unknown value errors with the documented message.
    #[test]
    fn test_environment_isolation_round_trip() {
        use std::str::FromStr;

        for (text, value) in [
            ("strict", EnvironmentIsolation::Strict),
            ("conda-build", EnvironmentIsolation::CondaBuild),
            ("none", EnvironmentIsolation::None),
        ] {
            assert_eq!(EnvironmentIsolation::from_str(text).unwrap(), value);
            assert_eq!(value.to_string(), text);
        }

        let err = EnvironmentIsolation::from_str("bogus").unwrap_err();
        assert!(
            err.contains("unknown environment isolation mode 'bogus'"),
            "unexpected error: {err}"
        );
    }

    fn bash_wrapper_body(wrapper: &str) -> String {
        let normalized = wrapper.replace("\r\n", "\n");
        let body = normalized
            .split_once("## End of preamble")
            .map(|(_, body)| body.trim_start())
            .unwrap_or_else(|| panic!("missing bash preamble marker:\n{wrapper}"));

        // The command-tracing prologue is part of the native wrapper preamble,
        // even though it is intentionally emitted after activation.
        body.strip_prefix(
            "# Trace each command as it runs so a failing line is visible (see #2264).\n\
             # Placed after activation so the sourced environment setup is not traced.\n\
             set -x\n",
        )
        .unwrap_or(body)
        .trim()
        .to_string()
    }

    fn cmd_wrapper_body(wrapper: &str, work_dir: &Path) -> String {
        let normalized = wrapper.replace("\r\n", "\n").replace('\\', "/");
        let normalized = if let Ok(command_processor) = std::env::var("COMSPEC") {
            normalized.replace(&command_processor.replace('\\', "/"), "cmd.exe")
        } else {
            normalized
        };
        let work_dir = work_dir.to_string_lossy().replace('\\', "/");
        let normalized = normalized.replace(&work_dir, "$WORK_DIR");
        let start = normalized
            .find("@rem ===")
            .or_else(|| normalized.find("setlocal"))
            .unwrap_or_else(|| panic!("missing cmd section body:\n{wrapper}"));
        normalized[start..].trim().to_string()
    }

    /// The single section is subshell-wrapped on bash (uniform isolation).
    #[tokio::test]
    async fn test_bash_single_section_wrapped_in_subshell() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("echo hi".to_string()),
            None,
        );
        let args = ExecutionArgs {
            context: args
                .context
                .with_runtime(RuntimeEnv::for_test(Platform::Linux64)),
            ..args
        };
        generate_build_script(&args).await.unwrap();
        let wrapper = fs::read_to_string(tmp.path().join("conda_build.sh")).unwrap();
        insta::assert_snapshot!(bash_wrapper_body(&wrapper), @r###"
(
echo hi
)
"###);
    }

    /// A recipe with no build script stays preamble-only: no empty subshell.
    #[tokio::test]
    async fn test_bash_missing_script_is_preamble_only() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Missing,
            None,
        );
        let args = ExecutionArgs {
            context: args
                .context
                .with_runtime(RuntimeEnv::for_test(Platform::Linux64)),
            ..args
        };
        generate_build_script(&args).await.unwrap();
        let wrapper = fs::read_to_string(tmp.path().join("conda_build.sh")).unwrap();
        insta::assert_snapshot!(bash_wrapper_body(&wrapper), @"");
    }

    /// The single section is `setlocal`/`endlocal`-scoped on cmd, `pushd` /
    /// `popd`-scoped for cwd, with a trailing errorlevel guard.
    #[tokio::test]
    async fn test_cmd_single_section_setlocal_and_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let args = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Inline("echo hi".to_string()),
            None,
        );
        let args = ExecutionArgs {
            context: args
                .context
                .with_runtime(RuntimeEnv::for_test(Platform::Win64)),
            ..args
        };
        generate_build_script(&args).await.unwrap();
        let wrapper = fs::read_to_string(tmp.path().join("conda_build.bat")).unwrap();
        insta::assert_snapshot!(cmd_wrapper_body(&wrapper, tmp.path()), @r###"
setlocal
pushd "." || exit /b 1
@cmd.exe /d /c call $WORK_DIR/conda_build_script.bat
set "RB_SECTION_ERRORLEVEL=%errorlevel%"
popd
if %RB_SECTION_ERRORLEVEL% equ 0 if %errorlevel% neq 0 set "RB_SECTION_ERRORLEVEL=%errorlevel%"
endlocal & if %RB_SECTION_ERRORLEVEL% neq 0 exit /b %RB_SECTION_ERRORLEVEL%
"###);
    }

    /// Native cmd sections are invoked through a nested `cmd /c call` so `exit /b`
    /// and bare `exit` inside a section script return to the wrapper instead of
    /// skipping later steps.
    #[tokio::test]
    async fn test_cmd_sections_use_call_indirection() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let base = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Missing,
            None,
        );

        let args = ExecutionArgs {
            context: ExecutionContext::shared(
                RuntimeEnv::for_test(Platform::Win64),
                base.context.host().path(),
                Platform::Win64,
                Platform::Win64,
            ),
            sections: vec![
                BuildScriptSection {
                    interpreter: None,
                    content: ResolvedScriptContents::Inline("echo one\nexit /b 0".to_string()),
                    env: IndexMap::new(),
                    cwd: None,
                    label: Some("step 0".to_string()),
                },
                BuildScriptSection {
                    interpreter: None,
                    content: ResolvedScriptContents::Inline("echo two".to_string()),
                    env: IndexMap::new(),
                    cwd: None,
                    label: Some("step 1".to_string()),
                },
            ],
            ..base
        };

        generate_build_script(&args).await.unwrap();
        let wrapper = fs::read_to_string(tmp.path().join("conda_build.bat")).unwrap();
        insta::assert_snapshot!(cmd_wrapper_body(&wrapper, tmp.path()), @r###"
@rem === step 0 ===
setlocal
pushd "." || exit /b 1
@cmd.exe /d /c call $WORK_DIR/conda_build_step0.bat
set "RB_SECTION_ERRORLEVEL=%errorlevel%"
popd
if %RB_SECTION_ERRORLEVEL% equ 0 if %errorlevel% neq 0 set "RB_SECTION_ERRORLEVEL=%errorlevel%"
endlocal & if %RB_SECTION_ERRORLEVEL% neq 0 exit /b %RB_SECTION_ERRORLEVEL%
@rem === step 1 ===
setlocal
pushd "." || exit /b 1
@cmd.exe /d /c call $WORK_DIR/conda_build_step1.bat
set "RB_SECTION_ERRORLEVEL=%errorlevel%"
popd
if %RB_SECTION_ERRORLEVEL% equ 0 if %errorlevel% neq 0 set "RB_SECTION_ERRORLEVEL=%errorlevel%"
endlocal & if %RB_SECTION_ERRORLEVEL% neq 0 exit /b %RB_SECTION_ERRORLEVEL%
"###);
    }

    #[tokio::test]
    async fn test_cmd_call_indirection_escapes_percent_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let work_dir = tmp.path().join("%NO_SUCH_VAR%");
        let prefix = work_dir.join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let args = execution_args(
            work_dir.clone(),
            prefix,
            ResolvedScriptContents::Inline("echo hi".to_string()),
            None,
        );
        let args = ExecutionArgs {
            context: ExecutionContext::shared(
                RuntimeEnv::for_test(Platform::Win64),
                args.context.host().path(),
                Platform::Win64,
                Platform::Win64,
            ),
            ..args
        };

        generate_build_script(&args).await.unwrap();
        let wrapper = fs::read_to_string(work_dir.join("conda_build.bat")).unwrap();

        assert!(
            wrapper.contains("cmd.exe /d /c call"),
            "native cmd sections should use nested cmd call indirection:\n{wrapper}"
        );
        assert!(
            wrapper.contains("%%NO_SUCH_VAR%%"),
            "percent signs in call paths must be escaped for the outer batch context:\n{wrapper}"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_cmd_bare_exit_does_not_skip_later_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let base = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Missing,
            None,
        );

        let args = ExecutionArgs {
            context: ExecutionContext::shared(
                RuntimeEnv::for_test(Platform::Win64),
                base.context.host().path(),
                Platform::Win64,
                Platform::Win64,
            ),
            sections: vec![
                BuildScriptSection {
                    interpreter: None,
                    content: ResolvedScriptContents::Inline("echo one\nexit 0".to_string()),
                    env: IndexMap::new(),
                    cwd: None,
                    label: Some("step 0".to_string()),
                },
                BuildScriptSection {
                    interpreter: None,
                    content: ResolvedScriptContents::Inline("echo two> marker.txt".to_string()),
                    env: IndexMap::new(),
                    cwd: None,
                    label: Some("step 1".to_string()),
                },
            ],
            ..base
        };

        run_script(args).await.unwrap();

        assert!(
            tmp.path().join("marker.txt").exists(),
            "bare `exit 0` in the first cmd section must not skip the second section"
        );
    }

    /// Multiple sections compose in order, each scoped, with labels and env.
    #[tokio::test]
    async fn test_multiple_sections_composed_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        fs::create_dir_all(&prefix).unwrap();
        let base = execution_args(
            tmp.path().to_path_buf(),
            prefix,
            ResolvedScriptContents::Missing,
            None,
        );

        let mut env0 = IndexMap::new();
        env0.insert("FOO".to_string(), "bar".to_string());

        let args = ExecutionArgs {
            context: ExecutionContext::shared(
                RuntimeEnv::for_test(Platform::Linux64),
                base.context.host().path(),
                Platform::Linux64,
                Platform::Linux64,
            ),
            sections: vec![
                BuildScriptSection {
                    interpreter: None,
                    content: ResolvedScriptContents::Inline("echo one".to_string()),
                    env: env0,
                    cwd: Some(PathBuf::from("/tmp/step0")),
                    label: Some("step 0".to_string()),
                },
                BuildScriptSection {
                    interpreter: None,
                    content: ResolvedScriptContents::Inline("echo two".to_string()),
                    env: IndexMap::new(),
                    cwd: None,
                    label: Some("step 1".to_string()),
                },
            ],
            ..base
        };

        generate_build_script(&args).await.unwrap();
        let wrapper = fs::read_to_string(tmp.path().join("conda_build.sh")).unwrap();

        insta::assert_snapshot!(bash_wrapper_body(&wrapper), @r###"
# === step 0 ===
(
export FOO=bar
cd /tmp/step0
echo one
)
# === step 1 ===
(
echo two
)
"###);
    }

    /// A sole section keeps the historic file name; multiple are numbered.
    #[test]
    fn test_section_script_filename_single_vs_multi() {
        let single = SectionIndex {
            position: 0,
            total: 1,
        };
        assert_eq!(
            section_script_filename("py", single),
            "conda_build_script.py"
        );
        let first = SectionIndex {
            position: 0,
            total: 2,
        };
        let second = SectionIndex {
            position: 1,
            total: 2,
        };
        assert_eq!(section_script_filename("py", first), "conda_build_step0.py");
        assert_eq!(
            section_script_filename("py", second),
            "conda_build_step1.py"
        );
    }
}
