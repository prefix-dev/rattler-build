//! Parser for the Extra section

use indexmap::IndexMap;
use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser::helpers::get_span;

use crate::{error::ParseResult, stage0::extra::Extra};

/// Convert a marked_yaml node to serde_value::Value
fn node_to_yaml_value(node: &MarkedNode) -> serde_value::Value {
    match node {
        MarkedNode::Scalar(s) => {
            // Try to parse as different types
            let val_str = s.as_str();

            // Check for null
            if val_str == "null" || val_str == "~" || s.is_empty() {
                return serde_value::Value::Option(None);
            }

            // Check for boolean
            if let Some(bool) = s.as_bool() {
                return serde_value::Value::Bool(bool);
            }

            // Try to parse as number
            if let Some(i) = s.as_i64() {
                return serde_value::Value::I64(i);
            }
            if let Some(f) = s.as_f64() {
                return serde_value::Value::F64(f);
            }

            // Default to string
            serde_value::Value::String(val_str.to_string())
        }
        MarkedNode::Mapping(m) => {
            let mut map = std::collections::BTreeMap::new();
            for (k, v) in m.iter() {
                let key = serde_value::Value::String(k.as_str().to_string());
                map.insert(key, node_to_yaml_value(v));
            }
            serde_value::Value::Map(map)
        }
        MarkedNode::Sequence(s) => {
            let seq: Vec<serde_value::Value> = s.iter().map(node_to_yaml_value).collect();
            serde_value::Value::Seq(seq)
        }
    }
}

/// Parse an Extra section from YAML
///
/// The Extra section is a free-form mapping that can contain any metadata.
///
/// Example YAML:
/// ```yaml
/// extra:
///   recipe-maintainers:
///     - alice
///     - bob
///   custom-field: value
///   another-field: 123
/// ```
pub fn parse_extra(yaml: &MarkedNode) -> ParseResult<Extra> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        crate::ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
    })?;

    let mut extra_map = IndexMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();

        // Convert the value node to serde_value::Value
        let value = node_to_yaml_value(value_node);

        extra_map.insert(key, value);
    }

    Ok(Extra { extra: extra_map })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml_extra(yaml_str: &str) -> MarkedNode {
        let wrapped = format!("extra:\n{}", yaml_str);
        let root = marked_yaml::parse_yaml(0, &wrapped).expect("Failed to parse test YAML");
        let mapping = root.as_mapping().expect("Expected mapping");
        mapping.get("extra").expect("Field not found").clone()
    }

    #[test]
    fn test_parse_empty_extra() {
        let yaml = parse_yaml_extra("  {}");
        let extra = parse_extra(&yaml).unwrap();
        assert!(extra.extra.is_empty());
    }

    #[test]
    fn test_parse_extra_with_maintainers() {
        let yaml_str = r#"
  recipe-maintainers:
    - alice
    - bob
    - charlie"#;
        let yaml = parse_yaml_extra(yaml_str);
        let extra = parse_extra(&yaml).unwrap();
        assert_eq!(extra.extra.len(), 1);
        assert!(extra.extra.contains_key("recipe-maintainers"));

        // Verify the value is a sequence
        let value = extra.extra.get("recipe-maintainers").unwrap();
        match value {
            serde_value::Value::Seq(seq) => {
                assert_eq!(seq.len(), 3);
            }
            _ => panic!("Expected sequence value"),
        }
    }

    #[test]
    fn test_parse_extra_with_multiple_fields() {
        let yaml_str = r#"
  recipe-maintainers:
    - alice
    - bob
  custom-field: some-value
  numeric-field: 123"#;
        let yaml = parse_yaml_extra(yaml_str);
        let extra = parse_extra(&yaml).unwrap();
        assert_eq!(extra.extra.len(), 3);
        assert!(extra.extra.contains_key("recipe-maintainers"));
        assert!(extra.extra.contains_key("custom-field"));
        assert!(extra.extra.contains_key("numeric-field"));
    }

    #[test]
    fn test_parse_extra_not_mapping() {
        let wrapped = "extra: not a mapping";
        let root = marked_yaml::parse_yaml(0, wrapped).expect("Failed to parse test YAML");
        let mapping = root.as_mapping().expect("Expected mapping");
        let yaml = mapping.get("extra").expect("Field not found");

        let result = parse_extra(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(err_string.contains("mapping") || err_string.contains("expected"));
    }
}
