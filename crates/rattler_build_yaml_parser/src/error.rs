//! Error types for YAML parsing

use marked_yaml::Span;
use std::path::PathBuf;
use thiserror::Error;

/// Result type for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;

/// Errors that can occur during YAML parsing
#[derive(Debug, Error, Clone)]
pub enum ParseError {
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
