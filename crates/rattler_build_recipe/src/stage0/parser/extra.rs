//! Parser for the Extra section

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ParseError, ParseResult},
    stage0::{extra::Extra, parser::helpers::get_span, parser::list::parse_conditional_list},
};

/// Parse an Extra section from YAML
///
/// The Extra section contains metadata like recipe maintainers.
///
/// Example YAML:
/// ```yaml
/// extra:
///   recipe-maintainers:
///     - alice
///     - bob
/// ```
pub fn parse_extra(yaml: &MarkedNode) -> ParseResult<Extra> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("Extra section must be a mapping")
    })?;

    let mut extra = Extra::default();

    // Parse recipe-maintainers field
    if let Some(maintainers) = mapping.get("recipe-maintainers") {
        extra.recipe_maintainers = parse_conditional_list(maintainers)?;
    }

    // Check for unknown fields
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(key_str, "recipe-maintainers") {
            return Err(ParseError::invalid_value(
                "extra",
                &format!("unknown field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion("valid fields are: recipe-maintainers"));
        }
    }

    Ok(extra)
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
        assert!(extra.recipe_maintainers.is_empty());
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
        assert_eq!(extra.recipe_maintainers.len(), 3);
    }

    #[test]
    fn test_parse_extra_with_template_maintainers() {
        let yaml_str = r#"
  recipe-maintainers:
    - '${{ maintainer_1 }}'
    - bob"#;
        let yaml = parse_yaml_extra(yaml_str);
        let extra = parse_extra(&yaml).unwrap();
        assert_eq!(extra.recipe_maintainers.len(), 2);

        // Verify template variable extraction
        let vars = extra.used_variables();
        assert_eq!(vars, vec!["maintainer_1"]);
    }

    #[test]
    fn test_parse_extra_unknown_field() {
        let yaml_str = r#"
  recipe-maintainers:
    - alice
  unknown-field: value"#;
        let yaml = parse_yaml_extra(yaml_str);
        let result = parse_extra(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
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
        assert!(err.message.as_ref().unwrap().contains("must be a mapping"));
    }
}
