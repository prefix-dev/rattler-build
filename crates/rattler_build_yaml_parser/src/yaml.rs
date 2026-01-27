//! Custom YAML parsing with proper quote handling
//!
//! This module provides a wrapper around marked_yaml that enables `prevent_coercion(true)`
//! to ensure that quoted values like "123" and "true" are treated as strings, not integers/booleans.

use marked_yaml::{LoadError, LoaderOptions, Node, parse_yaml_with_options};

/// Parse YAML from a string with proper quote handling
///
/// This function uses `prevent_coercion(true)` to ensure that:
/// - `foo: "123"` is parsed as string "123", not integer 123
/// - `bar: "true"` is parsed as string "true", not boolean true
/// - `baz: 123` is still parsed as integer 123
/// - `qux: true` is still parsed as boolean true
///
/// This is critical for correct variable type handling in context variables.
pub fn parse_yaml(source: &str) -> Result<Node, LoadError> {
    let options = LoaderOptions::default()
        .error_on_duplicate_keys(true)
        .prevent_coercion(true);

    parse_yaml_with_options(0, source, options)
}
