//! Conditional list and conditional parsing functions

use marked_yaml::Node as MarkedNode;
use rattler_build_jinja::JinjaExpression;

use crate::{
    error::{ParseError, ParseResult},
    helpers::{SpannedString, contains_jinja_template, get_span},
    list::parse_list_or_item,
    types::{Conditional, ConditionalList, Item, Value},
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
    let sequence = yaml
        .as_sequence()
        .ok_or_else(|| ParseError::expected_type("sequence", "non-sequence", get_span(yaml)))?;

    let mut items = Vec::new();
    for item in sequence.iter() {
        items.push(parse_item(item)?);
    }
    Ok(ConditionalList::new(items))
}

/// Parse an Item<T> from YAML
///
/// This handles both simple values and conditional (if/then/else) items
fn parse_item<T>(yaml: &MarkedNode) -> ParseResult<Item<T>>
where
    T: std::str::FromStr + ToString,
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
        let spanned = SpannedString::from_scalar(scalar);
        let s = spanned.as_str();
        let span = spanned.span();

        // Check if it's a template
        if contains_jinja_template(s) {
            let template = rattler_build_jinja::JinjaTemplate::new(s.to_string())
                .map_err(|e| ParseError::jinja_error(e, span))?;
            Ok(Item::Value(Value::new_template(template, Some(span))))
        } else {
            let value = s
                .parse::<T>()
                .map_err(|e| ParseError::invalid_value("item", &e.to_string(), span))?;
            Ok(Item::Value(Value::new_concrete(value, Some(span))))
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
fn parse_conditional<T>(yaml: &MarkedNode) -> ParseResult<Item<T>>
where
    T: std::str::FromStr + ToString,
    T::Err: std::fmt::Display,
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

    let condition_spanned = SpannedString::from_scalar(condition_scalar);
    let condition = JinjaExpression::new(condition_spanned.as_str().to_string())
        .map_err(|e| ParseError::jinja_error(e, condition_spanned.span()))?;

    // Get the "then" field
    let then_yaml = mapping
        .get("then")
        .ok_or_else(|| ParseError::missing_field("then", get_span(yaml)))?;

    let then = parse_list_or_item(then_yaml)?;

    // Get optional "else" field
    let else_value = if let Some(else_yaml) = mapping.get("else") {
        Some(parse_list_or_item(else_yaml)?)
    } else {
        None
    };

    Ok(Item::Conditional(Conditional {
        condition,
        then,
        else_value,
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
