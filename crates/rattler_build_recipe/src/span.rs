//! Span utilities - thin wrappers around marked_yaml::Span
//!
//! This module re-exports Span from marked_yaml and provides a compatibility
//! wrapper for SpannedString.

pub use marked_yaml::Span;

use marked_yaml::types::MarkedScalarNode as Scalar;

/// A string with span information - thin wrapper around marked_yaml::Scalar
#[derive(Debug, Clone)]
pub struct SpannedString {
    value: String,
    span: Span,
}

impl SpannedString {
    /// Get the string value
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Get the span information
    pub fn span(&self) -> Span {
        self.span
    }
}

impl From<&Scalar> for SpannedString {
    fn from(scalar: &Scalar) -> Self {
        Self {
            value: scalar.as_str().to_string(),
            span: *scalar.span(),
        }
    }
}
