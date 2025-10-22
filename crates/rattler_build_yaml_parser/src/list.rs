//! List and ListOrItem parsing functions

use marked_yaml::Node as MarkedNode;

use crate::{
    error::ParseResult,
    types::{ListOrItem, Value},
    value::parse_value,
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
    if let Some(sequence) = yaml.as_sequence() {
        // It's a list
        let mut items = Vec::new();
        for item in sequence.iter() {
            let parsed = parse_value(item)?;
            items.push(parsed);
        }
        Ok(ListOrItem::new(items))
    } else {
        // It's a single value
        let item = parse_value(yaml)?;
        Ok(ListOrItem::single(item))
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
        assert!(list.iter().nth(0).unwrap().is_concrete());
        assert!(list.iter().nth(1).unwrap().is_template());
    }
}
