//! NodeConverter implementation for minijinja Variable types
//!
//! This module provides a custom converter that handles the special semantics
//! of variant configuration values, including type coercion and proper handling
//! of quoted vs unquoted values.

use marked_yaml::Node as MarkedNode;
use rattler_build_jinja::Variable;
use rattler_build_yaml_parser::{NodeConverter, ParseError, ParseResult};

/// Converter for minijinja Variable types
///
/// This converter implements the special semantics needed for variant configuration:
/// - Distinguishes between quoted and unquoted values
/// - Handles type coercion for booleans, integers, and floats
/// - Preserves version numbers as strings (e.g., "3.14" stays as string, not float)
pub struct VariableConverter;

impl VariableConverter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for VariableConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeConverter<Variable> for VariableConverter {
    fn convert_scalar(&self, node: &MarkedNode, _field_name: &str) -> ParseResult<Variable> {
        let scalar = node
            .as_scalar()
            .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", *node.span()))?;

        // Check if the scalar may coerce to a non-string type (i.e., it's unquoted)
        // may_coerce() returns true for unquoted values that could be numbers/booleans
        if !scalar.may_coerce() {
            // Quoted string - always treat as string
            Ok(Variable::from(scalar.as_str()))
        } else {
            // Unquoted value - try to parse as bool, int, or float
            // Try to parse as bool first
            if let Some(bool) = scalar.as_bool() {
                Ok(Variable::from(bool))
            } else if let Some(i) = scalar.as_i64() {
                // Parse as integer
                Ok(Variable::from(i))
            } else if scalar.as_f64().is_some() {
                // Float - but we convert to string to preserve version numbers
                // This is important because "3.14" should be treated as a version string,
                // not as a float 3.14
                Ok(Variable::from(minijinja::Value::from(scalar.as_str())))
            } else {
                // Fallback to string
                Ok(Variable::from(minijinja::Value::from(scalar.as_str())))
            }
        }
    }
}
