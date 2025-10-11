//! Error message tests using miette for beautiful diagnostics
//!
//! These tests demonstrate the error messages produced by the parser
//! when encountering various error conditions.

#[cfg(feature = "miette")]
use crate::stage0::parser::parse_recipe_from_source;
#[cfg(feature = "miette")]
use crate::{ParseError, source_code::Source};

#[cfg(feature = "miette")]
const TEST_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-data/errors");

/// Wrapper that combines a ParseError with its Source for miette reporting
#[cfg(feature = "miette")]
#[derive(Debug, miette::Diagnostic)]
#[diagnostic()]
struct ParseErrorWithSource {
    #[source_code]
    source: Source,

    kind: crate::ErrorKind,

    #[label("{}", message.as_deref().unwrap_or("here"))]
    span: miette::SourceSpan,

    message: Option<String>,

    #[help]
    suggestion: Option<String>,
}

#[cfg(feature = "miette")]
impl std::fmt::Display for ParseErrorWithSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(msg) = &self.message {
            write!(f, ": {}", msg)?;
        }
        Ok(())
    }
}

#[cfg(feature = "miette")]
impl std::error::Error for ParseErrorWithSource {}

#[cfg(feature = "miette")]
impl ParseErrorWithSource {
    fn new(source: Source, error: ParseError) -> Self {
        // Calculate the actual byte offset from line/column information
        // miette needs byte offsets to properly highlight the source
        let span = Self::span_from_line_column(source.as_ref(), &error.span);

        Self {
            source,
            kind: error.kind,
            span,
            message: error.message,
            suggestion: error.suggestion,
        }
    }

    /// Convert line/column information to byte offsets for miette
    fn span_from_line_column(source: &str, span: &crate::Span) -> miette::SourceSpan {
        use miette::SourceOffset;

        // Calculate byte offset from line/column
        let start_offset = SourceOffset::from_location(source, span.start_line, span.start_column);

        let end_offset = if span.end_line > 0 && span.end_column > 0 {
            SourceOffset::from_location(source, span.end_line, span.end_column)
        } else {
            start_offset
        };

        let length = end_offset.offset().saturating_sub(start_offset.offset());
        let length = if length == 0 {
            // Find the length of the token at this position
            Self::find_token_length(source, start_offset.offset())
        } else {
            length
        };

        miette::SourceSpan::new(start_offset, length)
    }

    /// Find the length of a token starting at the given byte offset
    fn find_token_length(src: &str, start: usize) -> usize {
        let remaining = &src[start..];
        let mut len = 0;

        for (i, ch) in remaining.char_indices() {
            if ch.is_whitespace() || ch == ':' || ch == ',' {
                if len == 0 {
                    len = i;
                }
                break;
            }
            len = i + ch.len_utf8();
        }

        if len == 0 { remaining.len() } else { len }
    }
}

#[cfg(feature = "miette")]
fn load_error_test(filename: &str) -> Source {
    let path = format!("{}/{}", TEST_DATA_DIR, filename);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test file {}: {}", path, e));
    Source::from_string(filename.to_string(), contents)
}

#[cfg(feature = "miette")]
fn format_miette_report(error: ParseErrorWithSource) -> String {
    // Format the error using miette's GraphicalReportHandler
    use miette::{GraphicalReportHandler, GraphicalTheme};

    let mut output = String::new();
    let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());

    if let Ok(()) = handler.render_report(&mut output, &error) {
        output
    } else {
        format!("{:?}", error)
    }
}

#[cfg(feature = "miette")]
#[test]
fn test_error_missing_package() {
    let source = load_error_test("missing_package.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_missing_name() {
    let source = load_error_test("missing_name.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_missing_version() {
    let source = load_error_test("missing_version.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_invalid_package_name() {
    let source = load_error_test("invalid_package_name.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_unknown_top_level_field() {
    let source = load_error_test("unknown_top_level_field.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_unknown_about_field() {
    let source = load_error_test("unknown_about_field.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_invalid_jinja() {
    let source = load_error_test("invalid_jinja.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}

#[cfg(feature = "miette")]
#[test]
fn test_error_unknown_requirements_field() {
    let source = load_error_test("unknown_requirements_field.yaml");
    let result = parse_recipe_from_source(source.as_ref());

    assert!(result.is_err());
    let err = result.unwrap_err();

    let error_with_source = ParseErrorWithSource::new(source, err);
    insta::assert_snapshot!(format_miette_report(error_with_source));
}
