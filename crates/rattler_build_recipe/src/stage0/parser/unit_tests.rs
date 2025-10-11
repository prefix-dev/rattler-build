//! Tests for parser module

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ErrorKind, ParseResult},
    stage0::parser::{parse_conditional_list, parse_value},
};

fn parse_yaml_field(yaml_str: &str, field: &str) -> MarkedNode {
    let wrapped = format!("{}: {}", field, yaml_str);
    let root = marked_yaml::parse_yaml(0, &wrapped).expect("Failed to parse test YAML");
    let mapping = root.as_mapping().expect("Expected mapping");
    mapping.get(field).expect("Field not found").clone()
}

fn parse_yaml_list(yaml_str: &str) -> MarkedNode {
    let wrapped = format!("list:\n{}", yaml_str);
    let root = marked_yaml::parse_yaml(0, &wrapped).expect("Failed to parse test YAML");
    let mapping = root.as_mapping().expect("Expected mapping");
    mapping.get("list").expect("Field not found").clone()
}

#[test]
fn test_parse_concrete_value() {
    let yaml = parse_yaml_field("hello", "value");
    let result: crate::stage0::types::Value<String> = parse_value(&yaml).unwrap();
    match result {
        crate::stage0::types::Value::Concrete(s) => assert_eq!(s, "hello"),
        _ => panic!("Expected concrete value"),
    }
}

#[test]
fn test_parse_template_value() {
    let yaml = parse_yaml_field("'${{ name }}'", "value");
    let result: crate::stage0::types::Value<String> = parse_value(&yaml).unwrap();
    match result {
        crate::stage0::types::Value::Template(t) => {
            assert_eq!(t.as_str(), "${{ name }}");
            assert_eq!(t.used_variables(), &["name"]);
        }
        _ => panic!("Expected template value"),
    }
}

#[test]
fn test_parse_simple_list() {
    let yaml_str = r#"
  - gcc
  - make"#;
    let yaml = parse_yaml_list(yaml_str);
    let result: crate::stage0::types::ConditionalList<String> =
        parse_conditional_list(&yaml).unwrap();
    assert_eq!(result.len(), 2);
}

#[test]
fn test_parse_conditional_list() {
    let yaml_str = r#"
  - gcc
  - if: linux
    then: linux-headers
    else: windows-sdk"#;
    let yaml = parse_yaml_list(yaml_str);
    let result: crate::stage0::types::ConditionalList<String> =
        parse_conditional_list(&yaml).unwrap();
    assert_eq!(result.len(), 2);
}

#[test]
fn test_parse_error_with_span() {
    let yaml_str = r#"
  - item1
  - item2"#;
    let yaml = parse_yaml_list(yaml_str);
    // This should succeed as it's a valid list
    let result: ParseResult<crate::stage0::types::ConditionalList<String>> =
        parse_conditional_list(&yaml);
    assert!(result.is_ok());
}

#[test]
fn test_jinja_error_with_span() {
    // Invalid Jinja template should fail
    let yaml = parse_yaml_field("'${{ invalid jinja }}'", "value");
    let result: ParseResult<crate::stage0::types::Value<String>> = parse_value(&yaml);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind, ErrorKind::JinjaError);
}
