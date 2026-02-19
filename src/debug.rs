//! Debug command implementations: shell, host-add, build-add.

use std::{
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::LoggingOutputHandler,
    opt::{DebugEnvAddOpts, DebugShellOpts},
};
use rattler_conda_types::MatchSpec;
use rattler_config::config::ConfigBase;

/// Parse the directories JSON from environment, rattler-build-log.txt, or work directory.
///
/// Resolution order:
/// 1. If `--work-dir` is explicitly given, use it directly.
/// 2. If `RATTLER_BUILD_DIRECTORIES` env var is set (inside a debug shell), parse it.
/// 3. Fall back to reading `rattler-build-log.txt` from the output directory.
fn parse_directories_info(
    work_dir: Option<PathBuf>,
    output_dir: &Path,
) -> std::io::Result<(PathBuf, Option<serde_json::Value>)> {
    if let Some(dir) = work_dir {
        return Ok((dir, None));
    }

    // Check if we're inside a debug shell with RATTLER_BUILD_DIRECTORIES set
    if let Ok(json_str) = std::env::var("RATTLER_BUILD_DIRECTORIES") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
            let work_dir = json["work_dir"].as_str().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "work_dir not found in RATTLER_BUILD_DIRECTORIES",
                )
            })?;
            return Ok((PathBuf::from(work_dir), Some(json)));
        }
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

/// Build shell environment variable exports from directories JSON
fn build_env_exports(json: &serde_json::Value) -> String {
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
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(canonical) = exe.canonicalize() {
            env_exports.push_str(&format!(
                "export RATTLER_BUILD='{}'\nalias rattler-build='{}'\n",
                canonical.display(),
                canonical.display()
            ));
        }
    }

    env_exports
}

/// Print the debug shell welcome banner
fn print_debug_banner(work_dir: &Path, directories_json: &Option<serde_json::Value>) {
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
    println!("    rattler-build create-patch              Create a patch from your changes");
    println!("    rattler-build debug host-add <pkg>      Add packages to host env");
    println!("    rattler-build debug build-add <pkg>     Add packages to build env");
    println!();
    println!("  The build environment has been sourced. Run `bash -x conda_build.sh` to");
    println!("  execute the build script, or make changes and use create-patch.");
    println!();
    println!("  Exit with 'exit' or Ctrl+D.");
    println!();
}

/// Open a debug shell in the build environment
pub fn open_debug_shell(
    work_dir: PathBuf,
    directories_json: Option<serde_json::Value>,
) -> std::io::Result<()> {
    // Check if work_dir exists
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

    // Check if build_env.sh exists
    let build_env = work_dir.join("build_env.sh");
    if !build_env.exists() {
        eprintln!("Warning: build_env.sh not found in {}", work_dir.display());
        eprintln!("The build environment may not have been set up yet.");
    }

    print_debug_banner(&work_dir, &directories_json);

    // Determine the shell to use
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    // Build environment variable exports
    let env_exports = directories_json
        .as_ref()
        .map(build_env_exports)
        .unwrap_or_default();

    // Create a shell script that sources build_env.sh and then starts an interactive shell
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

    // Execute the shell
    let status = Command::new(&shell).arg("-c").arg(&shell_script).status()?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "Shell exited with status: {}",
            status
        )));
    }

    Ok(())
}

/// Open a debug shell from DebugShellOpts
pub fn debug_shell(opts: DebugShellOpts) -> std::io::Result<()> {
    let (work_dir, directories_json) = parse_directories_info(opts.work_dir, &opts.output_dir)?;
    open_debug_shell(work_dir, directories_json)
}

/// Add packages to a host or build environment in an existing debug build.
///
/// Reads the existing installed packages from the prefix's conda-meta/,
/// combines them with the new specs, resolves, and installs only what changed.
pub async fn debug_env_add(
    env_name: &str,
    opts: DebugEnvAddOpts,
    _config: Option<ConfigBase<()>>,
    log_handler: &Option<LoggingOutputHandler>,
) -> miette::Result<()> {
    let (_work_dir, directories_json) =
        parse_directories_info(opts.work_dir, &opts.output_dir).into_diagnostic()?;

    let directories_json = directories_json.ok_or_else(|| {
        miette::miette!("Could not read build directories info. Run `rattler-build debug` first.")
    })?;

    let prefix_key = format!("{}_prefix", env_name);
    let target_prefix = directories_json[&prefix_key].as_str().ok_or_else(|| {
        miette::miette!(
            "{}_prefix not found in build log. \
                 The build may have been created with an older version.",
            env_name
        )
    })?;
    let target_prefix = PathBuf::from(target_prefix);

    if !target_prefix.exists() {
        return Err(miette::miette!(
            "{} prefix does not exist: {}",
            env_name,
            target_prefix.display()
        ));
    }

    // Parse the new match specs
    let new_specs: Vec<MatchSpec> = opts
        .specs
        .iter()
        .map(|s| MatchSpec::from_str(s, rattler_conda_types::ParseStrictness::Lenient))
        .collect::<Result<Vec<_>, _>>()
        .into_diagnostic()?;

    // Read existing installed packages and create "locked" specs from them
    let existing_records =
        rattler_conda_types::PrefixRecord::collect_from_prefix(&target_prefix).into_diagnostic()?;

    let mut all_specs: Vec<MatchSpec> = existing_records
        .iter()
        .map(|r: &rattler_conda_types::PrefixRecord| {
            // Lock existing packages to their exact name+version+build so the solver
            // doesn't remove or change them when adding new packages
            MatchSpec::from_str(
                &format!(
                    "{}={}={}",
                    r.repodata_record.package_record.name.as_normalized(),
                    r.repodata_record.package_record.version,
                    r.repodata_record.package_record.build,
                ),
                rattler_conda_types::ParseStrictness::Lenient,
            )
            .expect("existing package record should parse as MatchSpec")
        })
        .collect();

    // Append the new specs
    all_specs.extend(new_specs);

    // Resolve channels
    let channels: Vec<rattler_conda_types::ChannelUrl> = {
        let channel_strs = opts.channels.unwrap_or_else(|| {
            vec![rattler_conda_types::NamedChannelOrUrl::from_str("conda-forge").unwrap()]
        });
        let channel_config = rattler_conda_types::ChannelConfig::default_with_root_dir(
            std::env::current_dir().into_diagnostic()?,
        );
        channel_strs
            .iter()
            .map(|c| {
                c.clone()
                    .into_channel(&channel_config)
                    .expect("invalid channel")
                    .base_url
            })
            .collect()
    };

    // Build a minimal Configuration using the builder directly
    let client = rattler_build::tool_configuration::reqwest_client_from_auth_storage(
        opts.auth_file,
        #[cfg(feature = "s3")]
        Default::default(),
        Default::default(),
        None,
    )
    .into_diagnostic()?;

    let mut builder = rattler_build::tool_configuration::Configuration::builder()
        .with_reqwest_client(client)
        .with_channel_priority(rattler_solve::ChannelPriority::Strict);

    if let Some(handler) = log_handler {
        builder = builder.with_logging_output_handler(handler.clone());
    }

    let tool_config = builder.finish();

    // Detect platform
    let platform_with_vp = rattler_build::metadata::PlatformWithVirtualPackages::detect(
        &rattler_virtual_packages::VirtualPackageOverrides::default(),
    )
    .into_diagnostic()?;

    tracing::info!(
        "\nAdding {} new spec(s) to {} environment ({} existing packages)",
        opts.specs.len(),
        env_name,
        existing_records.len(),
    );

    // Solve with all specs (existing locked + new), then install.
    // install_packages internally reads PrefixRecord::collect_from_prefix()
    // and only installs what's new/changed.
    rattler_build::render::solver::create_environment(
        env_name,
        &all_specs,
        &platform_with_vp,
        &target_prefix,
        &channels,
        &tool_config,
        rattler_solve::ChannelPriority::Strict,
        rattler_solve::SolveStrategy::default(),
        None,
    )
    .await?;

    tracing::info!("\nSuccessfully added packages to {} environment.", env_name);

    Ok(())
}
