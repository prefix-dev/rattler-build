use rattler_build_variant_config::{SelectorContext, VariantConfig, load_conda_build_config};
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

    let combinations = config.combinations(&used_vars, None).unwrap();
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

    let combinations = config.combinations(&used_vars, None).unwrap();
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

    let combinations = config.combinations(&used_vars, None).unwrap();
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

    let combinations = config.combinations(&used_vars, None).unwrap();
    insta::assert_yaml_snapshot!("zip_keys_all_combos", combinations);

    // Should be 3 (python/numpy zipped) × 2 (compiler/version zipped) = 6
    assert_eq!(combinations.len(), 6);
}

#[test]
fn test_conda_build_config_linux() {
    let path = test_data_dir().join("conda_build_config/conda_build_config.yaml");
    let context = SelectorContext::new(Platform::Linux64);

    let config = load_conda_build_config(&path, &context).unwrap();
    insta::assert_yaml_snapshot!("conda_build_config_linux", config);
}

#[test]
fn test_conda_build_config_osx() {
    let path = test_data_dir().join("conda_build_config/conda_build_config.yaml");
    let context = SelectorContext::new(Platform::OsxArm64);

    let config = load_conda_build_config(&path, &context).unwrap();
    insta::assert_yaml_snapshot!("conda_build_config_osx", config);
}

#[test]
fn test_conda_build_config_win() {
    let path = test_data_dir().join("conda_build_config/conda_build_config.yaml");
    let context = SelectorContext::new(Platform::Win64);

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

    let combinations = config.combinations(&used_vars, None).unwrap();
    insta::assert_yaml_snapshot!("multi_file_combos", combinations);

    // Should be 2 python/numpy (zipped) × 1 compiler = 2 combinations
    assert_eq!(combinations.len(), 2);
}

#[test]
fn test_filtered_combinations() {
    let path = test_data_dir().join("simple/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());
    used_vars.insert("compiler".into());

    // Filter to only python=3.10
    let mut filter = std::collections::BTreeMap::new();
    filter.insert("python".into(), "3.10".into());

    let combinations = config.combinations(&used_vars, Some(&filter)).unwrap();
    insta::assert_yaml_snapshot!("filtered_python_310_combos", combinations);

    // Should only have 2 combinations (python=3.10 with gcc and clang)
    assert_eq!(combinations.len(), 2);

    // All should have python=3.10
    for combo in &combinations {
        assert_eq!(combo.get(&"python".into()).unwrap().to_string(), "3.10");
    }
}

#[test]
fn test_partial_variable_usage() {
    let path = test_data_dir().join("simple/variants.yaml");
    let config = VariantConfig::from_file(&path).unwrap();

    // Only use python (ignore numpy and compiler)
    let mut used_vars = HashSet::new();
    used_vars.insert("python".into());

    let combinations = config.combinations(&used_vars, None).unwrap();
    insta::assert_yaml_snapshot!("partial_python_only_combos", combinations);

    // Should be 3 combinations (just python variants)
    assert_eq!(combinations.len(), 3);
}

#[cfg(feature = "miette")]
mod error_reporting_tests {
    use super::*;
    use rattler_build_variant_config::VariantConfigError;

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

        // Error message should mention the unsupported type
        assert!(
            err_msg.contains("Unsupported variant value type"),
            "Error message should mention unsupported type, got: {}",
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

        // Should contain "parse error" or "Unsupported"
        assert!(
            error_string.contains("parse error") || error_string.contains("Unsupported"),
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
            err.contains("Unsupported"),
            "Error should mention unsupported type"
        );
    }
}
