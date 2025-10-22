//! Value parsing functions - delegating to shared rattler_build_yaml_parser
//!
//! Since we now use the shared parser types directly, these are just thin wrappers
//! that convert errors from the shared parser format to recipe error format.

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser as yaml_parser;

use crate::error::{ErrorKind, ParseError, ParseResult};

/// Convert a shared parser error to a recipe ParseError
pub(super) fn convert_yaml_error(error: yaml_parser::ParseError) -> ParseError {
    let span = *error.span();
    match error {
        yaml_parser::ParseError::JinjaError { message, .. } => {
            ParseError::new(ErrorKind::JinjaError, span)
                .with_message(format!("Jinja error: {}", message))
        }
        yaml_parser::ParseError::InvalidValue {
            field,
            reason,
            suggestion,
            ..
        } => {
            let mut err = ParseError::new(ErrorKind::InvalidValue, span)
                .with_message(format!("invalid value for {}: {}", field, reason));
            if let Some(sug) = suggestion {
                err = err.with_suggestion(sug);
            }
            err
        }
        yaml_parser::ParseError::MissingField { field, .. } => {
            ParseError::new(ErrorKind::MissingField, span)
                .with_message(format!("missing required field: {}", field))
        }
        yaml_parser::ParseError::TypeMismatch {
            expected, actual, ..
        } => ParseError::new(ErrorKind::UnexpectedType, span)
            .with_message(format!("expected {}, found {}", expected, actual)),
        yaml_parser::ParseError::InvalidConditional { message, .. } => {
            ParseError::new(ErrorKind::InvalidConditional, span)
                .with_message(format!("invalid conditional: {}", message))
        }
        yaml_parser::ParseError::Generic {
            message,
            suggestion,
            ..
        } => {
            let mut err = ParseError::new(ErrorKind::ParseError, span).with_message(message);
            if let Some(sug) = suggestion {
                err = err.with_suggestion(sug);
            }
            err
        }
    }
}

/// Parse a Value<T> from YAML
///
/// This handles both concrete values and Jinja templates
///
/// # Arguments
/// * `yaml` - The YAML node to parse
pub fn parse_value<T>(yaml: &MarkedNode) -> ParseResult<crate::stage0::types::Value<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    parse_value_with_name(yaml, "value")
}

/// Parse a Value<T> from YAML with a specific field name for error messages
///
/// This handles both concrete values and Jinja templates
///
/// # Arguments
/// * `yaml` - The YAML node to parse
/// * `field_name` - Field name for error messages (e.g., "build.number")
pub fn parse_value_with_name<T>(
    yaml: &MarkedNode,
    field_name: &str,
) -> ParseResult<crate::stage0::types::Value<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    // Use the shared parser directly - no conversion needed since we use the same types!
    yaml_parser::parse_value_with_name(yaml, field_name).map_err(convert_yaml_error)
}
