//! Conditional list and conditional parsing functions

use marked_yaml::Node as MarkedNode;
use rattler_build_jinja::JinjaExpression;

use crate::{
    converter::{FromStrConverter, NodeConverter},
    error::{ParseError, ParseResult},
    helpers::get_span,
    list::parse_list_or_item_with_converter,
    types::{Conditional, ConditionalList, ConditionalListOrItem, Item},
    value::parse_value_with_converter,
};

/// Parse a ConditionalList<T> from YAML
///
/// This handles sequences that may contain if/then/else conditionals
///
/// # Arguments
/// * `yaml` - The YAML node to parse (must be a sequence)
pub fn parse_conditional_list<T>(yaml: &MarkedNode) -> ParseResult<ConditionalList<T>>
where
    T: std::str::FromStr + ToString,
    T::Err: std::fmt::Display,
{
    parse_conditional_list_with_converter(yaml, &FromStrConverter::new())
}

/// Parse a ConditionalList<T> from YAML using a custom converter
///
/// This handles sequences that may contain if/then/else conditionals
///
/// # Arguments
/// * `yaml` - The YAML node to parse (must be a sequence)
/// * `converter` - The converter to use for parsing concrete values
pub fn parse_conditional_list_with_converter<T, C>(
    yaml: &MarkedNode,
    converter: &C,
) -> ParseResult<ConditionalList<T>>
where
    C: NodeConverter<T>,
{
    let sequence = yaml
        .as_sequence()
        .ok_or_else(|| ParseError::expected_type("sequence", "non-sequence", get_span(yaml)))?;

    let mut items = Vec::new();
    for item in sequence.iter() {
        items.push(parse_item_with_converter(item, converter)?);
    }
    Ok(ConditionalList::new(items))
}

/// Parse a ConditionalListOrItem<T> from YAML
///
/// This handles either:
/// - A single scalar value (e.g., `imports: anyio`)
/// - A sequence that may contain if/then/else conditionals (e.g., `imports: [numpy, pandas]`)
///
/// # Arguments
/// * `yaml` - The YAML node to parse (can be a scalar or a sequence)
pub fn parse_conditional_list_or_item<T>(yaml: &MarkedNode) -> ParseResult<ConditionalListOrItem<T>>
where
    T: std::str::FromStr + ToString,
    T::Err: std::fmt::Display,
{
    parse_conditional_list_or_item_with_converter(yaml, &FromStrConverter::new())
}

/// Parse a ConditionalListOrItem<T> from YAML using a custom converter
///
/// This handles either a single value or a sequence that may contain conditionals
///
/// # Arguments
/// * `yaml` - The YAML node to parse (can be a scalar or a sequence)
/// * `converter` - The converter to use for parsing concrete values
pub fn parse_conditional_list_or_item_with_converter<T, C>(
    yaml: &MarkedNode,
    converter: &C,
) -> ParseResult<ConditionalListOrItem<T>>
where
    C: NodeConverter<T>,
{
    // If it's a sequence, parse as a list
    if let Some(sequence) = yaml.as_sequence() {
        let mut items = Vec::new();
        for item in sequence.iter() {
            items.push(parse_item_with_converter(item, converter)?);
        }
        return Ok(ConditionalListOrItem::new(items));
    }

    // Otherwise, parse as a single item (scalar or conditional mapping)
    let item = parse_item_with_converter(yaml, converter)?;
    Ok(ConditionalListOrItem::new(vec![item]))
}

/// Parse an Item<T> from YAML using a custom converter
///
/// This handles both simple values and conditional (if/then/else) items
fn parse_item_with_converter<T, C>(yaml: &MarkedNode, converter: &C) -> ParseResult<Item<T>>
where
    C: NodeConverter<T>,
{
    // Check if it's a mapping with "if" key (conditional)
    if let Some(mapping) = yaml.as_mapping()
        && mapping.get("if").is_some()
    {
        return parse_conditional_with_converter(yaml, converter);
    }

    // Otherwise, it's a simple value
    let value = parse_value_with_converter(yaml, "item", converter)?;
    Ok(Item::Value(value))
}

/// Parse a Conditional<T> from YAML using a custom converter
fn parse_conditional_with_converter<T, C>(yaml: &MarkedNode, converter: &C) -> ParseResult<Item<T>>
where
    C: NodeConverter<T>,
{
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::invalid_conditional("Expected mapping for conditional", get_span(yaml))
    })?;

    // Get the "if" field
    let condition_node = mapping
        .get("if")
        .ok_or_else(|| ParseError::missing_field("if", get_span(yaml)))?;

    let condition_scalar = condition_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(condition_node))
    })?;

    let condition_span = *condition_scalar.span();
    let condition = JinjaExpression::new(condition_scalar.as_str().to_string())
        .map_err(|e| ParseError::jinja_error(e, condition_span))?;

    // Get the "then" field
    let then_yaml = mapping
        .get("then")
        .ok_or_else(|| ParseError::missing_field("then", get_span(yaml)))?;

    let then = parse_list_or_item_with_converter(then_yaml, converter)?;

    // Get optional "else" field
    let else_value = if let Some(else_yaml) = mapping.get("else") {
        Some(parse_list_or_item_with_converter(else_yaml, converter)?)
    } else {
        None
    };

    Ok(Item::Conditional(Conditional {
        condition,
        then,
        else_value,
        condition_span: Some(condition_span),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_list() {
        let yaml = marked_yaml::parse_yaml(0, "val: [\"1.0\", \"2.0\"]").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ConditionalList<String> = parse_conditional_list(node).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|item| item.is_value()));
    }

    #[test]
    fn test_parse_conditional() {
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
val:
  - if: win
    then: "14"
  - if: unix
    then: "16"
"#,
        )
        .unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ConditionalList<String> = parse_conditional_list(node).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|item| item.is_conditional()));
    }

    #[test]
    fn test_parse_conditional_with_list() {
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
val:
  - if: unix
    then: ["3.9", "3.10"]
  - if: win
    then: ["3.8"]
    else: ["3.11", "3.12", "3.13"]
"#,
        )
        .unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ConditionalList<String> = parse_conditional_list(node).unwrap();
        assert_eq!(list.len(), 2);

        let first = list.iter().next().unwrap();
        if let Item::Conditional(cond) = first {
            assert_eq!(cond.then.len(), 2);
        } else {
            panic!("Expected conditional");
        }

        let second = list.iter().nth(1).unwrap();
        if let Item::Conditional(cond) = second {
            assert_eq!(cond.then.len(), 1);
            assert_eq!(cond.else_value.as_ref().unwrap().len(), 3);
        } else {
            panic!("Expected conditional");
        }
    }

    #[test]
    fn test_parse_mixed_list() {
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
val:
  - "plain"
  - ${{ template }}
  - if: condition
    then: "conditional"
"#,
        )
        .unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ConditionalList<String> = parse_conditional_list(node).unwrap();
        assert_eq!(list.len(), 3);

        let items: Vec<_> = list.iter().collect();
        assert!(items[0].is_value());
        assert!(items[1].is_value());
        assert!(items[2].is_conditional());
    }

    #[test]
    fn test_parse_conditional_with_else() {
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
val:
  - if: win
    then: "windows"
    else: "unix"
"#,
        )
        .unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ConditionalList<String> = parse_conditional_list(node).unwrap();
        assert_eq!(list.len(), 1);

        if let Item::Conditional(cond) = list.iter().next().unwrap() {
            assert!(cond.else_value.is_some());
            assert_eq!(cond.else_value.as_ref().unwrap().len(), 1);
        } else {
            panic!("Expected conditional");
        }
    }
}
