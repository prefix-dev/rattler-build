//! Source-agnostic span information for error reporting
//!
//! This module provides span types that can work with any source format (YAML, TOML, etc.)
//! by abstracting over the specific span implementation.

use std::fmt;

#[cfg(feature = "miette")]
use miette::SourceOffset;

/// A source-agnostic span representing a location in source text
///
/// This abstraction allows us to support multiple source formats (YAML, TOML, etc.)
/// without coupling our error types to a specific parser library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the start of the span
    pub start: usize,
    /// Byte offset of the end of the span (exclusive)
    pub end: usize,
    /// Line number (1-indexed) where the span starts
    pub start_line: usize,
    /// Column number (1-indexed) where the span starts
    pub start_column: usize,
    /// Line number (1-indexed) where the span ends
    pub end_line: usize,
    /// Column number (1-indexed) where the span ends
    pub end_column: usize,
}

impl Span {
    /// Create a new span with full location information
    pub fn new(
        start: usize,
        end: usize,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        Self {
            start,
            end,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    /// Create a span with just byte offsets (line/column info will be computed later if needed)
    pub fn from_offsets(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
        }
    }

    /// Create a blank/unknown span for cases where we don't have location information
    pub fn unknown() -> Self {
        Self {
            start: 0,
            end: 0,
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
        }
    }

    /// Check if this is a blank/unknown span
    pub fn is_unknown(&self) -> bool {
        self.start == 0 && self.end == 0
    }

    /// Get the length of the span in bytes
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Check if the span is empty
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_unknown() {
            write!(f, "<unknown>")
        } else if self.start_line == self.end_line {
            write!(f, "{}:{}", self.start_line, self.start_column)
        } else {
            write!(
                f,
                "{}:{}-{}:{}",
                self.start_line, self.start_column, self.end_line, self.end_column
            )
        }
    }
}

/// Convert our Span to miette::SourceSpan for error reporting
#[cfg(feature = "miette")]
impl From<Span> for miette::SourceSpan {
    fn from(span: Span) -> Self {
        miette::SourceSpan::new(SourceOffset::from(span.start), span.len())
    }
}

/// Convert from marked_yaml::Span to our source-agnostic Span
impl From<marked_yaml::Span> for Span {
    fn from(marked_span: marked_yaml::Span) -> Self {
        let start = marked_span
            .start()
            .map(|m| (m.source(), m.line(), m.column()))
            .unwrap_or((0, 0, 0));
        let end = marked_span
            .end()
            .map(|m| (m.source(), m.line(), m.column()))
            .unwrap_or(start);

        Self::new(start.0, end.0, start.1, start.2, end.1, end.2)
    }
}

/// A string value with associated span information
///
/// This type wraps a String with its location in the source text,
/// allowing for precise error reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedString {
    /// The string value
    value: String,
    /// Location in source where this string came from
    span: Span,
}

impl SpannedString {
    /// Create a new SpannedString
    pub fn new(value: String, span: Span) -> Self {
        Self { value, span }
    }

    /// Create a SpannedString without span information (for testing/synthetic values)
    pub fn without_span(value: String) -> Self {
        Self {
            value,
            span: Span::unknown(),
        }
    }

    /// Get the string value
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Get the span information
    pub fn span(&self) -> Span {
        self.span
    }

    /// Convert to owned String, discarding span information
    pub fn into_string(self) -> String {
        self.value
    }

    /// Get a reference to the inner string
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for SpannedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<String> for SpannedString {
    fn from(value: String) -> Self {
        Self::without_span(value)
    }
}

impl From<&str> for SpannedString {
    fn from(value: &str) -> Self {
        Self::without_span(value.to_string())
    }
}

impl AsRef<str> for SpannedString {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl std::ops::Deref for SpannedString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl std::borrow::Borrow<str> for SpannedString {
    fn borrow(&self) -> &str {
        &self.value
    }
}

/// Convert from marked_yaml::types::MarkedScalarNode to SpannedString
impl From<&marked_yaml::types::MarkedScalarNode> for SpannedString {
    fn from(node: &marked_yaml::types::MarkedScalarNode) -> Self {
        Self::new(node.as_str().to_string(), (*node.span()).into())
    }
}

impl From<marked_yaml::types::MarkedScalarNode> for SpannedString {
    fn from(node: marked_yaml::types::MarkedScalarNode) -> Self {
        Self::from(&node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_creation() {
        let span = Span::new(10, 20, 1, 10, 1, 20);
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
        assert!(!span.is_unknown());
    }

    #[test]
    fn test_span_unknown() {
        let span = Span::unknown();
        assert!(span.is_unknown());
        assert_eq!(span.to_string(), "<unknown>");
    }

    #[test]
    fn test_span_display_single_line() {
        let span = Span::new(10, 20, 5, 10, 5, 20);
        assert_eq!(span.to_string(), "5:10");
    }

    #[test]
    fn test_span_display_multi_line() {
        let span = Span::new(10, 50, 5, 10, 7, 5);
        assert_eq!(span.to_string(), "5:10-7:5");
    }

    #[test]
    fn test_spanned_string_creation() {
        let span = Span::new(0, 5, 1, 1, 1, 5);
        let spanned = SpannedString::new("hello".to_string(), span);
        assert_eq!(spanned.as_str(), "hello");
        assert_eq!(spanned.span(), span);
    }

    #[test]
    fn test_spanned_string_without_span() {
        let spanned = SpannedString::without_span("test".to_string());
        assert_eq!(spanned.as_str(), "test");
        assert!(spanned.span().is_unknown());
    }

    #[test]
    fn test_spanned_string_from_string() {
        let spanned: SpannedString = "hello".into();
        assert_eq!(spanned.as_str(), "hello");
        assert!(spanned.span().is_unknown());
    }

    #[test]
    fn test_spanned_string_deref() {
        let spanned = SpannedString::without_span("hello world".to_string());
        assert_eq!(spanned.len(), 11);
        assert!(spanned.starts_with("hello"));
    }
}
