//! Helper functions for parsing

use marked_yaml::{Node as MarkedNode, types::MarkedMappingNode};

use crate::{
    error::{ParseError, ParseResult},
    span::{Span, SpannedString},
};

/// Get the span from a marked_yaml node
pub(super) fn get_span(node: &MarkedNode) -> Span {
    match node {
        MarkedNode::Scalar(s) => (*s.span()).into(),
        MarkedNode::Mapping(m) => (*m.span()).into(),
        MarkedNode::Sequence(s) => (*s.span()).into(),
    }
}

/// Parse a ListOrItem<T> from YAML
///
/// Note: ListOrItem is a newtype wrapper around Vec<T>, so we parse
/// a list of items or a single item and wrap it appropriately
pub(super) fn parse_list_or_item<T>(
    yaml: &MarkedNode,
) -> ParseResult<crate::stage0::types::ListOrItem<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    if let Some(sequence) = yaml.as_sequence() {
        let mut items = Vec::new();
        for item in sequence.iter() {
            let parsed = parse_string_value(item)?;
            items.push(parsed);
        }
        Ok(crate::stage0::types::ListOrItem::new(items))
    } else {
        let item = parse_string_value(yaml)?;
        Ok(crate::stage0::types::ListOrItem::single(item))
    }
}

/// Parse a simple string value (not an Item, just T)
pub(super) fn parse_string_value<T>(yaml: &MarkedNode) -> ParseResult<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let scalar = yaml
        .as_scalar()
        .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", get_span(yaml)))?;

    let spanned = SpannedString::from(scalar);
    spanned
        .as_str()
        .parse::<T>()
        .map_err(|e| ParseError::invalid_value("value", &e.to_string(), spanned.span()))
}

/// Helper for validating that all fields in a mapping are known
///
/// This checks that every key in the mapping appears in the valid_fields list
/// and returns an error with a helpful suggestion if an unknown field is found.
///
/// # Arguments
/// * `mapping` - The YAML mapping to validate
/// * `context_name` - Name for error messages (e.g., "python test", "requirements")
/// * `valid_fields` - List of valid field names
///
/// # Example
/// ```ignore
/// validate_mapping_fields(mapping, "python test", &["imports", "pip_check", "python_version"])?;
/// ```
pub(super) fn validate_mapping_fields(
    mapping: &MarkedMappingNode,
    context_name: &str,
    valid_fields: &[&str],
) -> ParseResult<()> {
    for (key_node, _value_node) in mapping.iter() {
        let key = key_node.as_str();
        if !valid_fields.contains(&key) {
            return Err(ParseError::invalid_value(
                context_name,
                &format!("unknown field '{}'", key),
                (*key_node.span()).into(),
            )
            .with_suggestion(format!("Valid fields are: {}", valid_fields.join(", "))));
        }
    }
    Ok(())
}
