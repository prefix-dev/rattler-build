//! This is the main entry point for the `rattler-build` binary.

use std::{
    fs::File,
    io::{self, IsTerminal},
};

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use pixi_config::Config;
use rattler_build::{
    build_recipes,
    console_utils::init_logging,
    debug_recipe, get_recipe_path,
    opt::{
        AllowEmptyBehavior, App, BuildData, DebugData, RebuildData, ShellCompletion, SubCommands,
        TestData,
    },
    rebuild, run_test, upload_from_args,
};
use tempfile::{TempDir, tempdir};
use tokio::fs::read_to_string;

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
        let config_str = read_to_string(&config_path).await.into_diagnostic()?;
        let (config, _unused_keys) =
            Config::from_toml(config_str.as_str(), Some(&config_path.clone()))?;
        Some(config)
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
            let (recipe_paths, _temp_dir) =
                recipe_paths(recipes, recipe_dir, build_data.allow_empty_recipe_dir)?;

            if recipe_paths.is_empty() {
                if build_data.allow_empty_recipe_dir == AllowEmptyBehavior::Warn {
                    tracing::warn!(
                        "No recipes found. Proceeding as per --allow-empty-recipe-dir warn."
                    );
                    return Ok(());
                } else {
                    miette::bail!(
                        "Couldn't detect any recipes. Use --allow-empty-recipe-dir warn to allow this."
                    );
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
        None => {
            _ = App::command().print_long_help();
            Ok(())
        }
    }
}

fn recipe_paths(
    recipes: Vec<std::path::PathBuf>,
    recipe_dir: Option<std::path::PathBuf>,
    allow_empty_recipe_dir: AllowEmptyBehavior,
) -> Result<(Vec<std::path::PathBuf>, Option<TempDir>), miette::Error> {
    let mut recipe_paths_set = std::collections::HashSet::new();
    let mut temp_dir_opt = None;

    let mut handle_recipe_path = |path_arg: &std::path::Path| -> miette::Result<()> {
        match get_recipe_path(path_arg, allow_empty_recipe_dir) {
            Ok(resolved_path) => {
                if resolved_path.is_file() {
                    recipe_paths_set.insert(resolved_path);
                }
                Ok(())
            }
            Err(e) => {
                if allow_empty_recipe_dir == AllowEmptyBehavior::Deny {
                    Err(e)
                } else {
                    Ok(())
                }
            }
        }
    };

    if !std::io::stdin().is_terminal()
        && recipes.len() == 1
        && get_recipe_path(&recipes[0], allow_empty_recipe_dir).is_err()
    {
        let temp_dir = tempdir().into_diagnostic()?;
        let recipe_path = temp_dir.path().join("recipe.yaml");
        io::copy(
            &mut io::stdin(),
            &mut File::create(&recipe_path).into_diagnostic()?,
        )
        .into_diagnostic()?;

        handle_recipe_path(&recipe_path)?;
        temp_dir_opt = Some(temp_dir);
    } else {
        for recipe_path_arg in &recipes {
            handle_recipe_path(recipe_path_arg)?;
        }

        let recipes_found_before_dir_walk = recipe_paths_set.len();

        if let Some(recipe_dir) = &recipe_dir {
            for entry in ignore::Walk::new(recipe_dir) {
                let entry = entry.into_diagnostic()?;
                if entry.file_type().is_some_and(|ft| ft.is_file())
                    && entry.file_name() == "recipe.yaml"
                {
                    let canonical_path = dunce::canonicalize(entry.path()).into_diagnostic()?;
                    recipe_paths_set.insert(canonical_path);
                }
            }

            if recipe_paths_set.len() == recipes_found_before_dir_walk
                && allow_empty_recipe_dir == AllowEmptyBehavior::Deny
            {
                return Err(miette::miette!(
                    "No 'recipe.yaml' files found in the specified recipe directory: {}",
                    recipe_dir.display()
                ));
            }
        }
    }

    let mut recipe_paths: Vec<_> = recipe_paths_set.into_iter().collect();
    recipe_paths.sort();
    Ok((recipe_paths, temp_dir_opt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_err as fs;
    use std::fs::File;
    use tempfile::tempdir;

    fn create_dummy_recipe(path: &std::path::Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        File::create(path).unwrap();
    }

    #[test]
    fn test_recipe_paths_single_file() {
        let dir = tempdir().unwrap();
        let recipe_file = dir.path().join("recipe1.yaml");
        create_dummy_recipe(&recipe_file);
        let expected_path = dunce::canonicalize(&recipe_file).unwrap();

        let (paths, _temp_dir) =
            recipe_paths(vec![recipe_file], None, AllowEmptyBehavior::Deny).unwrap();
        assert_eq!(paths, vec![expected_path]);
    }

    #[test]
    fn test_recipe_paths_multiple_files() {
        let dir = tempdir().unwrap();
        let recipe_file1 = dir.path().join("recipe1.yaml");
        let recipe_file2 = dir.path().join("recipe2.yaml");
        create_dummy_recipe(&recipe_file1);
        create_dummy_recipe(&recipe_file2);
        let expected_path1 = dunce::canonicalize(&recipe_file1).unwrap();
        let expected_path2 = dunce::canonicalize(&recipe_file2).unwrap();

        let mut expected = vec![expected_path1, expected_path2];
        expected.sort();

        let (paths, _temp_dir) = recipe_paths(
            vec![recipe_file1, recipe_file2],
            None,
            AllowEmptyBehavior::Deny,
        )
        .unwrap();
        assert_eq!(paths, expected);
    }

    #[test]
    fn test_recipe_paths_from_dir() {
        let dir = tempdir().unwrap();
        let recipe_dir = dir.path().join("recipes");
        let recipe_in_dir1 = recipe_dir.join("r1/recipe.yaml");
        let recipe_in_dir2 = recipe_dir.join("r2/recipe.yaml");
        create_dummy_recipe(&recipe_in_dir1);
        create_dummy_recipe(&recipe_in_dir2);

        let expected_path1 = dunce::canonicalize(&recipe_in_dir1).unwrap();
        let expected_path2 = dunce::canonicalize(&recipe_in_dir2).unwrap();
        let mut expected = vec![expected_path1, expected_path2];
        expected.sort();

        let (paths, _temp_dir) =
            recipe_paths(vec![], Some(recipe_dir), AllowEmptyBehavior::Deny).unwrap();
        assert_eq!(paths, expected);
    }

    #[test]
    fn test_recipe_paths_files_and_dir() {
        let dir = tempdir().unwrap();
        let recipe_file = dir.path().join("recipe1.yaml");
        create_dummy_recipe(&recipe_file);

        let recipe_dir = dir.path().join("recipes");
        let recipe_in_dir = recipe_dir.join("r1/recipe.yaml");
        create_dummy_recipe(&recipe_in_dir);

        let expected_path1 = dunce::canonicalize(&recipe_file).unwrap();
        let expected_path2 = dunce::canonicalize(&recipe_in_dir).unwrap();
        let mut expected = vec![expected_path1, expected_path2];
        expected.sort();

        let (paths, _temp_dir) = recipe_paths(
            vec![recipe_file],
            Some(recipe_dir),
            AllowEmptyBehavior::Deny,
        )
        .unwrap();
        assert_eq!(paths, expected);
    }

    #[test]
    fn test_recipe_paths_deduplication() {
        let dir = tempdir().unwrap();
        let recipe_file = dir.path().join("recipe1.yaml");
        create_dummy_recipe(&recipe_file);
        let expected_path = dunce::canonicalize(&recipe_file).unwrap();

        let (paths, _temp_dir) = recipe_paths(
            vec![recipe_file.clone(), recipe_file],
            None,
            AllowEmptyBehavior::Deny,
        )
        .unwrap();
        assert_eq!(paths, vec![expected_path]);
    }

    #[test]
    fn test_recipe_paths_empty_dir_deny() {
        let dir = tempdir().unwrap();
        let empty_recipe_dir = dir.path().join("empty_recipes");
        fs::create_dir(&empty_recipe_dir).unwrap();

        let result = recipe_paths(vec![], Some(empty_recipe_dir), AllowEmptyBehavior::Deny);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No 'recipe.yaml' files found")
        );
    }

    #[test]
    fn test_recipe_paths_empty_dir_warn() {
        let dir = tempdir().unwrap();
        let empty_recipe_dir = dir.path().join("empty_recipes");
        fs::create_dir(&empty_recipe_dir).unwrap();

        let (paths, _temp_dir) =
            recipe_paths(vec![], Some(empty_recipe_dir), AllowEmptyBehavior::Warn).unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_recipe_paths_mixed_dir_warn() {
        let dir = tempdir().unwrap();
        let recipe_dir = dir.path().join("recipes");
        let recipe_in_dir = recipe_dir.join("r1/recipe.yaml");
        create_dummy_recipe(&recipe_in_dir);
        let empty_subdir = recipe_dir.join("empty_subdir");
        fs::create_dir(&empty_subdir).unwrap();

        let expected_path = dunce::canonicalize(&recipe_in_dir).unwrap();
        let (paths, _temp_dir) =
            recipe_paths(vec![], Some(recipe_dir), AllowEmptyBehavior::Warn).unwrap();
        assert_eq!(paths, vec![expected_path]);
    }
}
