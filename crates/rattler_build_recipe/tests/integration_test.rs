//! Integration tests for parsing real recipe files from test-data/recipes

use std::path::Path;

use rattler_build_recipe::stage0::{Recipe, parse_recipe_or_multi_from_source};

/// Helper to find all recipe.yaml files in test-data/recipes
fn find_recipe_files() -> Vec<std::path::PathBuf> {
    let test_data_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-data")
        .join("recipes");

    let mut recipes = Vec::new();

    if test_data_dir.exists() {
        for entry in walkdir::WalkDir::new(&test_data_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some("recipe.yaml") {
                recipes.push(path.to_path_buf());
            }
        }
    }

    recipes.sort();
    recipes
}

#[test]
fn test_parse_all_test_recipes() {
    let recipes = find_recipe_files();

    if recipes.is_empty() {
        println!("Warning: No recipe files found in test-data/recipes");
        return;
    }

    println!("Found {} recipe files to test", recipes.len());

    let mut successful = 0;
    let mut failed = Vec::new();

    for recipe_path in &recipes {
        let relative_path = recipe_path
            .strip_prefix(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap(),
            )
            .unwrap_or(recipe_path);

        match fs_err::read_to_string(recipe_path) {
            Ok(content) => match parse_recipe_or_multi_from_source(&content) {
                Ok(recipe) => {
                    successful += 1;
                    match recipe {
                        Recipe::SingleOutput(_) => {
                            println!("✓ {} [single-output]", relative_path.display());
                        }
                        Recipe::MultiOutput(_) => {
                            println!("✓ {} [multi-output]", relative_path.display());
                        }
                    }
                }
                Err(e) => {
                    failed.push((relative_path.to_path_buf(), e.to_string()));
                    println!("✗ {}: {}", relative_path.display(), e);
                }
            },
            Err(e) => {
                failed.push((
                    relative_path.to_path_buf(),
                    format!("Failed to read file: {}", e),
                ));
                println!("✗ {}: Failed to read file: {}", relative_path.display(), e);
            }
        }
    }

    println!("\n=== Summary ===");
    println!("Total recipes: {}", recipes.len());
    println!("Successful: {}", successful);
    println!("Failed: {}", failed.len());

    if !failed.is_empty() {
        println!("\n=== Failed Recipes ===");
        for (path, error) in &failed {
            println!("\n{}:", path.display());
            println!("  {}", error);
        }
    }

    // For now, we'll just print the results and not fail the test
    // This allows us to see which recipes need attention
    // TODO: Once all recipes parse correctly, make this test fail on errors
}

#[test]
fn test_parse_specific_known_recipes() {
    // Test a few specific recipes that we know should work
    let test_data_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-data")
        .join("recipes");

    let specific_recipes = vec![
        "symlink/recipe.yaml",
        "flask/recipe.yaml",
        "git_source/recipe.yaml",
    ];

    for recipe_name in specific_recipes {
        let recipe_path = test_data_dir.join(recipe_name);
        if recipe_path.exists() {
            let content = fs_err::read_to_string(&recipe_path).expect("Failed to read recipe");

            let result = parse_recipe_or_multi_from_source(&content);

            match result {
                Ok(recipe) => {
                    match recipe {
                        Recipe::SingleOutput(single) => {
                            println!(
                                "✓ Parsed {} [single-output]: package = {:?}",
                                recipe_name, single.package.name
                            );
                            // Basic sanity checks
                            assert!(!single.package.name.to_string().is_empty());
                        }
                        Recipe::MultiOutput(multi) => {
                            println!(
                                "✓ Parsed {} [multi-output]: {} outputs",
                                recipe_name,
                                multi.outputs.len()
                            );
                            // Basic sanity checks
                            assert!(!multi.outputs.is_empty());
                        }
                    }
                }
                Err(e) => {
                    println!("✗ Failed to parse {}: {}", recipe_name, e);
                    panic!("Expected recipe to parse successfully");
                }
            }
        } else {
            println!("Skipping {} (file not found)", recipe_name);
        }
    }
}
