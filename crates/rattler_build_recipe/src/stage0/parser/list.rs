//! List and conditional parsing functions

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ErrorKind, ParseError, ParseResult},
    span::SpannedString,
    stage0::parser::helpers::{get_span, parse_list_or_item},
};

/// Parse a ConditionalList<T> from YAML
///
/// This handles sequences that may contain if/then/else conditionals
pub fn parse_conditional_list<T>(
    yaml: &MarkedNode,
) -> ParseResult<crate::stage0::types::ConditionalList<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let sequence = yaml
        .as_sequence()
        .ok_or_else(|| ParseError::expected_type("sequence", "non-sequence", get_span(yaml)))?;

    let mut items = Vec::new();
    for item in sequence.iter() {
        items.push(parse_item(item)?);
    }
    Ok(crate::stage0::types::ConditionalList::new(items))
}

/// Parse an Item<T> from YAML
///
/// This handles both simple values and conditional (if/then/else) items
fn parse_item<T>(yaml: &MarkedNode) -> ParseResult<crate::stage0::types::Item<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    // Check if it's a mapping with "if" key (conditional)
    if let Some(mapping) = yaml.as_mapping() {
        if mapping.get("if").is_some() {
            return parse_conditional(yaml);
        }
    }

    // Otherwise, it's a simple value
    if let Some(scalar) = yaml.as_scalar() {
        let spanned = SpannedString::from(scalar);
        let s = spanned.as_str();

        // Simple string value
        if s.contains("${{") && s.contains("}}") {
            let template = crate::stage0::types::JinjaTemplate::new(s.to_string())
                .map_err(|e| ParseError::jinja_error(e, spanned.span()))?;
            Ok(crate::stage0::types::Item::Value(
                crate::stage0::types::Value::Template(template),
            ))
        } else {
            let value = s
                .parse::<T>()
                .map_err(|e| ParseError::invalid_value("item", &e.to_string(), spanned.span()))?;
            Ok(crate::stage0::types::Item::Value(
                crate::stage0::types::Value::Concrete(value),
            ))
        }
    } else {
        Err(ParseError::expected_type(
            "scalar or conditional",
            "other",
            get_span(yaml),
        ))
    }
}

/// Parse a Conditional<T> from YAML
fn parse_conditional<T>(yaml: &MarkedNode) -> ParseResult<crate::stage0::types::Item<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::new(ErrorKind::InvalidConditional, get_span(yaml))
            .with_message("Expected mapping for conditional")
    })?;

    // Get the "if" field
    let condition_node = mapping
        .get("if")
        .ok_or_else(|| ParseError::missing_field("if", get_span(yaml)))?;

    let condition_scalar = condition_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(condition_node))
            .with_message("Conditional 'if' field must be a scalar")
    })?;

    let condition_spanned = SpannedString::from(condition_scalar);
    let condition =
        crate::stage0::types::JinjaExpression::new(condition_spanned.as_str().to_string())
            .map_err(|e| ParseError::jinja_error(e, condition_spanned.span()))?;

    // Get the "then" field
    let then_yaml = mapping
        .get("then")
        .ok_or_else(|| ParseError::missing_field("then", get_span(yaml)))?;

    let then = parse_list_or_item(then_yaml)?;

    // Get optional "else" field
    let else_value = if let Some(else_yaml) = mapping.get("else") {
        parse_list_or_item(else_yaml)?
    } else {
        crate::stage0::types::ListOrItem::new(Vec::new())
    };

    Ok(crate::stage0::types::Item::Conditional(
        crate::stage0::types::Conditional {
            condition,
            then,
            else_value,
        },
    ))
}
