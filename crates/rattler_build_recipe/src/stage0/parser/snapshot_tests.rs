//! Snapshot tests for recipe parsing using insta
//!
//! These tests parse real recipe files from test-data/ and snapshot the results

use crate::stage0::parser::parse_recipe_from_source;

const TEST_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-data");

fn load_test_recipe(filename: &str) -> String {
    let path = format!("{}/{}", TEST_DATA_DIR, filename);
    fs_err::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test file {}: {}", path, e))
}

#[test]
fn test_minimal_recipe_snapshot() {
    let source = load_test_recipe("minimal.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse minimal recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[test]
fn test_full_recipe_snapshot() {
    let source = load_test_recipe("full.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse full recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[test]
fn test_templates_recipe_snapshot() {
    let source = load_test_recipe("templates.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse templates recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[test]
fn test_conditionals_recipe_snapshot() {
    let source = load_test_recipe("conditionals.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse conditionals recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[test]
fn test_complex_recipe_snapshot() {
    let source = load_test_recipe("complex.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse complex recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[test]
fn test_run_exports_recipe_snapshot() {
    let source = load_test_recipe("run_exports.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse run_exports recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[test]
fn test_templates_extract_variables() {
    let source = load_test_recipe("templates.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse templates recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[test]
fn test_conditionals_extract_variables() {
    let source = load_test_recipe("conditionals.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse conditionals recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[test]
fn test_complex_extract_variables() {
    let source = load_test_recipe("complex.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse complex recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}
