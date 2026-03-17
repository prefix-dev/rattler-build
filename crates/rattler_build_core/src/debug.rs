//! Cross-platform debug utilities for rattler-build.
//!
//! This module provides the core logic for setting up, inspecting, and running
//! debug build environments. It is used by both the CLI (`rattler-build debug`)
//! and the Python bindings (`DebugSession`).

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use rattler_conda_types::{ChannelUrl, MatchSpec, ParseStrictness, PrefixRecord};

use crate::render::resolved_dependencies::RunExportsDownload;
use crate::source::patch::apply_patch_custom;
use crate::tool_configuration::Configuration;

/// Result of running a debug build script.
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
pub fn open_debug_shell(
    work_dir: PathBuf,
    directories_json: Option<serde_json::Value>,
) -> std::io::Result<()> {
    validate_work_dir(&work_dir)?;
    print_debug_banner(&work_dir, &directories_json);
    open_debug_shell_platform(&work_dir, directories_json)
}

/// Run the build script and capture its output.
///
/// Returns a [`DebugRunResult`] with captured stdout and stderr.
/// On Unix, runs via `bash`. On Windows, runs via `cmd.exe`.
pub fn run_build_script(work_dir: &Path, trace: bool) -> std::io::Result<DebugRunResult> {
    validate_work_dir(work_dir)?;
    run_build_script_platform(work_dir, trace)
}

/// Run the build script with inherited stdio (for interactive CLI use).
///
/// Returns just the exit code. On Unix, runs via `bash`. On Windows, runs
/// via `cmd.exe`.
pub fn run_build_script_interactive(work_dir: &Path, trace: bool) -> std::io::Result<i32> {
    validate_work_dir(work_dir)?;
    run_build_script_interactive_platform(work_dir, trace)
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

// -- Unix implementations --------------------------------------------------

#[cfg(unix)]
fn open_debug_shell_platform(
    work_dir: &Path,
    directories_json: Option<serde_json::Value>,
) -> std::io::Result<()> {
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

    let status = Command::new(&shell).arg("-c").arg(&shell_script).status()?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "shell exited with status: {}",
            status
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn run_build_script_platform(work_dir: &Path, trace: bool) -> std::io::Result<DebugRunResult> {
    require_file(work_dir, "build_env.sh")?;
    require_file(work_dir, "conda_build.sh")?;

    let bash_flag = if trace { "-ex" } else { "-e" };
    let script = format!(
        "cd '{}' && source build_env.sh && bash {} conda_build.sh",
        work_dir.display(),
        bash_flag,
    );

    let output = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()?;

    Ok(DebugRunResult {
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(unix)]
fn run_build_script_interactive_platform(work_dir: &Path, trace: bool) -> std::io::Result<i32> {
    require_file(work_dir, "build_env.sh")?;
    require_file(work_dir, "conda_build.sh")?;

    let bash_flag = if trace { "-ex" } else { "-e" };
    let script = format!(
        "cd '{}' && source build_env.sh && bash {} conda_build.sh",
        work_dir.display(),
        bash_flag,
    );

    let status = Command::new("bash").arg("-c").arg(&script).status()?;
    Ok(status.code().unwrap_or(1))
}

// -- Windows implementations -----------------------------------------------

#[cfg(windows)]
fn open_debug_shell_platform(
    work_dir: &Path,
    directories_json: Option<serde_json::Value>,
) -> std::io::Result<()> {
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

    let status = Command::new(&comspec)
        .arg("/d")
        .arg("/k")
        .arg(&init_cmd)
        .status()?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "shell exited with status: {}",
            status
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn run_build_script_platform(work_dir: &Path, trace: bool) -> std::io::Result<DebugRunResult> {
    require_file(work_dir, "conda_build.bat")?;

    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());

    let output = Command::new(&comspec)
        .arg("/d")
        .arg("/c")
        .arg("conda_build.bat")
        .current_dir(work_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()?;

    Ok(DebugRunResult {
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(windows)]
fn run_build_script_interactive_platform(work_dir: &Path, _trace: bool) -> std::io::Result<i32> {
    require_file(work_dir, "conda_build.bat")?;

    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());

    let status = Command::new(&comspec)
        .arg("/d")
        .arg("/c")
        .arg("conda_build.bat")
        .current_dir(work_dir)
        .status()?;

    Ok(status.code().unwrap_or(1))
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

    tracing::info!(
        "\nAdding {} new spec(s) to {} environment ({} existing packages)",
        specs.len(),
        env_name,
        existing_records.len(),
    );

    crate::render::solver::create_environment(
        env_name,
        &all_specs,
        &platform_with_vp,
        prefix,
        channels,
        tool_config,
        rattler_solve::ChannelPriority::Strict,
        rattler_solve::SolveStrategy::default(),
        None,
    )
    .await?;

    tracing::info!("\nSuccessfully added packages to {} environment.", env_name);

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
