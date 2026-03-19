//! Debug command implementations: shell, host-add, build-add.

use std::{path::PathBuf, str::FromStr};

use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::LoggingOutputHandler,
    debug as core_debug,
    opt::{CreatePatchOpts, DebugEnvAddOpts, DebugRunOpts, DebugShellOpts, DebugWorkdirOpts},
    source::create_patch,
};
use rattler_config::config::ConfigBase;

/// Open a debug shell from DebugShellOpts.
pub fn debug_shell(opts: DebugShellOpts) -> std::io::Result<()> {
    let output_dir = opts
        .common
        .output_dir
        .unwrap_or_else(|| PathBuf::from("./output"));
    let (work_dir, directories_json) =
        core_debug::parse_directories_info(opts.work_dir, &output_dir)?;
    core_debug::open_debug_shell(work_dir, directories_json)
}

/// Print the work directory path to stdout.
pub fn debug_workdir(opts: DebugWorkdirOpts) -> std::io::Result<()> {
    let output_dir = opts
        .common
        .output_dir
        .unwrap_or_else(|| PathBuf::from("./output"));
    let (work_dir, _) = core_debug::parse_directories_info(opts.work_dir, &output_dir)?;
    println!("{}", work_dir.display());
    Ok(())
}

/// Re-run the build script in an existing debug environment.
///
/// Returns the exit code of the build script so the caller can propagate it.
pub fn debug_run(opts: DebugRunOpts) -> std::io::Result<i32> {
    let output_dir = opts
        .common
        .output_dir
        .unwrap_or_else(|| PathBuf::from("./output"));
    let (work_dir, _) = core_debug::parse_directories_info(opts.work_dir, &output_dir)?;
    core_debug::run_build_script_interactive(&work_dir, opts.trace)
}

/// Create a patch from changes in the work directory.
pub fn debug_create_patch(opts: CreatePatchOpts) -> miette::Result<()> {
    let exclude_vec = opts.exclude.clone().unwrap_or_default();
    let add_vec = opts.add.clone().unwrap_or_default();
    let include_vec = opts.include.clone().unwrap_or_default();

    // Try to parse environment variable if available
    let env_dirs = std::env::var("RATTLER_BUILD_DIRECTORIES")
        .ok()
        .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok());

    // Determine the directory to use: --directory → env var → log file → cwd
    let directory = if let Some(dir) = opts.directory {
        dir
    } else if let Some(ref json) = env_dirs {
        if let Some(work_dir) = json["work_dir"].as_str() {
            tracing::info!(
                "Using work directory from RATTLER_BUILD_DIRECTORIES: {}",
                work_dir
            );
            PathBuf::from(work_dir)
        } else {
            std::env::current_dir().into_diagnostic()?
        }
    } else {
        // Try rattler-build-log.txt before falling back to cwd
        let log_file = PathBuf::from("./output/rattler-build-log.txt");
        if log_file.exists() {
            if let Ok(content) = fs_err::read_to_string(&log_file)
                && let Some(last_line) = content.lines().last()
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(last_line)
                && let Some(work_dir) = json["work_dir"].as_str()
            {
                tracing::info!(
                    "Using work directory from rattler-build-log.txt: {}",
                    work_dir
                );
                PathBuf::from(work_dir)
            } else {
                std::env::current_dir().into_diagnostic()?
            }
        } else {
            std::env::current_dir().into_diagnostic()?
        }
    };

    // Determine patch_dir - use recipe_dir from environment if available and not specified
    let patch_dir = if opts.patch_dir.is_none() {
        if let Some(ref json) = env_dirs {
            if let Some(recipe_dir) = json["recipe_dir"].as_str() {
                tracing::info!(
                    "Using recipe directory from RATTLER_BUILD_DIRECTORIES for patch output: {}",
                    recipe_dir
                );
                Some(PathBuf::from(recipe_dir))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        opts.patch_dir
    };

    match create_patch::create_patch(
        directory,
        &opts.name,
        opts.overwrite,
        patch_dir.as_deref(),
        &exclude_vec,
        &add_vec,
        &include_vec,
        opts.dry_run,
    ) {
        Ok(()) => Ok(()),
        Err(create_patch::GeneratePatchError::PatchFileAlreadyExists(path)) => {
            tracing::warn!("Not writing patch file, already exists: {}", path.display());
            tracing::warn!("Use --overwrite to replace the existing patch file.");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
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
        core_debug::parse_directories_info(opts.work_dir, &opts.output_dir).into_diagnostic()?;

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

    core_debug::add_packages_to_prefix(
        env_name,
        &target_prefix,
        &opts.specs,
        &channels,
        &tool_config,
    )
    .await
}
