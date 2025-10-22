//! Value parsing functions - delegating to shared rattler_build_yaml_parser
//!
//! Since we now use the shared parser types directly (including ParseError),
//! these are just thin wrappers that directly delegate to the yaml_parser.

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser::{self as yaml_parser, ParseResult};

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
    // Use the shared parser directly - no conversion needed since we use the same ParseError type!
    yaml_parser::parse_value_with_name(yaml, field_name)
}
