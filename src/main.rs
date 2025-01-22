//! This is the main entry point for the `rattler-build` binary.

use std::{
    fs::File,
    io::{self, IsTerminal},
};

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    build_recipes,
    console_utils::init_logging,
    get_recipe_path,
    opt::{App, BuildData, ShellCompletion, SubCommands},
    rebuild, run_test, upload_from_args,
};
use tempfile::{tempdir, TempDir};

fn main() -> miette::Result<()> {
    // Initialize sandbox in sync/single-threaded context before tokio runtime
    #[cfg(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        target_os = "macos"
    ))]
    rattler_sandbox::init_sandbox();

    // Create and run the tokio runtime
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { async_main().await })
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

    match app.subcommand {
        Some(SubCommands::Completion(ShellCompletion { shell })) => {
            let mut cmd = App::command();
            fn print_completions<G: clap_complete::Generator>(gen: G, cmd: &mut clap::Command) {
                clap_complete::generate(
                    gen,
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
            let build_data = BuildData::from(build_args);

            // Get all recipe paths and keep tempdir alive until end of the function
            let (recipe_paths, _temp_dir) = recipe_paths(recipes, recipe_dir)?;

            if recipe_paths.is_empty() {
                miette::bail!("Couldn't detect any recipes.")
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
        Some(SubCommands::Test(test_args)) => run_test(test_args.into(), log_handler).await,
        Some(SubCommands::Rebuild(rebuild_args)) => {
            rebuild(
                rebuild_args.into(),
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
        None => {
            _ = App::command().print_long_help();
            Ok(())
        }
    }
}

fn recipe_paths(
    recipes: Vec<std::path::PathBuf>,
    recipe_dir: Option<std::path::PathBuf>,
) -> Result<(Vec<std::path::PathBuf>, Option<TempDir>), miette::Error> {
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
