//! This is the main entry point for the `rattler-build` binary.

// Use custom allocators for improved performance when the `performance` feature is enabled.
// This must be at the crate root to set the global allocator.
#[cfg(feature = "performance")]
use rattler_build_allocator as _;

use std::{
    fs::File,
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
};

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    build_recipes, bump_recipe,
    console_utils::init_logging,
    debug_recipe, extract_package, get_recipe_path,
    opt::{
        App, BuildData, BumpRecipeOpts, DebugData, DebugShellOpts, PackageCommands, PublishData,
        RebuildData, ShellCompletion, SubCommands, TestData,
    },
    publish_packages, rebuild, run_test, show_package_info,
    source::create_patch,
};
use rattler_config::config::ConfigBase;
use rattler_upload::upload_from_args;
use tempfile::{TempDir, tempdir};

/// Open a debug shell in the build environment
fn debug_shell(opts: DebugShellOpts) -> std::io::Result<()> {
    // Parse the directories info from the log file
    let (work_dir, directories_json) = if let Some(dir) = opts.work_dir {
        (dir, None)
    } else {
        // Read from rattler-build-log.txt
        let log_file = opts.output_dir.join("rattler-build-log.txt");
        if !log_file.exists() {
            eprintln!(
                "Error: Could not find rattler-build-log.txt at {}",
                log_file.display()
            );
            eprintln!("Please specify --work-dir or run a build first.");
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
        let (work_dir, json_data) =
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(last_line) {
                let work_dir = json["work_dir"].as_str().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "work_dir not found in JSON",
                    )
                })?;
                (PathBuf::from(work_dir), Some(json))
            } else {
                // Old format: plain path
                (PathBuf::from(last_line.trim()), None)
            };

        (work_dir, json_data)
    };

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

    println!("Opening debug shell in: {}", work_dir.display());
    println!("The build environment will be sourced automatically.");
    if directories_json.is_some() {
        println!("Build directories info available in RATTLER_BUILD_DIRECTORIES");
    }
    println!("Exit the shell with 'exit' or Ctrl+D to return.\n");

    // Determine the shell to use
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    // Build environment variable exports
    let mut env_exports = String::new();
    if let Some(ref json) = directories_json {
        // Export the full JSON as RATTLER_BUILD_DIRECTORIES
        env_exports.push_str(&format!(
            "export RATTLER_BUILD_DIRECTORIES='{}'\n",
            serde_json::to_string(json).unwrap_or_default()
        ));

        // Export individual directories for convenience
        if let Some(recipe_path) = json["recipe_path"].as_str() {
            env_exports.push_str(&format!(
                "export RATTLER_BUILD_RECIPE_PATH='{}'\n",
                recipe_path
            ));
        }
        if let Some(recipe_dir) = json["recipe_dir"].as_str() {
            env_exports.push_str(&format!(
                "export RATTLER_BUILD_RECIPE_DIR='{}'\n",
                recipe_dir
            ));
        }
        if let Some(build_dir) = json["build_dir"].as_str() {
            env_exports.push_str(&format!("export RATTLER_BUILD_BUILD_DIR='{}'\n", build_dir));
        }
        if let Some(output_dir) = json["output_dir"].as_str() {
            env_exports.push_str(&format!(
                "export RATTLER_BUILD_OUTPUT_DIR='{}'\n",
                output_dir
            ));
        }
    }

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

/// Run the bump-recipe command
async fn run_bump_recipe(opts: BumpRecipeOpts) -> miette::Result<()> {
    // Resolve recipe path
    let recipe_path = get_recipe_path(&opts.recipe)?;

    // Create a simple HTTP client
    let client = reqwest::Client::builder()
        .user_agent("rattler-build")
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
        Some(SubCommands::Debug(opts)) => {
            let debug_data = DebugData::from_opts_and_config(opts, config);
            debug_recipe(debug_data, &log_handler).await?;
            Ok(())
        }
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
        Some(SubCommands::DebugShell(opts)) => debug_shell(opts).into_diagnostic(),
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
        }
    }

    Ok((recipe_paths, temp_dir_opt))
}
