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

#[test]
fn test_minimal_recipe_snapshot() {
    let source = load_test_recipe("minimal.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse minimal recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_full_recipe_snapshot() {
    let source = load_test_recipe("full.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse full recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_templates_recipe_snapshot() {
    let source = load_test_recipe("templates.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse templates recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_conditionals_recipe_snapshot() {
    let source = load_test_recipe("conditionals.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse conditionals recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_complex_recipe_snapshot() {
    let source = load_test_recipe("complex.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse complex recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_run_exports_recipe_snapshot() {
    let source = load_test_recipe("run_exports.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse run_exports recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
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

#[test]
fn test_script_parsing_snapshot() {
    let source = load_test_recipe("script.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse script recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_license_files_snapshot() {
    let source = load_test_recipe("license_files.yaml");
    let recipe = parse_recipe_from_source(&source).expect("Failed to parse license_files recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

// ============================================================================
// Multi-output recipe tests
// ============================================================================

#[test]
fn test_multi_output_minimal_snapshot() {
    let source = load_test_recipe("multi_output_minimal.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output minimal recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_multi_output_full_snapshot() {
    let source = load_test_recipe("multi_output_full.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output full recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_multi_output_templates_snapshot() {
    let source = load_test_recipe("multi_output_templates.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output templates recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_multi_output_conditionals_snapshot() {
    let source = load_test_recipe("multi_output_conditionals.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output conditionals recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_multi_output_top_level_inherit_snapshot() {
    let source = load_test_recipe("multi_output_top_level_inherit.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output top-level inherit recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_multi_output_extract_variables() {
    let source = load_test_recipe("multi_output_full.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output full recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

#[test]
fn test_multi_output_templates_extract_variables() {
    let source = load_test_recipe("multi_output_templates.yaml");
    let recipe = parse_recipe_or_multi_from_source(&source)
        .expect("Failed to parse multi-output templates recipe");
    let vars = recipe.used_variables();
    insta::assert_debug_snapshot!(vars);
}

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

    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_nested_conditionals_snapshot() {
    let source = r#"
package:
  name: test-nested-conditionals
  version: "1.0.0"

requirements:
  build:
    - if: unix
      then:
        - gcc
        - if: linux
          then:
            - binutils
          else:
            - llvm
      else:
        - msvc

tests:
  - if: unix
    then:
      - script:
          - echo "Unix test"
  - script:
      - echo "Always runs"
"#;
    let recipe =
        parse_recipe_from_source(source).expect("Failed to parse nested conditionals recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_single_string_files_snapshot() {
    let source = r#"
package:
  name: test-files-string
  version: "1.0.0"

tests:
  - script:
      - pytest
    files:
      recipe: run_test.py
      source:
        - test_data/file1.txt
        - test_data/file2.txt
    requirements:
      run:
        - pytest
  - script:
      - python helper.py
    files:
      recipe:
        - helper.py
        - config.yaml
      source: single_file.txt
"#;
    let recipe =
        parse_recipe_from_source(source).expect("Failed to parse single string files recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}

#[test]
fn test_deeply_nested_conditionals_snapshot() {
    let source = r#"
package:
  name: test-deep-nesting
  version: "1.0.0"

requirements:
  run:
    - if: level1
      then:
        - package-l1
        - if: level2
          then:
            - package-l2
            - if: level3
              then: package-l3
"#;
    let recipe = parse_recipe_from_source(source)
        .expect("Failed to parse deeply nested conditionals recipe");
    insta::assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
}
