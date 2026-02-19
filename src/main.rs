//! This is the main entry point for the `rattler-build` binary.

// Use custom allocators for improved performance when the `performance` feature is enabled.
// This must be at the crate root to set the global allocator.
#[cfg(feature = "performance")]
use rattler_build_allocator as _;

use std::{
    fs::File,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    build_recipes, bump_recipe,
    console_utils::{LoggingOutputHandler, init_logging},
    debug_recipe, extract_package, get_recipe_path,
    opt::{
        App, BuildData, BumpRecipeOpts, DebugData, DebugEnvAddOpts, DebugSetupArgs, DebugShellOpts,
        DebugSubCommands, PackageCommands, PublishData, RebuildData, ShellCompletion, SubCommands,
        TestData,
    },
    publish_packages, rebuild, run_test, show_package_info,
    source::create_patch,
    tool_configuration::APP_USER_AGENT,
};
use rattler_conda_types::MatchSpec;
use rattler_config::config::ConfigBase;
use rattler_upload::upload_from_args;
use tempfile::{TempDir, tempdir};

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
fn open_debug_shell(
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
fn debug_shell(opts: DebugShellOpts) -> std::io::Result<()> {
    let (work_dir, directories_json) = parse_directories_info(opts.work_dir, &opts.output_dir)?;
    open_debug_shell(work_dir, directories_json)
}

/// Run the bump-recipe command
async fn run_bump_recipe(opts: BumpRecipeOpts) -> miette::Result<()> {
    // Resolve recipe path
    let recipe_path = get_recipe_path(&opts.recipe)?;

    // Create a simple HTTP client
    let client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .referer(false)
        .build()
        .into_diagnostic()?;

    if opts.check_only {
        // Only check for updates
        match bump_recipe::check_for_updates(&recipe_path, &client, opts.include_prerelease).await {
            Ok(Some(new_version)) => {
                tracing::info!("New version available: {}", new_version);
            }
            Ok(None) => {
                tracing::info!("No new version available");
            }
            Err(e) => {
                return Err(miette::miette!("Failed to check for updates: {}", e));
            }
        }
    } else {
        // Bump the recipe
        match bump_recipe::bump_recipe(
            &recipe_path,
            opts.version.as_deref(),
            &client,
            opts.include_prerelease,
            opts.dry_run,
            opts.keep_build_number,
        )
        .await
        {
            Ok(result) => {
                tracing::debug!("Provider: {:?}", result.provider);
                tracing::debug!(
                    "SHA256 changes: {:?} -> {:?}",
                    result.old_sha256,
                    result.new_sha256
                );
            }
            Err(bump_recipe::BumpRecipeError::NoNewVersion(v)) => {
                tracing::info!("Recipe is already at the latest version ({})", v);
            }
            Err(e) => {
                return Err(miette::miette!("Failed to bump recipe: {}", e));
            }
        }
    }

    Ok(())
}

/// Add packages to a host or build environment in an existing debug build.
///
/// Reads the existing installed packages from the prefix's conda-meta/,
/// combines them with the new specs, resolves, and installs only what changed.
async fn debug_env_add(
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

fn main() -> miette::Result<()> {
    // Stack size varies significantly across platforms:
    // - Windows: only 1MB by default
    // - macOS/Linux: ~8MB by default
    //
    // This discrepancy causes stack overflows primarily on Windows, especially in debug builds
    // To address this, we spawn another main thread (main2) with a consistent
    // larger stack size across all platforms.
    //
    // 4MB is sufficient for most operations while remaining memory-efficient.
    // If needed, developers should/can override with RUST_MIN_STACK environment variable.
    // Further, we preserve error messages from main thread in case something goes wrong.
    const STACK_SIZE: usize = 4 * 1024 * 1024;

    let thread_handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(|| {
            // Create and run the tokio runtime
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async { async_main().await })
        })
        .map_err(|e| miette::miette!("Failed to spawn thread: {}", e))?;

    thread_handle
        .join()
        .map_err(|_| miette::miette!("Thread panicked"))?
}

async fn async_main() -> miette::Result<()> {
    let app = App::parse();
    let log_handler = if !app.is_tui() {
        Some(
            init_logging(
                &app.log_style,
                &app.verbose,
                &app.color,
                app.wrap_log_lines,
                #[cfg(feature = "tui")]
                None,
            )
            .into_diagnostic()?,
        )
    } else {
        #[cfg(not(feature = "tui"))]
        return Err(miette::miette!("tui feature is not enabled!"));
        #[cfg(feature = "tui")]
        None
    };

    let config = if let Some(config_path) = app.config_file {
        Some(ConfigBase::<()>::load_from_files(&[config_path]).into_diagnostic()?)
    } else {
        None
    };

    match app.subcommand {
        Some(SubCommands::Completion(ShellCompletion { shell })) => {
            let mut cmd = App::command();
            fn print_completions<G: clap_complete::Generator>(
                generator: G,
                cmd: &mut clap::Command,
            ) {
                clap_complete::generate(
                    generator,
                    cmd,
                    cmd.get_name().to_string(),
                    &mut std::io::stdout(),
                );
            }

            print_completions(shell, &mut cmd);
            Ok(())
        }
        Some(SubCommands::Build(build_args)) => {
            let recipes = build_args.recipes.clone();
            let recipe_dir = build_args.recipe_dir.clone();
            let build_data = BuildData::from_opts_and_config(build_args, config);

            // Get all recipe paths and keep tempdir alive until end of the function
            let (recipe_paths, _temp_dir) = recipe_paths(recipes, recipe_dir.as_ref())?;
            if recipe_paths.is_empty() {
                if recipe_dir.is_some() {
                    tracing::warn!("No recipes found in recipe directory: {:?}", recipe_dir);
                    return Ok(());
                } else {
                    miette::bail!("Couldn't find recipe.")
                }
            }

            if build_data.tui {
                #[cfg(feature = "tui")]
                {
                    let tui = rattler_build::tui::init().await?;
                    let log_handler = init_logging(
                        &app.log_style,
                        &app.verbose,
                        &app.color,
                        Some(true),
                        Some(tui.event_handler.sender.clone()),
                    )
                    .into_diagnostic()?;
                    rattler_build::tui::run(tui, build_data, recipe_paths, log_handler).await?;
                }
                return Ok(());
            }

            build_recipes(recipe_paths, build_data, &log_handler).await
        }

        Some(SubCommands::Publish(publish_args)) => {
            let publish_data = PublishData::from_opts_and_config(publish_args, config);
            publish_packages(publish_data, &log_handler).await
        }

        Some(SubCommands::Test(test_args)) => {
            run_test(
                TestData::from_opts_and_config(test_args, config),
                log_handler,
            )
            .await
        }
        Some(SubCommands::Rebuild(rebuild_args)) => {
            rebuild(
                RebuildData::from_opts_and_config(rebuild_args, config),
                log_handler.expect("logger is not initialized"),
            )
            .await
        }
        Some(SubCommands::Upload(upload_args)) => upload_from_args(upload_args).await,
        #[cfg(feature = "recipe-generation")]
        Some(SubCommands::GenerateRecipe(args)) => {
            rattler_build::recipe_generator::generate_recipe(args).await
        }
        Some(SubCommands::Auth(args)) => rattler::cli::auth::execute(args).await.into_diagnostic(),
        Some(SubCommands::Debug(args)) => match args.subcommand {
            Some(DebugSubCommands::Shell(opts)) => debug_shell(opts).into_diagnostic(),
            Some(DebugSubCommands::HostAdd(opts)) => {
                debug_env_add("host", opts, config, &log_handler).await
            }
            Some(DebugSubCommands::BuildAdd(opts)) => {
                debug_env_add("build", opts, config, &log_handler).await
            }
            None => {
                // Default: set up debug environment and open shell
                let setup = args.setup.unwrap_or_else(|| DebugSetupArgs {
                    recipe: PathBuf::from("."),
                    output: None,
                    target_platform: None,
                    host_platform: None,
                    build_platform: None,
                    channels: None,
                    common: Default::default(),
                    output_name: None,
                    no_shell: false,
                });
                let no_shell = setup.no_shell;
                let debug_data = DebugData::from_setup_args_and_config(setup, config);
                let output_dir = debug_data.output_dir.clone();
                debug_recipe(debug_data, &log_handler).await?;

                if no_shell {
                    return Ok(());
                }

                // Auto-launch the debug shell using the just-created environment
                let shell_opts = DebugShellOpts {
                    work_dir: None,
                    output_dir,
                };
                debug_shell(shell_opts).into_diagnostic()
            }
        },
        Some(SubCommands::CreatePatch(opts)) => {
            let exclude_vec = opts.exclude.clone().unwrap_or_default();
            let add_vec = opts.add.clone().unwrap_or_default();
            let include_vec = opts.include.clone().unwrap_or_default();

            // Try to parse environment variable if available
            let env_dirs = std::env::var("RATTLER_BUILD_DIRECTORIES")
                .ok()
                .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok());

            // Determine the directory to use
            let directory = if let Some(dir) = opts.directory {
                dir
            } else if let Some(ref json) = env_dirs {
                // Use work_dir from environment variable (set by debug-shell)
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
                // Fall back to current directory
                std::env::current_dir().into_diagnostic()?
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
        Some(SubCommands::Package(cmd)) => match cmd {
            PackageCommands::Inspect(opts) => show_package_info(opts),
            PackageCommands::Extract(opts) => extract_package(opts).await,
        },
        Some(SubCommands::BumpRecipe(opts)) => run_bump_recipe(opts).await,
        None => {
            _ = App::command().print_long_help();
            Ok(())
        }
    }
}

fn recipe_paths(
    recipes: Vec<PathBuf>,
    recipe_dir: Option<&PathBuf>,
) -> Result<(Vec<PathBuf>, Option<TempDir>), miette::Error> {
    let mut recipe_paths = Vec::new();
    let mut temp_dir_opt = None;
    if !std::io::stdin().is_terminal()
        && recipes.len() == 1
        && get_recipe_path(&recipes[0]).is_err()
    {
        let temp_dir = tempdir().into_diagnostic()?;

        let recipe_path = temp_dir.path().join("recipe.yaml");
        io::copy(
            &mut io::stdin(),
            &mut File::create(&recipe_path).into_diagnostic()?,
        )
        .into_diagnostic()?;
        recipe_paths.push(get_recipe_path(&recipe_path)?);
        temp_dir_opt = Some(temp_dir);
    } else {
        for recipe_path in &recipes {
            recipe_paths.push(get_recipe_path(recipe_path)?);
        }
        if let Some(recipe_dir) = &recipe_dir {
            for entry in ignore::Walk::new(recipe_dir) {
                let entry = entry.into_diagnostic()?;
                if entry.path().is_dir()
                    && let Ok(recipe_path) = get_recipe_path(entry.path())
                {
                    recipe_paths.push(recipe_path);
                }
            }
            // Sort to ensure deterministic ordering across platforms/filesystems
            recipe_paths.sort();
        }
    }

    Ok((recipe_paths, temp_dir_opt))
}
