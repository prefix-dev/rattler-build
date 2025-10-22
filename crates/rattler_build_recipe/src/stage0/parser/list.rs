//! List and conditional parsing functions - now delegating to shared rattler_build_yaml_parser

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser as yaml_parser;

use crate::{error::ParseResult, stage0::parser_adapter};

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
    // Use the shared parser
    let shared_list =
        yaml_parser::parse_conditional_list(yaml).map_err(parser_adapter::convert_error)?;

    // Convert to recipe ConditionalList
    Ok(parser_adapter::convert_conditional_list(shared_list))
}
