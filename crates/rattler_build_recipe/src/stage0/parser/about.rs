//! Parser for the About section

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser::{
    ParseMapping, helpers::contains_jinja_template, parse_conditional_list, parse_value_with_name,
};

use crate::{
    error::{ParseError, ParseResult},
    stage0::{
        about::{About, License},
        parser::helpers::get_span,
        types::{ConditionalList, Item, JinjaTemplate, Value},
    },
};

/// Parse license_file field which can be either a single string or a list of strings
fn parse_license_file(yaml: &MarkedNode) -> ParseResult<ConditionalList<String>> {
    // Try parsing as a sequence first
    if yaml.as_sequence().is_some() {
        return parse_conditional_list(yaml);
    }

    // Try parsing as a single scalar string
    if let Some(scalar) = yaml.as_scalar() {
        let s = scalar.as_str();
        let span = *scalar.span();

        // Check if it's a template
        if contains_jinja_template(s) {
            let template =
                JinjaTemplate::new(s.to_string()).map_err(|e| ParseError::jinja_error(e, span))?;
            let items = vec![Item::Value(Value::new_template(template, Some(span)))];
            return Ok(ConditionalList::new(items));
        }

        // Plain string
        let items = vec![Item::Value(Value::new_concrete(s.to_string(), Some(span)))];
        return Ok(ConditionalList::new(items));
    }

    // Get proper span for better error message
    let span = get_span(yaml);
    Err(
        ParseError::expected_type("string or list of strings", "other", span)
            .with_message("license_file must be a string or a list of strings"),
    )
}

/// Parse an About section from YAML
///
/// All fields in the About section are optional and can contain templates.
///
/// Example YAML:
/// ```yaml
/// about:
///   homepage: https://example.com
///   license: MIT
///   license_file: LICENSE
///   license_family: MIT
///   summary: A short description
///   description: A longer description
///   documentation: https://docs.example.com
///   repository: https://github.com/example/repo
/// ```
pub fn parse_about(yaml: &MarkedNode) -> ParseResult<About> {
    // Validate field names first
    yaml.validate_keys(
        "about",
        &[
            "homepage",
            "license",
            "license_file",
            "license_family",
            "summary",
            "description",
            "documentation",
            "repository",
        ],
    )?;

    let mut about = About {
        homepage: yaml.try_get_field("homepage")?,
        license: None,
        license_family: yaml.try_get_field("license_family")?,
        summary: yaml.try_get_field("summary")?,
        description: yaml.try_get_field("description")?,
        documentation: yaml.try_get_field("documentation")?,
        repository: yaml.try_get_field("repository")?,
        license_file: Default::default(),
    };

    // Parse license using the generic value parser - LicenseParseError provides helpful messages
    if let Some(license_node) = yaml.as_mapping().and_then(|m| m.get("license")) {
        about.license = Some(parse_value_with_name::<License>(license_node, "license")?);
    }

    // Handle license_file specially since it can be a single value or list
    if let Some(license_file_node) = yaml.as_mapping().and_then(|m| m.get("license_file")) {
        about.license_file = Some(parse_license_file(license_file_node)?);
    }

    Ok(about)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml_about(yaml_str: &str) -> MarkedNode {
        let wrapped = format!("about:\n{}", yaml_str);
        let root = marked_yaml::parse_yaml(0, &wrapped).expect("Failed to parse test YAML");
        let mapping = root.as_mapping().expect("Expected mapping");
        mapping.get("about").expect("Field not found").clone()
    }

    #[test]
    fn test_parse_empty_about() {
        let yaml = parse_yaml_about("  {}");
        let about = parse_about(&yaml).unwrap();
        assert!(about.homepage.is_none());
        assert!(about.license.is_none());
        assert!(about.summary.is_none());
    }

    #[test]
    fn test_parse_about_with_concrete_values() {
        let yaml_str = r#"
  homepage: https://example.com
  license: MIT
  summary: A test package"#;
        let yaml = parse_yaml_about(yaml_str);
        let about = parse_about(&yaml).unwrap();

        assert!(about.homepage.is_some());
        assert!(about.license.is_some());
        assert!(about.summary.is_some());

        // Verify concrete values
        if let Some(url) = about.homepage.as_ref().unwrap().as_concrete() {
            assert_eq!(url.as_str(), "https://example.com/");
        } else {
            panic!("Expected concrete value");
        }

        if let Some(license) = about.license.as_ref().unwrap().as_concrete() {
            assert_eq!(license.0.as_ref(), "MIT");
        } else {
            panic!("Expected concrete value");
        }
    }

    #[test]
    fn test_parse_about_with_templates() {
        let yaml_str = r#"
  homepage: '${{ homepage }}'
  license: '${{ license }}'
  summary: '${{ name }} - ${{ summary }}'"#;
        let yaml = parse_yaml_about(yaml_str);
        let about = parse_about(&yaml).unwrap();

        // Verify templates
        if let Some(t) = about.homepage.as_ref().unwrap().as_template() {
            assert_eq!(t.used_variables(), &["homepage"]);
        } else {
            panic!("Expected template value");
        }

        if let Some(t) = about.summary.as_ref().unwrap().as_template() {
            let mut vars = t.used_variables().to_vec();
            vars.sort();
            assert_eq!(vars, vec!["name", "summary"]);
        } else {
            panic!("Expected template value");
        }
    }

    #[test]
    fn test_parse_about_all_fields() {
        let yaml_str = r#"
  homepage: https://example.com
  license: Apache-2.0
  license_file: LICENSE
  summary: A test package
  description: A longer description
  documentation: https://docs.example.com
  repository: https://github.com/example/repo"#;
        let yaml = parse_yaml_about(yaml_str);
        let about = parse_about(&yaml).unwrap();

        assert!(about.homepage.is_some());
        assert!(about.license.is_some());
        assert!(!about.license_file.is_none());
        assert!(about.summary.is_some());
        assert!(about.description.is_some());
        assert!(about.documentation.is_some());
        assert!(about.repository.is_some());
    }

    #[test]
    fn test_parse_about_unknown_field() {
        let yaml_str = r#"
  homepage: https://example.com
  unknown_field: some value"#;
        let yaml = parse_yaml_about(yaml_str);
        let result = parse_about(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(err_string.contains("unknown field"));
    }

    #[test]
    fn test_parse_about_not_mapping() {
        let wrapped = "about: not a mapping";
        let root = marked_yaml::parse_yaml(0, wrapped).expect("Failed to parse test YAML");
        let mapping = root.as_mapping().expect("Expected mapping");
        let yaml = mapping.get("about").expect("Field not found");

        let result = parse_about(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(err_string.contains("mapping") || err_string.contains("expected"));
    }

    #[test]
    fn test_parse_about_partial_fields() {
        let yaml_str = r#"
  license: MIT
  summary: Just a summary"#;
        let yaml = parse_yaml_about(yaml_str);
        let about = parse_about(&yaml).unwrap();

        assert!(about.homepage.is_none());
        assert!(about.license.is_some());
        assert!(about.summary.is_some());
        assert!(about.repository.is_none());
    }
}
