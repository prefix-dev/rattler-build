//! Error types for recipe parsing with span information

use std::fmt;

use crate::span::Span;

/// Result type for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;

/// Error during recipe parsing with span information for excellent error messages
#[cfg_attr(feature = "miette", derive(thiserror::Error, miette::Diagnostic))]
#[cfg_attr(feature = "miette", error("{kind}"))]
#[derive(Debug, Clone)]
pub struct ParseError {
    /// The kind of error that occurred
    pub kind: ErrorKind,
    /// Location in source where the error occurred
    #[cfg_attr(feature = "miette", label("{}", message.as_deref().unwrap_or("here")))]
    pub span: Span,
    /// Additional context message
    pub message: Option<String>,
    /// Optional suggestion for fixing the error
    #[cfg_attr(feature = "miette", help)]
    pub suggestion: Option<String>,
}

impl ParseError {
    /// Create a new parse error
    pub fn new(kind: ErrorKind, span: Span) -> Self {
        Self {
            kind,
            span,
            message: None,
            suggestion: None,
        }
    }

    /// Create a parse error with a custom message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Add a suggestion for fixing the error
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Create an error for an unexpected type
    pub fn expected_type(expected: &str, found: &str, span: Span) -> Self {
        Self::new(ErrorKind::UnexpectedType, span)
            .with_message(format!("expected {}, found {}", expected, found))
    }

    /// Create an error for a missing required field
    pub fn missing_field(field: &str, span: Span) -> Self {
        Self::new(ErrorKind::MissingField, span)
            .with_message(format!("missing required field: {}", field))
    }

    /// Create an error for an invalid field value
    pub fn invalid_value(field: &str, reason: &str, span: Span) -> Self {
        Self::new(ErrorKind::InvalidValue, span)
            .with_message(format!("invalid value for {}: {}", field, reason))
    }

    /// Create an error for a Jinja template parsing failure
    pub fn jinja_error(error: String, span: Span) -> Self {
        Self::new(ErrorKind::JinjaError, span).with_message(format!("Jinja error: {}", error))
    }
}

#[cfg(not(feature = "miette"))]
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.kind, self.span)?;
        if let Some(msg) = &self.message {
            write!(f, ": {}", msg)?;
        }
        if let Some(sug) = &self.suggestion {
            write!(f, " (suggestion: {})", sug)?;
        }
        Ok(())
    }
}

#[cfg(not(feature = "miette"))]
impl std::error::Error for ParseError {}

/// The kind of parsing error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Expected a different type (e.g., expected string, got number)
    UnexpectedType,
    /// A required field is missing
    MissingField,
    /// A field has an invalid value
    InvalidValue,
    /// Error parsing a Jinja2 template
    JinjaError,
    /// Duplicate key in mapping
    DuplicateKey,
    /// Invalid field name
    InvalidField,
    /// YAML parsing error
    YamlError,
    /// Conditional (if/then/else) is malformed
    InvalidConditional,
    /// Generic parse error
    ParseError,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::UnexpectedType => write!(f, "unexpected type"),
            ErrorKind::MissingField => write!(f, "missing field"),
            ErrorKind::InvalidValue => write!(f, "invalid value"),
            ErrorKind::JinjaError => write!(f, "Jinja template error"),
            ErrorKind::DuplicateKey => write!(f, "duplicate key"),
            ErrorKind::InvalidField => write!(f, "invalid field"),
            ErrorKind::YamlError => write!(f, "YAML parsing error"),
            ErrorKind::InvalidConditional => write!(f, "invalid conditional"),
            ErrorKind::ParseError => write!(f, "parse error"),
        }
    }
}

/// A collection of parse errors
///
/// Some parsing operations may accumulate multiple errors before failing.
#[derive(Debug, Clone)]
pub struct ParseErrors {
    pub errors: Vec<ParseError>,
}

impl ParseErrors {
    /// Create a new collection with a single error
    pub fn single(error: ParseError) -> Self {
        Self {
            errors: vec![error],
        }
    }

    /// Create a new collection from multiple errors
    pub fn new(errors: Vec<ParseError>) -> Self {
        Self { errors }
    }

    /// Add an error to the collection
    pub fn push(&mut self, error: ParseError) {
        self.errors.push(error);
    }

    /// Check if there are any errors
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get the number of errors
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Convert into the underlying Vec of errors
    pub fn into_vec(self) -> Vec<ParseError> {
        self.errors
    }
}

impl fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} error(s) occurred:", self.errors.len())?;
        for (i, err) in self.errors.iter().enumerate() {
            write!(f, "\n  {}. {}", i + 1, err)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseErrors {}

impl From<ParseError> for ParseErrors {
    fn from(error: ParseError) -> Self {
        Self::single(error)
    }
}

impl FromIterator<ParseError> for ParseErrors {
    fn from_iter<T: IntoIterator<Item = ParseError>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let span = Span::new(10, 20, 1, 10, 1, 20);
        let err =
            ParseError::new(ErrorKind::MissingField, span).with_message("field 'name' is required");

        assert_eq!(err.kind, ErrorKind::MissingField);
        assert_eq!(err.span, span);
        assert_eq!(err.message, Some("field 'name' is required".to_string()));
    }

    #[test]
    fn test_error_with_suggestion() {
        let span = Span::unknown();
        let err = ParseError::new(ErrorKind::InvalidValue, span)
            .with_message("invalid version")
            .with_suggestion("use format 'x.y.z'");

        assert_eq!(err.suggestion, Some("use format 'x.y.z'".to_string()));
    }

    #[test]
    fn test_error_display() {
        let span = Span::new(0, 5, 1, 1, 1, 5);
        let err = ParseError::new(ErrorKind::MissingField, span).with_message("missing 'name'");

        let display = format!("{}", err);
        assert!(display.contains("missing field"));
        // With miette feature, the display format is different
        #[cfg(not(feature = "miette"))]
        assert!(display.contains("1:1"));
    }

    #[test]
    fn test_parse_errors_collection() {
        let mut errors = ParseErrors::new(vec![]);
        errors.push(ParseError::new(ErrorKind::MissingField, Span::unknown()));
        errors.push(ParseError::new(ErrorKind::InvalidValue, Span::unknown()));

        assert_eq!(errors.len(), 2);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_expected_type_error() {
        let span = Span::new(0, 5, 1, 1, 1, 5);
        let err = ParseError::expected_type("string", "number", span);

        assert_eq!(err.kind, ErrorKind::UnexpectedType);
        assert!(err.message.unwrap().contains("expected string"));
    }

    #[test]
    fn test_jinja_error() {
        let span = Span::new(0, 10, 1, 1, 1, 10);
        let err = ParseError::jinja_error("undefined variable 'foo'".to_string(), span);

        assert_eq!(err.kind, ErrorKind::JinjaError);
        assert!(err.message.unwrap().contains("undefined variable"));
    }
}
