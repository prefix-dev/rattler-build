//! Node conversion traits and implementations
//!
//! This module provides the `NodeConverter` trait which allows customizing
//! how YAML nodes are converted to specific types. This enables the parser
//! to support different conversion strategies (e.g., FromStr, minijinja Variables, etc.)

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ParseError, ParseResult},
    helpers::{contains_jinja_template, get_span},
};

/// Trait for converting YAML nodes to specific types
///
/// This trait allows customizing the conversion logic for different target types.
/// The default implementation (`FromStrConverter`) uses `FromStr`, but specialized
/// implementations can provide custom conversion logic (e.g., for minijinja Variables).
pub trait NodeConverter<T> {
    /// Convert a YAML scalar node to a concrete value
    ///
    /// # Arguments
    /// * `node` - The YAML node to convert (must be a scalar)
    /// * `field_name` - Field name for error messages (e.g., "build.number")
    ///
    /// # Returns
    /// The converted value or a parse error
    fn convert_scalar(&self, node: &MarkedNode, field_name: &str) -> ParseResult<T>;

    /// Check if a string should be treated as a template
    ///
    /// By default, checks if the string contains `${{` and `}}`
    fn is_template(&self, s: &str) -> bool {
        contains_jinja_template(s)
    }
}

/// Default converter that uses `FromStr` for parsing
///
/// This is the default conversion strategy used by the parser.
pub struct FromStrConverter<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> FromStrConverter<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Default for FromStrConverter<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> NodeConverter<T> for FromStrConverter<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    fn convert_scalar(&self, node: &MarkedNode, field_name: &str) -> ParseResult<T> {
        let scalar = node
            .as_scalar()
            .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", get_span(node)))?;

        let s = scalar.as_str();
        let span = *scalar.span();

        s.parse::<T>()
            .map_err(|e| ParseError::invalid_value(field_name, e.to_string(), span))
    }
}
