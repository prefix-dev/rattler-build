//! Value parsing functions - now delegating to shared rattler_build_yaml_parser

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser as yaml_parser;

use crate::{error::ParseResult, stage0::parser_adapter};

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
    // Use the shared parser
    let shared_value = yaml_parser::parse_value_with_name(yaml, field_name)
        .map_err(parser_adapter::convert_error)?;

    // Convert to recipe Value
    Ok(parser_adapter::convert_value(shared_value))
}
