//! Error message tests using miette for beautiful diagnostics
//!
//! These tests demonstrate the error messages produced by the variant config parser
//! when encountering various error conditions.

#[cfg(feature = "miette")]
use rattler_build_variant_config::VariantConfigError;
#[cfg(feature = "miette")]
use rattler_build_variant_config::yaml_parser::parse_variant_str;
#[cfg(feature = "miette")]
use rattler_build_yaml_parser::ParseErrorWithSource;

#[cfg(feature = "miette")]
use std::sync::Arc;

#[cfg(feature = "miette")]
const TEST_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-data/errors");

/// A simple source code wrapper for error reporting
#[cfg(feature = "miette")]
#[derive(Debug, Clone)]
struct Source {
    name: String,
    code: Arc<str>,
}

#[cfg(feature = "miette")]
impl Source {
    fn from_string(name: String, contents: String) -> Self {
        Self {
            name,
            code: Arc::from(contents.as_str()),
        }
    }
}

#[cfg(feature = "miette")]
impl AsRef<str> for Source {
    fn as_ref(&self) -> &str {
        self.code.as_ref()
    }
}

#[cfg(feature = "miette")]
impl miette::SourceCode for Source {
    fn read_span<'a>(
        &'a self,
        span: &miette::SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        let inner_contents =
            self.as_ref()
                .read_span(span, context_lines_before, context_lines_after)?;
        let contents = miette::MietteSpanContents::new_named(
            self.name.clone(),
            inner_contents.data(),
            *inner_contents.span(),
            inner_contents.line(),
            inner_contents.column(),
            inner_contents.line_count(),
        );
        Ok(Box::new(contents))
    }
}

/// Macro to format and snapshot miette diagnostic reports
#[cfg(feature = "miette")]
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

#[cfg(feature = "miette")]
fn load_error_test(filename: &str) -> Source {
    let path = format!("{}/{}", TEST_DATA_DIR, filename);
    let contents = fs_err::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test file {}: {}", path, e));
    Source::from_string(filename.to_string(), contents)
}

#[cfg(feature = "miette")]
#[test]
fn test_error_with_map() {
    let source = load_error_test("with_map.yaml");
    let result = parse_variant_str(source.as_ref(), None);

    assert!(result.is_err());
    let err = result.unwrap_err();

    // Extract the inner ParseError from VariantConfigError
    if let VariantConfigError::ParseError {
        source: parse_err, ..
    } = err
    {
        let error_with_source = ParseErrorWithSource::new(source, parse_err);
        assert_miette_snapshot!(error_with_source);
    } else {
        panic!("Expected ParseError variant, got: {:?}", err);
    }
}

#[cfg(feature = "miette")]
#[test]
fn test_error_wrong_type() {
    let source = load_error_test("wrong_type.yaml");
    let result = parse_variant_str(source.as_ref(), None);

    assert!(result.is_err());
    let err = result.unwrap_err();

    // Extract the inner ParseError from VariantConfigError
    if let VariantConfigError::ParseError {
        source: parse_err, ..
    } = err
    {
        let error_with_source = ParseErrorWithSource::new(source, parse_err);
        assert_miette_snapshot!(error_with_source);
    } else {
        panic!("Expected ParseError variant, got: {:?}", err);
    }
}

// Note: invalid_jinja.yaml doesn't cause a parse error because Jinja templates
// are stored as strings and validated later during evaluation, not during parsing.

#[cfg(feature = "miette")]
#[test]
fn test_error_zip_keys_error() {
    let source = load_error_test("zip_keys_error.yaml");
    let result = parse_variant_str(source.as_ref(), None);

    assert!(result.is_err());
    let err = result.unwrap_err();

    // Extract the inner ParseError from VariantConfigError
    if let VariantConfigError::ParseError {
        source: parse_err, ..
    } = err
    {
        let error_with_source = ParseErrorWithSource::new(source, parse_err);
        assert_miette_snapshot!(error_with_source);
    } else {
        panic!("Expected ParseError variant, got: {:?}", err);
    }
}
