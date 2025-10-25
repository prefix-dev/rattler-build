use rattler_build_jinja::JinjaConfig;
use rattler_build_variant_config::{VariantConfig, load_conda_build_config};
use rattler_conda_types::Platform;
use std::collections::HashSet;
use std::path::PathBuf;

fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data")
}

#[test]
fn test_simple_variant_config() {
    let path = test_data_dir().join("simple/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    insta::assert_yaml_snapshot!("simple_config", config);
}

#[test]
fn test_simple_combinations() {
    let path = test_data_dir().join("simple/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    // Test with just python and numpy
    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());
    used_vars.insert("numpy".into());

    let combinations = config.combinations(&used_vars).unwrap();
    insta::assert_yaml_snapshot!("simple_python_numpy_combos", combinations);
}

#[test]
fn test_simple_all_combinations() {
    let path = test_data_dir().join("simple/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    // Test with all variables
    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());
    used_vars.insert("numpy".into());
    used_vars.insert("compiler".into());

    let combinations = config.combinations(&used_vars).unwrap();
    insta::assert_yaml_snapshot!("simple_all_combos", combinations);

    // Should be 3 python × 3 numpy × 2 compiler = 18 combinations
    assert_eq!(combinations.len(), 18);
}

#[test]
fn test_zip_keys_config() {
    let path = test_data_dir().join("with_zip_keys/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    insta::assert_yaml_snapshot!("zip_keys_config", config);
}

#[test]
fn test_zip_keys_combinations() {
    let path = test_data_dir().join("with_zip_keys/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    // Test with python and numpy (should be zipped)
    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());
    used_vars.insert("numpy".into());

    let combinations = config.combinations(&used_vars).unwrap();
    insta::assert_yaml_snapshot!("zip_keys_python_numpy_combos", combinations);

    // Should be 3 combinations (zipped), not 9
    assert_eq!(combinations.len(), 3);
}

#[test]
fn test_zip_keys_all_combinations() {
    let path = test_data_dir().join("with_zip_keys/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    // Test with all variables (two zip groups)
    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());
    used_vars.insert("numpy".into());
    used_vars.insert("compiler".into());
    used_vars.insert("compiler_version".into());

    let combinations = config.combinations(&used_vars).unwrap();
    insta::assert_yaml_snapshot!("zip_keys_all_combos", combinations);

    // Should be 3 (python/numpy zipped) × 2 (compiler/version zipped) = 6
    assert_eq!(combinations.len(), 6);
}

#[test]
fn test_conda_build_config_linux() {
    let path = test_data_dir().join("conda_build_config/conda_build_config.yaml");
    let context = JinjaConfig {
        target_platform: Platform::Linux64,
        ..Default::default()
    };

    let config = load_conda_build_config(&path, &context).unwrap();
    insta::assert_yaml_snapshot!("conda_build_config_linux", config);
}

#[test]
fn test_conda_build_config_osx() {
    let path = test_data_dir().join("conda_build_config/conda_build_config.yaml");
    let context = JinjaConfig {
        target_platform: Platform::OsxArm64,
        ..Default::default()
    };

    let config = load_conda_build_config(&path, &context).unwrap();
    insta::assert_yaml_snapshot!("conda_build_config_osx", config);
}

#[test]
fn test_conda_build_config_win() {
    let path = test_data_dir().join("conda_build_config/conda_build_config.yaml");
    let context = JinjaConfig {
        target_platform: Platform::Win64,
        ..Default::default()
    };

    let config = load_conda_build_config(&path, &context).unwrap();
    insta::assert_yaml_snapshot!("conda_build_config_win", config);
}

#[test]
fn test_multi_file_merge() {
    let base = test_data_dir().join("multi_file/base.yaml");
    let override_file = test_data_dir().join("multi_file/override.yaml");

    let config = VariantConfig::from_files(&[base, override_file]).unwrap();
    insta::assert_yaml_snapshot!("multi_file_merged", config);

    // Python should be overridden to ["3.11", "3.12"]
    let python_variants = config.get(&"python".into()).unwrap();
    assert_eq!(python_variants.len(), 2);
    assert_eq!(python_variants[0].to_string(), "3.11");
}

#[test]
fn test_multi_file_combinations() {
    let base = test_data_dir().join("multi_file/base.yaml");
    let override_file = test_data_dir().join("multi_file/override.yaml");

    let config = VariantConfig::from_files(&[base, override_file]).unwrap();

    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());
    used_vars.insert("numpy".into());
    used_vars.insert("compiler".into());

    let combinations = config.combinations(&used_vars).unwrap();
    insta::assert_yaml_snapshot!("multi_file_combos", combinations);

    // Should be 2 python/numpy (zipped) × 1 compiler = 2 combinations
    assert_eq!(combinations.len(), 2);
}

#[test]
fn test_partial_variable_usage() {
    let path = test_data_dir().join("simple/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    // Only use python (ignore numpy and compiler)
    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());

    let combinations = config.combinations(&used_vars).unwrap();
    insta::assert_yaml_snapshot!("partial_python_only_combos", combinations);

    // Should be 3 combinations (just python variants)
    assert_eq!(combinations.len(), 3);
}

#[test]
fn test_flatten_selectors_linux() {
    let path = test_data_dir().join("with_selectors/variants.yaml");
    let jinja_config = rattler_build_jinja::JinjaConfig {
        target_platform: Platform::Linux64,
        build_platform: Platform::Linux64,
        ..Default::default()
    };

    let config = VariantConfig::from_file_with_context(&path, &jinja_config).unwrap();

    // Verify that conditionals were evaluated for Unix
    // unix_level should be present (unix=true for Unix)
    assert!(config.variants.contains_key(&"unix_level".into()));
    assert!(config.variants.contains_key(&"c_compiler".into()));

    insta::assert_yaml_snapshot!("flatten_selectors_linux", config);
}

#[test]
fn test_flatten_selectors_win() {
    let path = test_data_dir().join("with_selectors/variants.yaml");
    let jinja_config = rattler_build_jinja::JinjaConfig {
        target_platform: Platform::Win64,
        build_platform: Platform::Win64,
        ..Default::default()
    };

    let config = VariantConfig::from_file_with_context(&path, &jinja_config).unwrap();

    // Verify that conditionals were evaluated for Windows
    // unix_level should NOT be present (unix=false for Windows)
    assert!(!config.variants.contains_key(&"unix_level".into()));

    // c_compiler should have vs2019 (win selector matched)
    assert!(config.variants.contains_key(&"c_compiler".into()));

    insta::assert_yaml_snapshot!("flatten_selectors_win", config);
}

#[test]
fn test_load_conda_build_config_with_types() {
    let path = test_data_dir().join("variant_files/variant_config_1.yaml");
    let context = JinjaConfig {
        target_platform: Platform::Linux64,
        ..Default::default()
    };

    let config = load_conda_build_config(&path, &context).unwrap();

    // Test that values are handled correctly by load_conda_build_config
    // load_conda_build_config quotes numeric values to preserve version numbers
    // So both "5" and 5 become the string "5"
    assert_eq!(
        config.variants.get(&"noboolean".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from("true")]
    );

    assert_eq!(
        config.variants.get(&"boolean".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from(true)]
    );

    assert_eq!(
        config.variants.get(&"nointeger".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from_string("5")]
    );

    // load_conda_build_config quotes numeric values in lists
    assert_eq!(
        config.variants.get(&"integer".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from_string("5")]
    );

    insta::assert_yaml_snapshot!("load_conda_build_config_types", config);
}

#[test]
fn test_load_variant_config_with_types() {
    let path = test_data_dir().join("variant_files/variant_config_1.yaml");

    let jinja_config = rattler_build_jinja::JinjaConfig {
        target_platform: Platform::Linux64,
        ..Default::default()
    };
    let config = VariantConfig::from_file_with_context(&path, &jinja_config).unwrap();

    // Test that values are handled correctly by load_conda_build_config
    // load_conda_build_config quotes numeric values to preserve version numbers
    // So both "5" and 5 become the string "5"
    assert_eq!(
        config.variants.get(&"noboolean".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from("true")]
    );

    assert_eq!(
        config.variants.get(&"boolean".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from(true)]
    );

    assert_eq!(
        config.variants.get(&"nointeger".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from_string("5")]
    );

    assert_eq!(
        config.variants.get(&"integer".into()).unwrap(),
        &vec![rattler_build_jinja::Variable::from(5)]
    );

    insta::assert_yaml_snapshot!("load_variant_config_types", config);
}

#[test]
fn test_variant_combinations_with_zip_and_filter() {
    let mut config = VariantConfig::new();
    config.insert("a", vec!["1".into(), "2".into()]);
    config.insert("b", vec!["3".into(), "4".into()]);
    config.zip_keys = Some(vec![vec!["a".into(), "b".into()]]);

    // Test with just 'a' used
    let used_vars = vec!["a".into()].into_iter().collect();
    let combinations = config.combinations(&used_vars).unwrap();
    assert_eq!(combinations.len(), 2);

    // Test with both 'a' and 'b' used
    let used_vars = vec!["a".into(), "b".into()].into_iter().collect();
    let combinations = config.combinations(&used_vars).unwrap();
    assert_eq!(combinations.len(), 2);

    // Add 'c' variable
    config.insert("c", vec!["5".into(), "6".into(), "7".into()]);
    let used_vars = vec!["a".into(), "b".into(), "c".into()]
        .into_iter()
        .collect();
    let combinations = config.combinations(&used_vars).unwrap();
    assert_eq!(combinations.len(), 2 * 3); // 2 zipped pairs × 3 c values

    // Test without zip_keys (full cartesian product)
    config.zip_keys = None;
    let combinations = config.combinations(&used_vars).unwrap();
    assert_eq!(combinations.len(), 2 * 2 * 3); // 2a × 2b × 3c
}

#[test]
fn test_zip_keys_validation_flat_list() {
    // Test that invalid zip_keys (flat list) fails validation
    let yaml = r#"
zip_keys: [python, compiler]
python:
  - "3.9"
  - "3.10"
compiler:
  - gcc
  - clang
"#;
    let jinja_config = rattler_build_jinja::JinjaConfig {
        target_platform: Platform::Linux64,
        ..Default::default()
    };
    let result = VariantConfig::from_yaml_str_with_context(yaml, &jinja_config);

    // The parser should reject this structure
    assert!(
        result.is_err(),
        "Expected flat zip_keys list to fail parsing"
    );
}

#[test]
fn test_zip_keys_validation_nested_list() {
    // Test that valid zip_keys (list of lists) succeeds
    let yaml = r#"
zip_keys:
  - [python, compiler]
python:
  - "3.9"
  - "3.10"
compiler:
  - gcc
  - clang
"#;
    let jinja_config = rattler_build_jinja::JinjaConfig {
        target_platform: Platform::Linux64,
        ..Default::default()
    };
    let result = VariantConfig::from_yaml_str_with_context(yaml, &jinja_config);

    assert!(
        result.is_ok(),
        "Expected nested zip_keys list to succeed: {:?}",
        result.err()
    );
    let config = result.unwrap();

    // Verify the structure
    assert!(config.zip_keys.is_some());
    assert_eq!(config.zip_keys.as_ref().unwrap().len(), 1);
    assert_eq!(config.zip_keys.as_ref().unwrap()[0].len(), 2);
}

#[cfg(feature = "miette")]
mod error_reporting_tests {
    use super::*;

    #[test]
    fn test_unsupported_value_type_error() {
        let yaml = r#"
python:
  - 3.10
  - 3.11
  - map: 123
"#;
        let result = VariantConfig::from_yaml_str(yaml);

        // Should fail with ParseError
        assert!(result.is_err());
        let err_msg = result.unwrap_err();

        // Error message should mention the type mismatch (scalar expected)
        assert!(
            err_msg.contains("scalar") || err_msg.contains("Unsupported variant value type"),
            "Error message should mention type issue, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_invalid_conditional_structure() {
        let yaml = r#"
python:
  - if: true
    # Missing 'then' key - this should fail
"#;
        let result = VariantConfig::from_yaml_str(yaml);

        // Should fail with ParseError
        assert!(result.is_err());
        let err_msg = result.unwrap_err();

        // Error message should mention the missing 'then' key
        assert!(
            err_msg.contains("then") || err_msg.contains("Conditional"),
            "Error message should mention conditional structure issue, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_invalid_structure_error() {
        let yaml = r#"
python: "not a list"
"#;
        let result = VariantConfig::from_yaml_str(yaml);

        // Should fail with ParseError
        assert!(result.is_err());
        let err_msg = result.unwrap_err();

        // Error message should mention list expectation
        assert!(
            err_msg.contains("list") || err_msg.contains("sequence"),
            "Error message should mention list requirement, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_parse_error_structure() {
        let yaml = r#"
python:
  - 3.10
  - map: 123
"#;
        let result = VariantConfig::from_yaml_str(yaml);

        // Should fail
        assert!(result.is_err());
        let err_msg = result.unwrap_err();

        // Verify the error message structure
        let error_string = err_msg.to_string();

        // Should contain "parse error", "scalar", or "Unsupported"
        assert!(
            error_string.contains("parse error")
                || error_string.contains("Unsupported")
                || error_string.contains("scalar"),
            "Error should indicate parsing issue, got: {}",
            error_string
        );
    }

    #[test]
    fn test_error_preserves_span_info() {
        let yaml = r#"python:
  - 3.10
  - map: 123
"#;
        let result = VariantConfig::from_yaml_str(yaml);
        assert!(result.is_err());

        // The error should be a String from from_yaml_str
        // but the underlying ParseError should have span information
        let err = result.unwrap_err();

        // Just verify we get an error message
        assert!(!err.is_empty(), "Error message should not be empty");
        assert!(
            err.contains("Unsupported") || err.contains("scalar"),
            "Error should mention type issue"
        );
    }
}
