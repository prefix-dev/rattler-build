//! This is the main entry point for the `rattler-build` binary.

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::init_logging,
    get_build_output, get_recipe_path,
    opt::{App, ShellCompletion, SubCommands},
    rebuild_from_args,
    recipe_generator::generate_recipe,
    run_build_from_args, run_test_from_args, upload_from_args,
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
        #[cfg(feature = "tui")]
        Some(SubCommands::Build(build_args)) => {
            if build_args.tui {
                let tui = rattler_build::tui::init().await?;
                let log_handler = init_logging(
                    &app.log_style,
                    &app.verbose,
                    &app.color,
                    Some(tui.event_handler.sender.clone()),
                )
                .into_diagnostic()?;
                rattler_build::tui::run(tui, build_args, log_handler).await
            } else {
                let recipe_path = get_recipe_path(&build_args.recipe)?;
                let build_output = get_build_output(
                    build_args,
                    recipe_path,
                    log_handler.expect("logger is not initialized"),
                )
                .await?;
                run_build_from_args(build_output).await
            }
        }
        #[cfg(not(feature = "tui"))]
        Some(SubCommands::Build(build_args)) => {
            let recipe_path = get_recipe_path(&build_args.recipe)?;
            let build_output = get_build_output(
                build_args,
                recipe_path,
                log_handler.expect("logger is not initialized"),
            )
            .await?;
            run_build_from_args(build_output).await
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
