//! Error message tests using miette for beautiful diagnostics
//!
//! These tests demonstrate the error messages produced by the parser
//! when encountering various error conditions.

use crate::stage0::parser::{parse_recipe_from_source, parse_recipe_or_multi_from_source};
use crate::{ParseErrorWithSource, source_code::Source};

const TEST_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-data/errors");

/// Macro to format and snapshot miette diagnostic reports
macro_rules! assert_miette_snapshot {
    ($value:expr) => {{
        let mut value = String::new();
        ::miette::GraphicalReportHandler::new_themed(::miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut value, &$value)
            .unwrap();
        ::insta::assert_snapshot!(::insta::_macro_support::AutoName, value, stringify!($value));
    }};
}

fn load_error_test(filename: &str) -> Source {
    let path = format!("{}/{}", TEST_DATA_DIR, filename);
    let contents = fs_err::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test file {}: {}", path, e));
    Source::from_string(filename.to_string(), contents)
}

#[test]
fn test_error_missing_package() {
    let source = load_error_test("missing_package.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_missing_name() {
    let source = load_error_test("missing_name.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_missing_version() {
    let source = load_error_test("missing_version.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_invalid_package_name() {
    let source = load_error_test("invalid_package_name.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_unknown_top_level_field() {
    let source = load_error_test("unknown_top_level_field.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_unknown_about_field() {
    let source = load_error_test("unknown_about_field.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_invalid_jinja() {
    let source = load_error_test("invalid_jinja.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_unknown_requirements_field() {
    let source = load_error_test("unknown_requirements_field.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_invalid_license() {
    let source = load_error_test("error_license.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_invalid_build_number() {
    let source = load_error_test("error.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_invalid_matchspec() {
    let source = load_error_test("error_matchspec.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

// ============================================================================
// Multi-output recipe error tests
// ============================================================================

#[test]
fn test_error_multi_output_missing_outputs() {
    let source = load_error_test("multi_output_missing_outputs.yaml");
    let result = parse_recipe_or_multi_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_multi_output_staging_with_run_requirements() {
    let source = load_error_test("multi_output_staging_with_run.yaml");
    let result = parse_recipe_or_multi_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_multi_output_staging_with_about() {
    let source = load_error_test("multi_output_staging_with_about.yaml");
    let result = parse_recipe_or_multi_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}

#[test]
fn test_error_multi_output_empty_outputs() {
    let source = load_error_test("multi_output_empty_outputs.yaml");
    let result = parse_recipe_or_multi_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    assert_miette_snapshot!(error_with_source);
}
