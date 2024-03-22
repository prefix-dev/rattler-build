//! This is the main entry point for the `rattler-build` binary.

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::init_logging,
    get_build_output, get_recipe_path,
    opt::{App, ShellCompletion, SubCommands},
    rebuild_from_args,
    recipe_generator::generate_recipe,
    run_build_from_args, run_test_from_args, sort_build_outputs_topologically, upload_from_args,
};

#[tokio::main]
async fn main() -> miette::Result<()> {
    let app = App::parse();
    let log_handler = if !app.is_tui() {
        Some(
            init_logging(
                &app.log_style,
                &app.verbose,
                &app.color,
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
            let shell = shell
                .or(clap_complete::Shell::from_env())
                .unwrap_or(clap_complete::Shell::Bash);
            print_completions(shell, &mut cmd);
            Ok(())
        }
        Some(SubCommands::Build(build_args)) => {
            let mut recipe_paths = Vec::new();
            for recipe_path in &build_args.recipe {
                recipe_paths.push(get_recipe_path(recipe_path)?);
            }
            if let Some(recipe_dir) = &build_args.recipe_dir {
                for entry in ignore::Walk::new(recipe_dir) {
                    let entry = entry.into_diagnostic()?;
                    if entry.path().is_dir() {
                        if let Ok(recipe_path) = get_recipe_path(entry.path()) {
                            recipe_paths.push(recipe_path);
                        }
                    }
                }
            }
            if build_args.tui {
                #[cfg(feature = "tui")]
                {
                    let tui = rattler_build::tui::init().await?;
                    let log_handler = init_logging(
                        &app.log_style,
                        &app.verbose,
                        &app.color,
                        Some(tui.event_handler.sender.clone()),
                    )
                    .into_diagnostic()?;
                    rattler_build::tui::run(tui, build_args, recipe_paths, log_handler).await?;
                }
            } else {
                let log_handler = log_handler.expect("logger is not initialized");
                let mut outputs = Vec::new();
                for recipe_path in &recipe_paths {
                    let output =
                        get_build_output(&build_args, recipe_path.clone(), &log_handler).await?;
                    outputs.push(output);
                }
                let outputs = sort_build_outputs_topologically(&outputs)?;
                for output in outputs {
                    run_build_from_args(output).await?;
                }
            }
            Ok(())
        }
        Some(SubCommands::Test(test_args)) => {
            run_test_from_args(test_args, log_handler.expect("logger is not initialized")).await
        }
        Some(SubCommands::Rebuild(rebuild_args)) => {
            rebuild_from_args(
                rebuild_args,
                log_handler.expect("logger is not initialized"),
            )
            .await
        }
        Some(SubCommands::Upload(upload_args)) => upload_from_args(upload_args).await,
        Some(SubCommands::GenerateRecipe(args)) => generate_recipe(args).await,
        Some(SubCommands::Auth(args)) => rattler::cli::auth::execute(args).await.into_diagnostic(),
        None => {
            _ = App::command().print_long_help();
            Ok(())
        }
    }
}
