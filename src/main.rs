//! This is the main entry point for the `rattler-build` binary.

use std::{
    fs::File,
    io::{self, IsTerminal},
};

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use rattler_build::{
    console_utils::init_logging,
    get_build_output, get_recipe_path, get_tool_config,
    opt::{App, ShellCompletion, SubCommands},
    rebuild_from_args, run_build_from_args, run_test_from_args, skip_noarch,
    sort_build_outputs_topologically, upload_from_args,
};
use tempfile::tempdir;

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
            let mut recipe_paths = Vec::new();
            let temp_dir = tempdir().into_diagnostic()?;
            if !std::io::stdin().is_terminal()
                && build_args.recipe.len() == 1
                && get_recipe_path(&build_args.recipe[0]).is_err()
            {
                let recipe_path = temp_dir.path().join("recipe.yaml");
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

            if recipe_paths.is_empty() {
                miette::bail!("Couldn't detect any recipes.")
            }

            if build_args.tui {
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

                if build_args.render_sources {
                    for output in &outputs {
                        println!("{:?}", output.recipe.source);
                    }
                    return Ok(());
                }

                if build_args.render_only {
                    let outputs = if build_args.with_solve {
                        let mut updated_outputs = Vec::new();
                        for output in outputs {
                            updated_outputs.push(
                                output
                                    .resolve_dependencies(&tool_config)
                                    .await
                                    .into_diagnostic()?,
                            );
                        }
                        updated_outputs
                    } else {
                        outputs
                    };

                    println!(
                        "{}",
                        serde_json::to_string_pretty(&outputs).into_diagnostic()?
                    );
                    return Ok(());
                }

                // Skip noarch builds before the topological sort
                outputs = skip_noarch(outputs, &tool_config).await?;

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
