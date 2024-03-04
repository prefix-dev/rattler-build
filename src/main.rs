//! This is the main entry point for the `rattler-build` binary.

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::init_logging,
    opt::{App, ShellCompletion, SubCommands},
    rebuild_from_args,
    recipe_generator::generate_recipe,
    run_build_from_args, run_test_from_args, upload_from_args,
};

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = App::parse();
    match args.subcommand {
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
            if build_args.tui {
                let tui = rattler_build::tui::init().await?;
                let log_handler = init_logging(
                    &args.log_style,
                    &args.verbose,
                    &args.color,
                    Some(tui.event_handler.sender.clone()),
                )
                .into_diagnostic()?;
                tokio::spawn(
                    async move { run_build_from_args(build_args, log_handler).await.unwrap() },
                );
                rattler_build::tui::run(tui).await
            } else {
                let log_handler = init_logging(&args.log_style, &args.verbose, &args.color, None)
                    .into_diagnostic()?;
                run_build_from_args(build_args, log_handler).await
            }
        }
        Some(SubCommands::Test(test_args)) => {
            let log_handler = init_logging(&args.log_style, &args.verbose, &args.color, None)
                .into_diagnostic()?;
            run_test_from_args(test_args, log_handler).await
        }
        Some(SubCommands::Rebuild(rebuild_args)) => {
            let log_handler = init_logging(&args.log_style, &args.verbose, &args.color, None)
                .into_diagnostic()?;
            rebuild_from_args(rebuild_args, log_handler).await
        }
        Some(SubCommands::Upload(args)) => upload_from_args(args).await,
        Some(SubCommands::GenerateRecipe(args)) => generate_recipe(args).await,
        Some(SubCommands::Auth(args)) => rattler::cli::auth::execute(args).await.into_diagnostic(),
        None => {
            _ = App::command().print_long_help();
            Ok(())
        }
    }
}
