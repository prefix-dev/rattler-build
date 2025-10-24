//! Value parsing functions

use marked_yaml::Node as MarkedNode;
use rattler_build_jinja::JinjaTemplate;

use crate::{
    converter::{FromStrConverter, NodeConverter},
    error::{ParseError, ParseResult},
    helpers::get_span,
    types::Value,
};

/// Parse a Value<T> from YAML
///
/// This handles both concrete values and Jinja templates
///
/// # Arguments
/// * `yaml` - The YAML node to parse
pub fn parse_value<T>(yaml: &MarkedNode) -> ParseResult<Value<T>>
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
pub fn parse_value_with_name<T>(yaml: &MarkedNode, field_name: &str) -> ParseResult<Value<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    parse_value_with_converter(yaml, field_name, &FromStrConverter::new())
}

/// Parse a Value<T> from YAML using a custom converter
///
/// This is the most flexible parsing function that allows custom conversion logic
///
/// # Arguments
/// * `yaml` - The YAML node to parse
/// * `field_name` - Field name for error messages (e.g., "build.number")
/// * `converter` - The converter to use for parsing concrete values
pub fn parse_value_with_converter<T, C>(
    yaml: &MarkedNode,
    field_name: &str,
    converter: &C,
) -> ParseResult<Value<T>>
where
    C: NodeConverter<T>,
{
    let scalar = yaml
        .as_scalar()
        .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", get_span(yaml)))?;

    let s = scalar.as_str();
    let span = *scalar.span();

    // Check if it contains a Jinja template
    if converter.is_template(s) {
        // It's a template
        let template =
            JinjaTemplate::new(s.to_string()).map_err(|e| ParseError::jinja_error(e, span))?;
        Ok(Value::new_template(template, Some(span)))
    } else {
        // Try to parse as concrete value using the converter
        let value = converter.convert_scalar(yaml, field_name)?;
        Ok(Value::new_concrete(value, Some(span)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_concrete_value() {
        let yaml = marked_yaml::parse_yaml(0, "val: 42").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let value: Value<i32> = parse_value(node).unwrap();
        assert!(value.is_concrete());
        assert_eq!(value.as_concrete(), Some(&42));
    }

    #[test]
    fn test_parse_template_value() {
        let yaml = marked_yaml::parse_yaml(0, "val: \"${{ foo }}\"").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let value: Value<String> = parse_value(node).unwrap();
        assert!(value.is_template());
    }

    #[test]
    fn test_parse_string_value() {
        let yaml = marked_yaml::parse_yaml(0, "val: \"hello\"").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let value: Value<String> = parse_value(node).unwrap();
        assert!(value.is_concrete());
        assert_eq!(value.as_concrete(), Some(&"hello".to_string()));
    }
}
