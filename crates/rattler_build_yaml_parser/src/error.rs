//! Error types for YAML parsing

use marked_yaml::Span;
use std::{path::PathBuf, rc::Rc};
use thiserror::Error;

#[cfg(feature = "miette")]
use miette::{Diagnostic, SourceSpan};

/// Result type for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;

/// Errors that can occur during YAML parsing
#[derive(Debug, Error, Clone)]
pub enum ParseError {
    /// IO error when reading a file
    #[error("IO error while reading file {path}: {source}")]
    IoError {
        path: PathBuf,
        source: Rc<std::io::Error>,
    },

    /// Generic parse error with message and location
    #[error("parse error: {message}")]
    Generic {
        message: String,
        span: Span,
        suggestion: Option<String>,
    },

    /// Missing required field
    #[error("missing required field '{field}'")]
    MissingField { field: String, span: Span },

    /// Type mismatch
    #[error("expected {expected} but got {actual}")]
    TypeMismatch {
        expected: String,
        actual: String,
        span: Span,
    },

    /// Invalid value
    #[error("invalid value for '{field}': {reason}")]
    InvalidValue {
        field: String,
        reason: String,
        span: Span,
        suggestion: Option<String>,
    },

    /// Jinja template error
    #[error("Jinja template error: {message}")]
    JinjaError { message: String, span: Span },

    /// Invalid conditional structure
    #[error("invalid conditional: {message}")]
    InvalidConditional { message: String, span: Span },
}

impl ParseError {
    /// Create a generic parse error
    pub fn generic(message: impl Into<String>, span: Span) -> Self {
        Self::Generic {
            message: message.into(),
            span,
            suggestion: None,
        }
    }

    /// Create a missing field error
    pub fn missing_field(field: impl Into<String>, span: Span) -> Self {
        Self::MissingField {
            field: field.into(),
            span,
        }
    }

    /// Create a type mismatch error
    pub fn expected_type(
        expected: impl Into<String>,
        actual: impl Into<String>,
        span: Span,
    ) -> Self {
        Self::TypeMismatch {
            expected: expected.into(),
            actual: actual.into(),
            span,
        }
    }

    /// Create an invalid value error
    pub fn invalid_value(field: impl Into<String>, reason: impl Into<String>, span: Span) -> Self {
        Self::InvalidValue {
            field: field.into(),
            reason: reason.into(),
            span,
            suggestion: None,
        }
    }

    /// Create a Jinja error
    pub fn jinja_error(error: impl Into<String>, span: Span) -> Self {
        Self::JinjaError {
            message: error.into(),
            span,
        }
    }

    /// Create an invalid conditional error
    pub fn invalid_conditional(message: impl Into<String>, span: Span) -> Self {
        Self::InvalidConditional {
            message: message.into(),
            span,
        }
    }

    pub fn io_error(path: PathBuf, source: std::io::Error) -> Self {
        Self::IoError {
            path,
            source: Rc::new(source),
        }
    }

    /// Add a suggestion to the error
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        match &mut self {
            Self::Generic { suggestion: s, .. } | Self::InvalidValue { suggestion: s, .. } => {
                *s = Some(suggestion.into());
            }
            _ => {}
        }
        self
    }

    /// Get the span from this error
    pub fn span(&self) -> &Span {
        match self {
            Self::Generic { span, .. }
            | Self::MissingField { span, .. }
            | Self::TypeMismatch { span, .. }
            | Self::InvalidValue { span, .. }
            | Self::JinjaError { span, .. }
            | Self::InvalidConditional { span, .. } => span,
            Self::IoError { .. } => panic!("IO errors do not have associated spans"),
        }
    }

    /// Create a simple error from a message (for compatibility)
    pub fn from_message(message: impl Into<String>) -> Self {
        Self::Generic {
            message: message.into(),
            span: marked_yaml::Span::new_blank(),
            suggestion: None,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        match &mut self {
            Self::Generic { message: m, .. } => {
                *m = message.into();
            }
            _ => {}
        }
        self
    }
}

#[cfg(feature = "miette")]
impl Diagnostic for ParseError {
    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        match self {
            Self::IoError { .. } => return None,
            _ => {}
        }

        let source_span = span_to_source_span(self.span());

        let label = match self {
            Self::IoError { path, source } => panic!(
                "IO errors do not have associated spans: {}: {}",
                path.display(),
                source
            ),
            Self::Generic { message, .. } => {
                miette::LabeledSpan::new_with_span(Some(message.clone()), source_span)
            }
            Self::MissingField { field, .. } => {
                miette::LabeledSpan::new_with_span(
                    Some(format!("missing field '{}'", field)),
                    source_span,
                )
            }
            Self::TypeMismatch { expected, actual, .. } => {
                miette::LabeledSpan::new_with_span(
                    Some(format!("expected {} but got {}", expected, actual)),
                    source_span,
                )
            }
            Self::InvalidValue { field, reason, .. } => {
                miette::LabeledSpan::new_with_span(
                    Some(format!("invalid value for '{}': {}", field, reason)),
                    source_span,
                )
            }
            Self::JinjaError { message, .. } => {
                miette::LabeledSpan::new_with_span(Some(message.clone()), source_span)
            }
            Self::InvalidConditional { message, .. } => {
                miette::LabeledSpan::new_with_span(Some(message.clone()), source_span)
            }
        };

        Some(Box::new(std::iter::once(label)))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        match self {
            Self::Generic { suggestion: Some(s), .. } | Self::InvalidValue { suggestion: Some(s), .. } => {
                Some(Box::new(s.clone()))
            }
            _ => None,
        }
    }
}

/// Format a span for error messages
pub fn format_span(span: &Span) -> String {
    if let Some(start) = span.start() {
        format!("line {}, column {}", start.line(), start.column())
    } else {
        "unknown location".to_string()
    }
}

/// Convert marked_yaml Span to miette SourceSpan
#[cfg(feature = "miette")]
fn span_to_source_span(span: &Span) -> SourceSpan {
    if let Some(start) = span.start() {
        let offset = start.character();
        // If we have an end marker, calculate the length
        let len = if let Some(end) = span.end() {
            end.character().saturating_sub(offset).max(1)
        } else {
            1 // Default to highlighting 1 character
        };
        SourceSpan::new(offset.into(), len)
    } else {
        // No span information, use offset 0 with length 0
        SourceSpan::new(0.into(), 0)
    }
}

/// Error type for file-based parsing operations
#[derive(Debug, Error)]
pub enum FileParseError {
    /// IO error when reading file
    #[error("Failed to read file {path}: {source}")]
    IoError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// YAML parsing error
    #[error("YAML parsing error in {}: {message}", path.display())]
    YamlError { path: PathBuf, message: String },

    /// Parse error
    #[error("Parse error in {}: {source}", path.display())]
    ParseError {
        path: PathBuf,
        #[source]
        source: ParseError,
    },
}
