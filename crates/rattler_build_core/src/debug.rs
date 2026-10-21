//! Cross-platform debug utilities for rattler-build.
//!
//! This module provides the core logic for setting up, inspecting, and running
//! debug build environments. It is used by both the CLI (`rattler-build debug`)
//! and the Python bindings (`DebugSession`).

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use rattler_build_script::{
    EnvironmentIsolation,
    runner::{
        ExecResultExt, ExecSpec, ExecStatus, GuestPath, Mounts, OutputSink, OutputStream, Runner,
        RunnerError, SessionSpec,
    },
};
use rattler_conda_types::{ChannelUrl, MatchSpec, ParseStrictness, PrefixRecord};

use crate::render::resolved_dependencies::RunExportsDownload;
use crate::source::patch::apply_patch_custom;
use crate::tool_configuration::Configuration;

/// Result of running a debug build script.
#[derive(Debug)]
pub struct DebugRunResult {
    /// The exit code of the build script process.
    pub exit_code: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

/// Parse the directories JSON from environment, rattler-build-log.txt, or work directory.
///
/// Resolution order:
/// 1. If `work_dir` is explicitly given, use it directly.
/// 2. If `RATTLER_BUILD_DIRECTORIES` env var is set (inside a debug shell), parse it.
/// 3. Fall back to reading `rattler-build-log.txt` from the output directory.
pub fn parse_directories_info(
    work_dir: Option<PathBuf>,
    output_dir: &Path,
) -> std::io::Result<(PathBuf, Option<serde_json::Value>)> {
    if let Some(dir) = work_dir {
        return Ok((dir, None));
    }

    // Check if we're inside a debug shell with RATTLER_BUILD_DIRECTORIES set
    if let Ok(json_str) = std::env::var("RATTLER_BUILD_DIRECTORIES")
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        let work_dir = json["work_dir"].as_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "work_dir not found in RATTLER_BUILD_DIRECTORIES",
            )
        })?;
        return Ok((PathBuf::from(work_dir), Some(json)));
    }

    // Read from rattler-build-log.txt
    let log_file = output_dir.join("rattler-build-log.txt");
    if !log_file.exists() {
        eprintln!(
            "Error: Could not find rattler-build-log.txt at {}",
            log_file.display()
        );
        eprintln!("Hint: Run from inside a `rattler-build debug` shell, or specify --work-dir.");
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "rattler-build-log.txt not found",
        ));
    }

    let content = fs_err::read_to_string(&log_file)?;
    let last_line = content.lines().last().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "rattler-build-log.txt is empty",
        )
    })?;

    // Try to parse as JSON, fall back to plain path for backwards compatibility
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(last_line) {
        let work_dir = json["work_dir"].as_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "work_dir not found in JSON",
            )
        })?;
        Ok((PathBuf::from(work_dir), Some(json)))
    } else {
        // Old format: plain path
        Ok((PathBuf::from(last_line.trim()), None))
    }
}

/// Build Unix shell environment variable exports from directories JSON.
pub fn build_env_exports(json: &serde_json::Value) -> String {
    let mut env_exports = String::new();

    // Export the full JSON as RATTLER_BUILD_DIRECTORIES
    env_exports.push_str(&format!(
        "export RATTLER_BUILD_DIRECTORIES='{}'\n",
        serde_json::to_string(json).unwrap_or_default()
    ));

    // Export individual directories for convenience
    for (key, env_var) in &[
        ("recipe_path", "RATTLER_BUILD_RECIPE_PATH"),
        ("recipe_dir", "RATTLER_BUILD_RECIPE_DIR"),
        ("build_dir", "RATTLER_BUILD_BUILD_DIR"),
        ("output_dir", "RATTLER_BUILD_OUTPUT_DIR"),
        ("host_prefix", "RATTLER_BUILD_HOST_PREFIX"),
        ("build_prefix", "RATTLER_BUILD_BUILD_PREFIX"),
    ] {
        if let Some(val) = json[*key].as_str() {
            env_exports.push_str(&format!("export {}='{}'\n", env_var, val));
        }
    }

    // Export the path to the current rattler-build binary so that
    // subcommands like `$RATTLER_BUILD debug host-add` work even when
    // the system PATH points to a different (older) installation.
    if let Ok(exe) = std::env::current_exe()
        && let Ok(canonical) = exe.canonicalize()
    {
        env_exports.push_str(&format!(
            "export RATTLER_BUILD='{}'\nalias rattler-build='{}'\n",
            canonical.display(),
            canonical.display()
        ));
    }

    env_exports
}

/// Build Windows cmd.exe `set` commands from directories JSON.
#[cfg(windows)]
pub fn build_env_sets_windows(json: &serde_json::Value) -> String {
    let mut sets = String::new();

    // Set the full JSON as RATTLER_BUILD_DIRECTORIES
    sets.push_str(&format!(
        "set \"RATTLER_BUILD_DIRECTORIES={}\"&",
        serde_json::to_string(json).unwrap_or_default()
    ));

    for (key, env_var) in &[
        ("recipe_path", "RATTLER_BUILD_RECIPE_PATH"),
        ("recipe_dir", "RATTLER_BUILD_RECIPE_DIR"),
        ("build_dir", "RATTLER_BUILD_BUILD_DIR"),
        ("output_dir", "RATTLER_BUILD_OUTPUT_DIR"),
        ("host_prefix", "RATTLER_BUILD_HOST_PREFIX"),
        ("build_prefix", "RATTLER_BUILD_BUILD_PREFIX"),
    ] {
        if let Some(val) = json[*key].as_str() {
            sets.push_str(&format!("set \"{}={}\"&", env_var, val));
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Ok(canonical) = exe.canonicalize()
    {
        sets.push_str(&format!("set \"RATTLER_BUILD={}\"&", canonical.display()));
    }

    sets
}

/// Print the debug shell welcome banner.
pub fn print_debug_banner(work_dir: &Path, directories_json: &Option<serde_json::Value>) {
    println!();
    println!("  rattler-build debug shell");
    println!("  ========================");
    println!();
    println!("  Work directory: {}", work_dir.display());

    if let Some(json) = &directories_json {
        if let Some(host_prefix) = json["host_prefix"].as_str() {
            println!("  Host prefix:    {}", host_prefix);
        }
        if let Some(build_prefix) = json["build_prefix"].as_str() {
            println!("  Build prefix:   {}", build_prefix);
        }
    }

    println!();
    println!("  Available commands:");
    println!("    rattler-build debug create-patch         Create a patch from your changes");
    println!("    rattler-build debug host-add <pkg>       Add packages to host env");
    println!("    rattler-build debug build-add <pkg>      Add packages to build env");
    println!();
    println!(
        "  The build environment has been sourced. Run `{}` to",
        build_script_hint()
    );
    println!("  execute the build script, or make changes and use create-patch.");
    println!();
    println!("  Exit with 'exit' or Ctrl+D.");
    println!();
}

/// Open an interactive debug shell in the build environment.
///
/// On Unix, this starts `$SHELL` with `build_env.sh` sourced.
/// On Windows, this starts `cmd.exe` with `build_env.bat` called.
/// The shell runs with the terminal's stdio in a session of `runner` that
/// mirrors the build directory and keeps the user's own environment.
pub async fn open_debug_shell(
    runner: &dyn Runner,
    work_dir: PathBuf,
    directories_json: Option<serde_json::Value>,
) -> std::io::Result<()> {
    validate_work_dir(&work_dir)?;
    print_debug_banner(&work_dir, &directories_json);
    let exec = open_debug_shell_exec(&work_dir, directories_json);
    let mut session = runner
        .start_session(debug_session_spec(runner, &work_dir)?)
        .await
        .map_err(io_error)?;
    let status = session
        .exec_interactive(exec)
        .await
        .into_status()
        .map_err(io_error)?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "shell exited with status: {}",
            describe_exit_status(status)
        )));
    }
    Ok(())
}

/// Run the build script and capture its output.
///
/// Returns a [`DebugRunResult`] with captured stdout and stderr.
/// On Unix, runs via `bash`. On Windows, runs via `cmd.exe`.
/// The script runs in a session of `runner` that mirrors the build directory
/// and keeps the user's own environment; an exit without a code reports 1.
pub async fn run_build_script(
    runner: &dyn Runner,
    work_dir: &Path,
    trace: bool,
) -> std::io::Result<DebugRunResult> {
    validate_work_dir(work_dir)?;
    let exec = build_script_exec(work_dir, trace)?;
    let mut session = runner
        .start_session(debug_session_spec(runner, work_dir)?)
        .await
        .map_err(io_error)?;
    let mut output = CapturedOutput::default();
    let status = session
        .exec(exec, &mut output)
        .await
        .into_status()
        .map_err(io_error)?;

    Ok(DebugRunResult {
        exit_code: status.code.unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// Run the build script with inherited stdio (for interactive CLI use).
///
/// Returns just the exit code. On Unix, runs via `bash`. On Windows, runs
/// via `cmd.exe`. The script runs in a session of `runner` that mirrors the
/// build directory and keeps the user's own environment; an exit without a
/// code reports 1.
pub async fn run_build_script_interactive(
    runner: &dyn Runner,
    work_dir: &Path,
    trace: bool,
) -> std::io::Result<i32> {
    validate_work_dir(work_dir)?;
    let exec = build_script_exec(work_dir, trace)?;
    let mut session = runner
        .start_session(debug_session_spec(runner, work_dir)?)
        .await
        .map_err(io_error)?;
    let status = session
        .exec_interactive(exec)
        .await
        .into_status()
        .map_err(io_error)?;

    Ok(status.code.unwrap_or(1))
}

// ---------------------------------------------------------------------------
// Platform-specific implementations
// ---------------------------------------------------------------------------

/// Hint text for the build script command (used in the banner).
#[cfg(unix)]
fn build_script_hint() -> &'static str {
    "bash -x conda_build.sh"
}

#[cfg(windows)]
fn build_script_hint() -> &'static str {
    "conda_build.bat"
}

/// Validate that a work directory exists.
fn validate_work_dir(work_dir: &Path) -> std::io::Result<()> {
    if !work_dir.exists() {
        eprintln!(
            "Error: Work directory does not exist: {}",
            work_dir.display()
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Work directory not found: {}", work_dir.display()),
        ));
    }
    Ok(())
}

/// Check that a required file exists in the work directory, returning a
/// descriptive IO error if it does not.
fn require_file(work_dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let path = work_dir.join(name);
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} not found in {}", name, work_dir.display()),
        ));
    }
    Ok(path)
}

/// A command that runs `argv` in `work_dir` without explicit environment
/// variables, so the shell sees exactly the session's host environment, plus
/// the `RATTLER_BUILD_PACKAGE_FILES` channel, so a build script that appends
/// to that file runs the same way it does in a build. The contents are
/// discarded: debug commands do not package.
fn shell_exec(argv: Vec<String>, work_dir: &Path) -> ExecSpec {
    ExecSpec {
        argv,
        cwd: GuestPath(work_dir.to_path_buf()),
        env: IndexMap::new(),
        channels: vec![crate::consts::RATTLER_BUILD_PACKAGE_FILES.to_string()],
    }
}

/// The session every debug command runs in.
///
/// The session mounts only the build directory (the parent of `work_dir`,
/// which also holds the host and build prefixes), writable, so the shell can
/// change sources and prefixes in place; `work_dir` is made absolute first so
/// a relative path still yields that parent. The build directory is also the
/// session's scratch directory. Nothing else, such as the recipe
/// directory or the user's home, is mounted: debug sessions are meant for the
/// local runner, which exposes host paths unchanged. The session applies
/// [`EnvironmentIsolation::None`]: the debug shell deliberately runs in the
/// user's own environment so that their tools, shell configuration, and
/// credentials are available.
fn debug_session_spec(runner: &dyn Runner, work_dir: &Path) -> std::io::Result<SessionSpec> {
    let work_dir = std::path::absolute(work_dir)?;
    let build_dir = work_dir.parent().unwrap_or(&work_dir);
    Ok(SessionSpec {
        platform: runner.execution_platform(),
        mounts: Mounts::new().writable(build_dir),
        image: None,
        work_dir: GuestPath(work_dir.clone()),
        scratch_dir: GuestPath(build_dir.to_path_buf()),
        env_isolation: EnvironmentIsolation::None,
    })
}

/// Converts a [`RunnerError`] into the I/O error the debug commands report.
fn io_error(error: RunnerError) -> std::io::Error {
    match error {
        RunnerError::Io(error) => error,
        RunnerError::UnsupportedPlatform { .. }
        | RunnerError::MissingExecutable { .. }
        | RunnerError::UnsupportedMount { .. }
        | RunnerError::ChannelTooLarge { .. } => std::io::Error::other(error),
    }
}

/// Describes an [`ExecStatus`] the way [`std::process::ExitStatus`] displays
/// it on this platform.
fn describe_exit_status(status: ExecStatus) -> String {
    match status.code {
        Some(code) => format!("{EXIT_CODE_LABEL}: {code}"),
        None => "terminated by signal".to_string(),
    }
}

/// Collects the output of a captured build-script run per stream.
///
/// Every received line is stored followed by a newline, so each stream holds
/// the process output with line endings normalized to `\n`.
#[derive(Default)]
struct CapturedOutput {
    stdout: String,
    stderr: String,
}

impl OutputSink for CapturedOutput {
    fn line(&mut self, stream: OutputStream, line: &str) {
        let buffer = match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr => &mut self.stderr,
        };
        buffer.push_str(line);
        buffer.push('\n');
    }
}

// -- Unix implementations --------------------------------------------------

/// The label [`std::process::ExitStatus`] displays before an exit code.
#[cfg(unix)]
const EXIT_CODE_LABEL: &str = "exit status";

/// The command that starts `$SHELL` with `build_env.sh` sourced.
#[cfg(unix)]
fn open_debug_shell_exec(work_dir: &Path, directories_json: Option<serde_json::Value>) -> ExecSpec {
    let build_env = work_dir.join("build_env.sh");
    if !build_env.exists() {
        eprintln!("Warning: build_env.sh not found in {}", work_dir.display());
        eprintln!("The build environment may not have been set up yet.");
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    let env_exports = directories_json
        .as_ref()
        .map(build_env_exports)
        .unwrap_or_default();

    let shell_script = if build_env.exists() {
        format!(
            "cd '{}' && {}source build_env.sh && exec {} -i",
            work_dir.display(),
            env_exports,
            shell
        )
    } else {
        format!(
            "cd '{}' && {}exec {} -i",
            work_dir.display(),
            env_exports,
            shell
        )
    };

    shell_exec(vec![shell, "-c".to_string(), shell_script], work_dir)
}

/// The command that runs `conda_build.sh` through `bash` with `build_env.sh`
/// sourced, tracing every command when `trace` is set.
#[cfg(unix)]
fn build_script_exec(work_dir: &Path, trace: bool) -> std::io::Result<ExecSpec> {
    require_file(work_dir, "build_env.sh")?;
    require_file(work_dir, "conda_build.sh")?;

    let bash_flag = if trace { "-ex" } else { "-e" };
    let script = format!(
        "cd '{}' && source build_env.sh && bash {} conda_build.sh",
        work_dir.display(),
        bash_flag,
    );

    Ok(shell_exec(
        vec!["bash".to_string(), "-c".to_string(), script],
        work_dir,
    ))
}

// -- Windows implementations -----------------------------------------------

/// The label [`std::process::ExitStatus`] displays before an exit code.
#[cfg(windows)]
const EXIT_CODE_LABEL: &str = "exit code";

/// The command that starts `cmd.exe` with `build_env.bat` called.
#[cfg(windows)]
fn open_debug_shell_exec(work_dir: &Path, directories_json: Option<serde_json::Value>) -> ExecSpec {
    let build_env = work_dir.join("build_env.bat");
    if !build_env.exists() {
        eprintln!("Warning: build_env.bat not found in {}", work_dir.display());
        eprintln!("The build environment may not have been set up yet.");
    }

    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());

    let env_sets = directories_json
        .as_ref()
        .map(build_env_sets_windows)
        .unwrap_or_default();

    let init_cmd = if build_env.exists() {
        format!(
            "cd /d \"{}\" && {}call build_env.bat",
            work_dir.display(),
            env_sets
        )
    } else {
        format!("cd /d \"{}\" && {}", work_dir.display(), env_sets)
    };

    shell_exec(
        vec![comspec, "/d".to_string(), "/k".to_string(), init_cmd],
        work_dir,
    )
}

/// The command that runs `conda_build.bat` through `cmd.exe`, echoing every
/// command when `trace` is set. The script is named with an explicit `.\`
/// so `cmd.exe` opens it from the work directory instead of searching
/// `PATH`, which skips the current directory when
/// `NoDefaultCurrentDirectoryInExePath` is set.
#[cfg(windows)]
fn build_script_exec(work_dir: &Path, trace: bool) -> std::io::Result<ExecSpec> {
    require_file(work_dir, "conda_build.bat")?;

    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());

    let script = if trace {
        r"@echo on && .\conda_build.bat"
    } else {
        r".\conda_build.bat"
    };

    Ok(shell_exec(
        vec![
            comspec,
            "/d".to_string(),
            "/c".to_string(),
            script.to_string(),
        ],
        work_dir,
    ))
}

// ---------------------------------------------------------------------------
// Environment modification
// ---------------------------------------------------------------------------

/// Add packages to a conda prefix, preserving existing packages.
///
/// Reads installed packages from `prefix`, locks them at their current
/// version, merges in `specs`, resolves, and installs only what changed.
pub async fn add_packages_to_prefix(
    env_name: &str,
    prefix: &Path,
    specs: &[String],
    channels: &[ChannelUrl],
    tool_config: &Configuration,
    exclude_newer: Option<rattler_solve::ExcludeNewer>,
) -> miette::Result<()> {
    use miette::IntoDiagnostic;

    let new_specs: Vec<MatchSpec> = specs
        .iter()
        .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
        .collect::<Result<Vec<_>, _>>()
        .into_diagnostic()?;

    let existing_records = PrefixRecord::collect_from_prefix(prefix).into_diagnostic()?;

    let mut all_specs: Vec<MatchSpec> = existing_records
        .iter()
        .map(|r: &PrefixRecord| {
            MatchSpec::from_str(
                &format!(
                    "{}={}={}",
                    r.repodata_record.package_record.name.as_normalized(),
                    r.repodata_record.package_record.version,
                    r.repodata_record.package_record.build,
                ),
                ParseStrictness::Lenient,
            )
            .expect("existing package record should parse as MatchSpec")
        })
        .collect();
    all_specs.extend(new_specs);

    let platform_with_vp = crate::metadata::PlatformWithVirtualPackages::detect(
        &rattler_virtual_packages::VirtualPackageOverrides::default(),
    )
    .into_diagnostic()?;

    let span_msg = format!(
        "Adding {} new spec(s) to {env_name} environment ({} existing packages)",
        specs.len(),
        existing_records.len(),
    );
    let span = tracing::info_span!("", message = %span_msg);
    let _enter = span.enter();
    tracing::info!("");

    crate::render::solver::create_environment(
        env_name,
        &all_specs,
        &platform_with_vp,
        prefix,
        channels,
        tool_config,
        rattler_solve::ChannelPriority::Strict,
        rattler_solve::SolveStrategy::default(),
        exclude_newer,
    )
    .await?;

    tracing::info!(
        "{} Successfully added packages to {env_name} environment",
        console::style(console::Emoji("✔", "")).green(),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Output extension
// ---------------------------------------------------------------------------

impl crate::metadata::Output {
    /// Set up a debug environment without running the build.
    ///
    /// This consolidates the 5-step setup sequence:
    /// 1. Recreate directories
    /// 2. Fetch sources
    /// 3. Resolve dependencies
    /// 4. Install environments
    /// 5. Create build script
    pub async fn setup_debug_environment(
        self,
        tool_config: &Configuration,
    ) -> miette::Result<crate::metadata::Output> {
        use miette::IntoDiagnostic;

        self.build_configuration
            .directories
            .recreate_directories()
            .into_diagnostic()?;

        let output: crate::metadata::Output = self
            .fetch_sources(tool_config, apply_patch_custom)
            .await
            .into_diagnostic()?;

        let output: crate::metadata::Output = output
            .resolve_dependencies(tool_config, RunExportsDownload::DownloadMissing)
            .await
            .into_diagnostic()?;

        output
            .install_environments(tool_config)
            .await
            .into_diagnostic()?;

        output.create_build_script().await.into_diagnostic()?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use rattler_build_script::runner::{
        CHANNEL_SIZE_LIMIT, ExecOutcome, GuestInfo, LocalRunner, Session,
    };
    use rattler_conda_types::Subdir;

    use super::*;

    /// Which [`Session`] method a [`RecordingRunner`] session received.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ExecKind {
        Captured,
        Interactive,
    }

    /// The specs a [`RecordingRunner`] session received.
    #[derive(Default, Clone)]
    struct RecordedSpecs {
        session: Option<SessionSpec>,
        exec: Option<(ExecKind, ExecSpec)>,
    }

    /// A [`Runner`] whose session records the specs it receives and replays
    /// fixed output lines into the sink, so the session path of the debug
    /// commands can be observed without spawning a process. With
    /// [`RecordingRunner::with_oversized_channel`] every command fails with
    /// [`RunnerError::ChannelTooLarge`] for its first channel instead of
    /// returning an outcome.
    struct RecordingRunner {
        local: LocalRunner,
        status: ExecStatus,
        output: Vec<(OutputStream, String)>,
        oversized_channel: bool,
        recorded: Arc<Mutex<RecordedSpecs>>,
    }

    impl RecordingRunner {
        fn new(status: ExecStatus, output: Vec<(OutputStream, String)>) -> Self {
            Self {
                local: LocalRunner::new(),
                status,
                output,
                oversized_channel: false,
                recorded: Arc::default(),
            }
        }

        fn with_oversized_channel(mut self) -> Self {
            self.oversized_channel = true;
            self
        }

        fn recorded(&self) -> RecordedSpecs {
            self.recorded.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Runner for RecordingRunner {
        fn name(&self) -> &str {
            "recording"
        }

        fn execution_platform(&self) -> Subdir {
            self.local.execution_platform()
        }

        async fn check_usable(&self) -> Result<(), RunnerError> {
            Ok(())
        }

        async fn probe(&self) -> Result<GuestInfo, RunnerError> {
            self.local.probe().await
        }

        async fn start_session(&self, spec: SessionSpec) -> Result<Box<dyn Session>, RunnerError> {
            self.recorded.lock().unwrap().session = Some(spec);
            Ok(Box::new(RecordingSession {
                status: self.status,
                output: self.output.clone(),
                oversized_channel: self.oversized_channel,
                recorded: Arc::clone(&self.recorded),
            }))
        }
    }

    struct RecordingSession {
        status: ExecStatus,
        output: Vec<(OutputStream, String)>,
        oversized_channel: bool,
        recorded: Arc<Mutex<RecordedSpecs>>,
    }

    impl RecordingSession {
        /// Records `spec` as received through `kind` and answers it with the
        /// fixed status and an empty file per declared channel, or with the
        /// oversized-channel error carrying that status.
        fn outcome(&self, kind: ExecKind, spec: ExecSpec) -> Result<ExecOutcome, RunnerError> {
            let channels: IndexMap<String, Vec<u8>> = spec
                .channels
                .iter()
                .map(|name| (name.clone(), Vec::new()))
                .collect();
            self.recorded.lock().unwrap().exec = Some((kind, spec));
            if self.oversized_channel
                && let Some(variable) = channels.keys().next()
            {
                return Err(RunnerError::ChannelTooLarge {
                    variable: variable.clone(),
                    limit: CHANNEL_SIZE_LIMIT,
                    status: self.status,
                });
            }
            Ok(ExecOutcome {
                status: self.status,
                channels,
            })
        }
    }

    #[async_trait]
    impl Session for RecordingSession {
        async fn exec(
            &mut self,
            spec: ExecSpec,
            sink: &mut dyn OutputSink,
        ) -> Result<ExecOutcome, RunnerError> {
            for (stream, line) in &self.output {
                sink.line(*stream, line);
            }
            self.outcome(ExecKind::Captured, spec)
        }

        async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecOutcome, RunnerError> {
            self.outcome(ExecKind::Interactive, spec)
        }

        async fn find_executable(
            &mut self,
            _names: &[String],
            _search_paths: &[GuestPath],
        ) -> Result<Option<GuestPath>, RunnerError> {
            Ok(None)
        }
    }

    /// Creates `<temp_dir>/build/work` holding the build environment script
    /// and `conda_build_script` as the build script, and returns the work
    /// directory.
    fn work_dir_with_scripts(temp_dir: &Path, conda_build_script: &str) -> PathBuf {
        let work_dir = temp_dir.join("build").join("work");
        fs_err::create_dir_all(&work_dir).unwrap();
        fs_err::write(work_dir.join(build_env_file_name()), "").unwrap();
        fs_err::write(work_dir.join(build_script_file_name()), conda_build_script).unwrap();
        work_dir
    }

    #[cfg(unix)]
    fn build_env_file_name() -> &'static str {
        "build_env.sh"
    }

    #[cfg(windows)]
    fn build_env_file_name() -> &'static str {
        "build_env.bat"
    }

    #[cfg(unix)]
    fn build_script_file_name() -> &'static str {
        "conda_build.sh"
    }

    #[cfg(windows)]
    fn build_script_file_name() -> &'static str {
        "conda_build.bat"
    }

    #[cfg(unix)]
    fn expected_shell_argv(work_dir: &Path, directories_json: &serde_json::Value) -> Vec<String> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        vec![
            shell.clone(),
            "-c".to_string(),
            format!(
                "cd '{}' && {}source build_env.sh && exec {} -i",
                work_dir.display(),
                build_env_exports(directories_json),
                shell
            ),
        ]
    }

    #[cfg(windows)]
    fn expected_shell_argv(work_dir: &Path, directories_json: &serde_json::Value) -> Vec<String> {
        vec![
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            "/d".to_string(),
            "/k".to_string(),
            format!(
                "cd /d \"{}\" && {}call build_env.bat",
                work_dir.display(),
                build_env_sets_windows(directories_json)
            ),
        ]
    }

    #[cfg(unix)]
    fn expected_build_script_argv(work_dir: &Path, trace: bool) -> Vec<String> {
        let bash_flag = if trace { "-ex" } else { "-e" };
        vec![
            "bash".to_string(),
            "-c".to_string(),
            format!(
                "cd '{}' && source build_env.sh && bash {} conda_build.sh",
                work_dir.display(),
                bash_flag
            ),
        ]
    }

    #[cfg(windows)]
    fn expected_build_script_argv(_work_dir: &Path, trace: bool) -> Vec<String> {
        let script = if trace {
            r"@echo on && .\conda_build.bat"
        } else {
            r".\conda_build.bat"
        };
        vec![
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            "/d".to_string(),
            "/c".to_string(),
            script.to_string(),
        ]
    }

    /// Every debug command starts its session for the work directory on the
    /// runner's platform, with the build directory mounted writable and used
    /// as the scratch directory, and without environment isolation.
    fn assert_debug_session(session: &SessionSpec, work_dir: &Path) {
        assert_eq!(session.platform, Subdir::current().unwrap());
        assert_eq!(
            session.mounts,
            Mounts::new().writable(work_dir.parent().unwrap())
        );
        assert!(session.image.is_none());
        assert_eq!(session.work_dir, GuestPath(work_dir.to_path_buf()));
        assert_eq!(
            session.scratch_dir,
            GuestPath(work_dir.parent().unwrap().to_path_buf())
        );
        assert_eq!(session.env_isolation, EnvironmentIsolation::None);
    }

    /// An overfilled channel does not hide how the script exited: the debug
    /// commands discard channel contents and keep the exit status.
    #[tokio::test]
    async fn oversized_channel_keeps_the_exit_status() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = work_dir_with_scripts(temp_dir.path(), "");
        let runner = RecordingRunner::new(
            ExecStatus { code: Some(5) },
            vec![(OutputStream::Stdout, "out".to_string())],
        )
        .with_oversized_channel();

        let result = run_build_script(&runner, &work_dir, false).await.unwrap();
        let exit_code = run_build_script_interactive(&runner, &work_dir, false)
            .await
            .unwrap();
        let shell_error = open_debug_shell(&runner, work_dir.clone(), None)
            .await
            .unwrap_err();

        assert_eq!(result.exit_code, 5);
        assert_eq!(result.stdout, "out\n");
        assert_eq!(exit_code, 5);
        assert_eq!(
            shell_error.to_string(),
            format!("shell exited with status: {EXIT_CODE_LABEL}: 5")
        );
    }

    /// Every debug command runs in the work directory without explicit
    /// variables and with the package-files channel declared.
    fn assert_exec(exec: &ExecSpec, work_dir: &Path, argv: Vec<String>) {
        assert_eq!(exec.argv, argv);
        assert_eq!(exec.cwd, GuestPath(work_dir.to_path_buf()));
        assert!(exec.env.is_empty());
        assert_eq!(
            exec.channels,
            vec![crate::consts::RATTLER_BUILD_PACKAGE_FILES.to_string()]
        );
    }

    #[tokio::test]
    async fn open_debug_shell_runs_the_shell_interactively_in_a_debug_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = work_dir_with_scripts(temp_dir.path(), "");
        let directories_json = serde_json::json!({
            "work_dir": work_dir,
            "host_prefix": temp_dir.path().join("build").join("host_env"),
        });
        let runner = RecordingRunner::new(ExecStatus { code: Some(0) }, Vec::new());

        open_debug_shell(&runner, work_dir.clone(), Some(directories_json.clone()))
            .await
            .unwrap();

        let recorded = runner.recorded();
        assert_debug_session(&recorded.session.unwrap(), &work_dir);
        let (kind, exec) = recorded.exec.unwrap();
        assert_eq!(kind, ExecKind::Interactive);
        assert_exec(
            &exec,
            &work_dir,
            expected_shell_argv(&work_dir, &directories_json),
        );
    }

    #[tokio::test]
    async fn open_debug_shell_reports_a_failing_shell() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = work_dir_with_scripts(temp_dir.path(), "");
        let runner = RecordingRunner::new(ExecStatus { code: Some(2) }, Vec::new());

        let error = open_debug_shell(&runner, work_dir, None).await.unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert_eq!(
            error.to_string(),
            format!("shell exited with status: {EXIT_CODE_LABEL}: 2")
        );
    }

    /// A relative work directory is resolved against the current directory
    /// before its parent becomes the mount, so the mount is never empty.
    #[test]
    fn relative_work_dir_mounts_its_absolute_parent() {
        let runner = RecordingRunner::new(ExecStatus { code: Some(0) }, Vec::new());
        let work_dir = Path::new("work");

        let session = debug_session_spec(&runner, work_dir).unwrap();

        let absolute_work_dir = std::path::absolute(work_dir).unwrap();
        assert_eq!(
            session.mounts,
            Mounts::new().writable(absolute_work_dir.parent().unwrap())
        );
        assert_eq!(session.work_dir, GuestPath(absolute_work_dir));
    }

    #[tokio::test]
    async fn run_build_script_captures_the_session_output_per_stream() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = work_dir_with_scripts(temp_dir.path(), "");
        let runner = RecordingRunner::new(
            ExecStatus { code: Some(5) },
            vec![
                (OutputStream::Stdout, "out1".to_string()),
                (OutputStream::Stderr, "err".to_string()),
                (OutputStream::Stdout, "out2".to_string()),
            ],
        );

        let result = run_build_script(&runner, &work_dir, false).await.unwrap();

        assert_eq!(result.exit_code, 5);
        assert_eq!(result.stdout, "out1\nout2\n");
        assert_eq!(result.stderr, "err\n");
        let recorded = runner.recorded();
        assert_debug_session(&recorded.session.unwrap(), &work_dir);
        let (kind, exec) = recorded.exec.unwrap();
        assert_eq!(kind, ExecKind::Captured);
        assert_exec(
            &exec,
            &work_dir,
            expected_build_script_argv(&work_dir, false),
        );
    }

    #[tokio::test]
    async fn run_build_script_traces_the_script_when_asked() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = work_dir_with_scripts(temp_dir.path(), "");
        let runner = RecordingRunner::new(ExecStatus { code: Some(0) }, Vec::new());

        run_build_script(&runner, &work_dir, true).await.unwrap();

        let (_, exec) = runner.recorded().exec.unwrap();
        assert_exec(
            &exec,
            &work_dir,
            expected_build_script_argv(&work_dir, true),
        );
    }

    /// A script terminated without an exit code reports exit code 1.
    #[tokio::test]
    async fn run_build_script_interactive_runs_the_script_in_a_debug_session() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = work_dir_with_scripts(temp_dir.path(), "");
        let runner = RecordingRunner::new(ExecStatus { code: None }, Vec::new());

        let exit_code = run_build_script_interactive(&runner, &work_dir, true)
            .await
            .unwrap();

        assert_eq!(exit_code, 1);
        let recorded = runner.recorded();
        assert_debug_session(&recorded.session.unwrap(), &work_dir);
        let (kind, exec) = recorded.exec.unwrap();
        assert_eq!(kind, ExecKind::Interactive);
        assert_exec(
            &exec,
            &work_dir,
            expected_build_script_argv(&work_dir, true),
        );
    }

    #[tokio::test]
    async fn run_build_script_through_the_local_runner_captures_both_streams() {
        let temp_dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let script = "echo out\necho err >&2\nexit 3\n";
        #[cfg(windows)]
        let script = "@echo off\r\necho out\r\n1>&2 echo err\r\nexit /b 3\r\n";
        let work_dir = work_dir_with_scripts(temp_dir.path(), script);
        let runner = LocalRunner::new();

        let result = run_build_script(&runner, &work_dir, false).await.unwrap();

        assert_eq!(result.exit_code, 3, "unexpected result: {result:?}");
        assert_eq!(result.stdout, "out\n");
        assert_eq!(result.stderr, "err\n");
    }

    /// With `trace`, the real shell echoes every command of the script:
    /// `bash -x` traces to stderr, `cmd.exe` with echo on prints the command
    /// line to stdout before its output.
    #[tokio::test]
    async fn run_build_script_through_the_local_runner_traces_the_script() {
        let temp_dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let script = "echo out\n";
        #[cfg(windows)]
        let script = "echo out\r\n";
        let work_dir = work_dir_with_scripts(temp_dir.path(), script);
        let runner = LocalRunner::new();

        let result = run_build_script(&runner, &work_dir, true).await.unwrap();

        assert_eq!(result.exit_code, 0, "unexpected result: {result:?}");
        #[cfg(unix)]
        {
            assert_eq!(result.stdout, "out\n");
            assert!(
                result.stderr.contains("+ echo out\n"),
                "unexpected stderr: {:?}",
                result.stderr
            );
        }
        #[cfg(windows)]
        {
            assert!(
                result.stdout.contains(">echo out"),
                "unexpected stdout: {:?}",
                result.stdout
            );
            assert!(
                result.stdout.lines().any(|line| line == "out"),
                "unexpected stdout: {:?}",
                result.stdout
            );
            assert_eq!(result.stderr, "");
        }
    }

    #[tokio::test]
    async fn missing_build_script_fails_before_a_session_starts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = temp_dir.path().join("build").join("work");
        fs_err::create_dir_all(&work_dir).unwrap();
        let runner = RecordingRunner::new(ExecStatus { code: Some(0) }, Vec::new());

        let captured = run_build_script(&runner, &work_dir, false)
            .await
            .unwrap_err();
        let interactive = run_build_script_interactive(&runner, &work_dir, false)
            .await
            .unwrap_err();

        assert_eq!(captured.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(interactive.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            captured.to_string(),
            format!(
                "{} not found in {}",
                required_build_file_name(),
                work_dir.display()
            )
        );
        assert!(runner.recorded().session.is_none());
    }

    /// The first file the build-script commands require to exist.
    #[cfg(unix)]
    fn required_build_file_name() -> &'static str {
        "build_env.sh"
    }

    #[cfg(windows)]
    fn required_build_file_name() -> &'static str {
        "conda_build.bat"
    }

    #[tokio::test]
    async fn missing_work_dir_fails_before_a_session_starts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let work_dir = temp_dir.path().join("missing");
        let runner = RecordingRunner::new(ExecStatus { code: Some(0) }, Vec::new());

        let error = open_debug_shell(&runner, work_dir.clone(), None)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            error.to_string(),
            format!("Work directory not found: {}", work_dir.display())
        );
        assert!(runner.recorded().session.is_none());
    }
}
