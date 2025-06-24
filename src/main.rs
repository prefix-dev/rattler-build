//! This is the main entry point for the `rattler-build` binary.

use std::{
    fs::File,
    io::{self, IsTerminal},
    path::PathBuf,
};

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    build_recipes,
    console_utils::init_logging,
    debug_recipe, get_recipe_path,
    opt::{App, BuildData, Config, DebugData, RebuildData, ShellCompletion, SubCommands, TestData},
    rebuild, run_test,
    source::create_patch,
    upload_from_args,
};
use tempfile::{TempDir, tempdir};

fn main() -> miette::Result<()> {
    // Initialize sandbox in sync/single-threaded context before anything else
    #[cfg(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        target_os = "macos"
    ))]
    rattler_sandbox::init_sandbox();

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
        Some(Config::load_from_files(&[config_path]).into_diagnostic()?)
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
            let _ = create_patch::create_patch(
                opts.directory,
                &opts.name,
                opts.overwrite,
                opts.patch_dir.as_deref(),
                &exclude_vec,
                opts.dry_run,
            );
            Ok(())
        }
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
                if entry.path().is_dir() {
                    if let Ok(recipe_path) = get_recipe_path(entry.path()) {
                        recipe_paths.push(recipe_path);
                    }
                }
            }
        }
    }

    Ok((recipe_paths, temp_dir_opt))
}
