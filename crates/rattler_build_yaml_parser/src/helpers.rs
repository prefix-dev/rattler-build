//! Helper functions for YAML parsing

use marked_yaml::{Node as MarkedNode, Span, types::MarkedMappingNode};

use crate::error::{ParseError, ParseResult};

/// Get the span from a marked_yaml node
pub fn get_span(node: &MarkedNode) -> Span {
    match node {
        MarkedNode::Scalar(s) => *s.span(),
        MarkedNode::Mapping(m) => *m.span(),
        MarkedNode::Sequence(s) => *s.span(),
    }
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
pub fn validate_mapping_fields(
    mapping: &MarkedMappingNode,
    context_name: &str,
    valid_fields: &[&str],
) -> ParseResult<()> {
    for (key_node, _value_node) in mapping.iter() {
        let key = key_node.as_str();
        if !valid_fields.contains(&key) {
            return Err(ParseError::invalid_value(
                context_name,
                format!("unknown field '{}'", key),
                *key_node.span(),
            )
            .with_suggestion(format!("Valid fields are: {}", valid_fields.join(", "))));
        }
    }
    Ok(())
}

/// Check if a string contains a Jinja2 template
pub fn contains_jinja_template(s: &str) -> bool {
    s.contains("${{") && s.contains("}}")
}
