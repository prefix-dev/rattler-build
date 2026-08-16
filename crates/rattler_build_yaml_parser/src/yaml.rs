//! Custom YAML parsing with proper quote handling
//!
//! This module provides a wrapper around marked_yaml that enables `prevent_coercion(true)`
//! to ensure that quoted values like "123" and "true" are treated as strings, not integers/booleans.

use marked_yaml::{LoadError, LoaderOptions, Node, Span, parse_yaml_with_options};

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

/// Extract the source [`Span`] from a marked_yaml [`LoadError`].
///
/// Every `LoadError` variant carries location information describing *where* in
/// the source the problem occurred (a [`marked_yaml::Marker`], or, for duplicate
/// keys, the offending scalar node). This converts that location into a `Span`
/// so the error can be reported at the offending line/column instead of
/// defaulting to the top of the file.
///
/// This is particularly important for errors triggered by malformed Jinja, e.g.
/// a `{{ var }}` that should have been `${{ var }}` parses as a YAML flow
/// mapping and yields a "Keys in mappings must be scalar" error whose marker
/// points at the actual mistake.
pub fn load_error_span(error: &LoadError) -> Span {
    match error {
        LoadError::TopLevelMustBeMapping(marker)
        | LoadError::TopLevelMustBeSequence(marker)
        | LoadError::UnexpectedAnchor(marker)
        | LoadError::MappingKeyMustBeScalar(marker)
        | LoadError::UnexpectedTag(marker)
        | LoadError::ScanError(marker, _) => Span::new_start(*marker),
        LoadError::DuplicateKey(inner) => *inner.key.span(),
    }
}
