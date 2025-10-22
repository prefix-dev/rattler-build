//! List and conditional parsing functions - delegating to shared rattler_build_yaml_parser
//!
//! Since we now use the shared parser types directly, these are just thin wrappers
//! that convert errors from the shared parser format to recipe error format.

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser as yaml_parser;

use crate::error::ParseResult;

/// Parse a ConditionalList<T> from YAML
///
/// This handles sequences that may contain if/then/else conditionals
pub fn parse_conditional_list<T>(
    yaml: &MarkedNode,
) -> ParseResult<crate::stage0::types::ConditionalList<T>>
where
    T: std::str::FromStr + ToString,
    T::Err: std::fmt::Display,
{
    // Use the shared parser directly - no conversion needed since we use the same types!
    yaml_parser::parse_conditional_list(yaml).map_err(super::value::convert_yaml_error)
}
