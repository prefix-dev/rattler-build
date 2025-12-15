//! List and ListOrItem parsing functions

use marked_yaml::Node as MarkedNode;

use crate::{
    conditional::parse_item_with_converter,
    converter::{FromStrConverter, NodeConverter},
    error::ParseResult,
    types::{ListOrItem, NestedItemList, Value},
    value::parse_value_with_converter,
};

/// Parse a ListOrItem<Value<T>> from YAML
///
/// This handles both single values and lists of values
///
/// # Arguments
/// * `yaml` - The YAML node to parse
pub fn parse_list_or_item<T>(yaml: &MarkedNode) -> ParseResult<ListOrItem<Value<T>>>
where
    T: std::str::FromStr + ToString,
    T::Err: std::fmt::Display,
{
    parse_list_or_item_with_converter(yaml, &FromStrConverter::new())
}

/// Parse a ListOrItem<Value<T>> from YAML using a custom converter
///
/// This handles both single values and lists of values
///
/// # Arguments
/// * `yaml` - The YAML node to parse
/// * `converter` - The converter to use for parsing concrete values
pub fn parse_list_or_item_with_converter<T, C>(
    yaml: &MarkedNode,
    converter: &C,
) -> ParseResult<ListOrItem<Value<T>>>
where
    C: NodeConverter<T>,
{
    if let Some(sequence) = yaml.as_sequence() {
        // It's a list
        let mut items = Vec::new();
        for item in sequence.iter() {
            let parsed = parse_value_with_converter(item, "item", converter)?;
            items.push(parsed);
        }
        Ok(ListOrItem::new(items))
    } else {
        // It's a single value
        let item = parse_value_with_converter(yaml, "item", converter)?;
        Ok(ListOrItem::single(item))
    }
}

/// Parse a NestedItemList<T> from YAML
///
/// This handles both single values and lists of values, with support for
/// nested if/then/else conditionals at any level.
///
/// # Arguments
/// * `yaml` - The YAML node to parse (can be a scalar, mapping, or sequence)
pub fn parse_nested_item_list<T>(yaml: &MarkedNode) -> ParseResult<NestedItemList<T>>
where
    T: std::str::FromStr + ToString,
    T::Err: std::fmt::Display,
{
    parse_nested_item_list_with_converter(yaml, &FromStrConverter::new())
}

/// Parse a NestedItemList<T> from YAML using a custom converter
///
/// This handles both single values and lists of values, with support for
/// nested if/then/else conditionals at any level.
///
/// # Arguments
/// * `yaml` - The YAML node to parse (can be a scalar, mapping, or sequence)
/// * `converter` - The converter to use for parsing concrete values
pub fn parse_nested_item_list_with_converter<T, C>(
    yaml: &MarkedNode,
    converter: &C,
) -> ParseResult<NestedItemList<T>>
where
    C: NodeConverter<T>,
{
    if let Some(sequence) = yaml.as_sequence() {
        // It's a list - parse each item which can be a value or conditional
        let mut items = Vec::new();
        for item in sequence.iter() {
            let parsed = parse_item_with_converter(item, converter)?;
            items.push(parsed);
        }
        Ok(NestedItemList::new(items))
    } else {
        // It's a single item (scalar, mapping, or conditional)
        let item = parse_item_with_converter(yaml, converter)?;
        Ok(NestedItemList::single(item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_item() {
        let yaml = marked_yaml::parse_yaml(0, "val: 42").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ListOrItem<Value<i32>> = parse_list_or_item(node).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_parse_list() {
        let yaml = marked_yaml::parse_yaml(0, "val: [1, 2, 3]").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ListOrItem<Value<i32>> = parse_list_or_item(node).unwrap();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_parse_mixed_list() {
        let yaml = marked_yaml::parse_yaml(0, "val: [\"hello\", \"${{ world }}\"]").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();
        let list: ListOrItem<Value<String>> = parse_list_or_item(node).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().next().unwrap().is_concrete());
        assert!(list.iter().nth(1).unwrap().is_template());
    }
}
