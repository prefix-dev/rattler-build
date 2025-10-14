//! Snapshot tests for recipe parsing using insta
//!
//! These tests parse real recipe files from test-data/ and snapshot the results

use crate::stage0::parser::{parse_recipe_from_source, parse_recipe_or_multi_from_source};

const TEST_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-data");

fn load_test_recipe(filename: &str) -> String {
    let path = format!("{}/{}", TEST_DATA_DIR, filename);
    fs_err::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test file {}: {}", path, e))
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_minimal_recipe_snapshot() {
    let source = load_test_recipe("minimal.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse minimal recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_full_recipe_snapshot() {
    let source = load_test_recipe("full.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse full recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_templates_recipe_snapshot() {
    let source = load_test_recipe("templates.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse templates recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_conditionals_recipe_snapshot() {
    let source = load_test_recipe("conditionals.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse conditionals recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_complex_recipe_snapshot() {
    let source = load_test_recipe("complex.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse complex recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_run_exports_recipe_snapshot() {
    let source = load_test_recipe("run_exports.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse run_exports recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_templates_extract_variables() {
    let source = load_test_recipe("templates.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse templates recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_conditionals_extract_variables() {
    let source = load_test_recipe("conditionals.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse conditionals recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_complex_extract_variables() {
    let source = load_test_recipe("complex.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse complex recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_script_parsing_snapshot() {
    let source = load_test_recipe("script.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse script recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_license_files_snapshot() {
    let source = load_test_recipe("license_files.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse license_files recipe");
    insta::assert_debug_snapshot!(recipe);
}

// ============================================================================
// Multi-output recipe tests
// ============================================================================

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_minimal_snapshot() {
    let source = load_test_recipe("multi_output_minimal.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output minimal recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_full_snapshot() {
    let source = load_test_recipe("multi_output_full.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output full recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_templates_snapshot() {
    let source = load_test_recipe("multi_output_templates.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output templates recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_conditionals_snapshot() {
    let source = load_test_recipe("multi_output_conditionals.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output conditionals recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_top_level_inherit_snapshot() {
    let source = load_test_recipe("multi_output_top_level_inherit.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output top-level inherit recipe");
    insta::assert_debug_snapshot!(recipe);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_extract_variables() {
    let source = load_test_recipe("multi_output_full.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output full recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_multi_output_templates_extract_variables() {
    let source = load_test_recipe("multi_output_templates.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output templates recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn test_single_output_compatibility() {
    // Test that single-output recipes still work with new parser
    let source = load_test_recipe("minimal.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse single-output recipe with multi parser");

    // Verify it's parsed as SingleOutput variant
    match recipe {
        crate::stage0::Recipe::SingleOutput(_) => {}
        crate::stage0::Recipe::MultiOutput(_) => panic!("Expected SingleOutput, got MultiOutput"),
    }

    insta::assert_debug_snapshot!(recipe);
}
