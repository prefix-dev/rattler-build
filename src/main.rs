//! This is the main entry point for the `rattler-build` binary.

use std::{
    env,
    fs::{self, File},
    io::{self, IsTerminal},
};

use clap::{CommandFactory, Parser};
use clap_markdown::print_help_markdown;
use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::init_logging,
    get_build_output, get_recipe_path, get_tool_config,
    opt::{App, ShellCompletion, SubCommands},
    rebuild_from_args,
    recipe_generator::generate_recipe,
    run_build_from_args, run_test_from_args, sort_build_outputs_topologically, upload_from_args,
    utils::get_current_timestamp,
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

    if app.markdown_help {
        print_help_markdown::<App>();
        return Ok(());
    }

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
            if !std::io::stdin().is_terminal()
                && build_args.recipe.len() == 1
                && get_recipe_path(&build_args.recipe[0]).is_err()
            {
                let package_name =
                    format!("{}-{}", env!("CARGO_PKG_NAME"), get_current_timestamp()?);
                let temp_dir = env::temp_dir().join(package_name);
                fs::create_dir(&temp_dir).into_diagnostic()?;
                let recipe_path = temp_dir.join("recipe.yaml");
                io::copy(
                    &mut io::stdin(),
                    &mut File::create(&recipe_path).into_diagnostic()?,
                )
                .into_diagnostic()?;
                recipe_paths.push(get_recipe_path(&recipe_path)?);
            } else {
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
                let tool_config = get_tool_config(&build_args, &log_handler)?;
                let mut outputs = Vec::new();
                for recipe_path in &recipe_paths {
                    let output = get_build_output(&build_args, recipe_path, &tool_config).await?;
                    outputs.extend(output);
                }

                if build_args.render_only {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&outputs).into_diagnostic()?
                    );
                    return Ok(());
                }

                sort_build_outputs_topologically(&mut outputs, build_args.up_to.as_deref())?;
                run_build_from_args(outputs, tool_config).await?;
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
