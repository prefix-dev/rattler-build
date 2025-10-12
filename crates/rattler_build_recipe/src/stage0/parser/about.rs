//! Parser for the About section

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ParseError, ParseResult},
    span::SpannedString,
    stage0::{
        about::About,
        parser::{helpers::get_span, list::parse_conditional_list, value::parse_value},
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
        let spanned = SpannedString::from(scalar);
        let s = spanned.as_str();

        // Check if it's a template
        if s.contains("${{") && s.contains("}}") {
            let template = JinjaTemplate::new(s.to_string())
                .map_err(|e| ParseError::jinja_error(e, spanned.span()))?;
            let items = vec![Item::Value(Value::Template(template))];
            return Ok(ConditionalList::new(items));
        }

        // Plain string
        let items = vec![Item::Value(Value::Concrete(s.to_string()))];
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
///   summary: A short description
///   description: A longer description
///   documentation: https://docs.example.com
///   repository: https://github.com/example/repo
/// ```
pub fn parse_about(yaml: &MarkedNode) -> ParseResult<About> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("About section must be a mapping")
    })?;

    let mut about = About::default();

    // Parse each optional field
    if let Some(homepage) = mapping.get("homepage") {
        about.homepage = Some(parse_value(homepage)?);
    }

    if let Some(license) = mapping.get("license") {
        about.license = Some(parse_value(license)?);
    }

    if let Some(license_file) = mapping.get("license_file") {
        about.license_file = parse_license_file(license_file)?;
    }

    if let Some(license_family) = mapping.get("license_family") {
        about.license_family = Some(parse_value(license_family)?);
    }

    if let Some(summary) = mapping.get("summary") {
        about.summary = Some(parse_value(summary)?);
    }

    if let Some(description) = mapping.get("description") {
        about.description = Some(parse_value(description)?);
    }

    if let Some(documentation) = mapping.get("documentation") {
        about.documentation = Some(parse_value(documentation)?);
    }

    if let Some(repository) = mapping.get("repository") {
        about.repository = Some(parse_value(repository)?);
    }

    // Check for unknown fields to provide helpful error messages
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(
            key_str,
            "homepage"
                | "license"
                | "license_file"
                | "license_family"
                | "summary"
                | "description"
                | "documentation"
                | "repository"
        ) {
            return Err(ParseError::invalid_value(
                "about",
                &format!("unknown field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion(
                "valid fields are: homepage, license, license_file, license_family, summary, description, documentation, repository"
            ));
        }
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
        match about.homepage.as_ref().unwrap() {
            crate::stage0::types::Value::Concrete(url) => {
                assert_eq!(url.as_str(), "https://example.com/");
            }
            _ => panic!("Expected concrete value"),
        }

        match about.license.as_ref().unwrap() {
            crate::stage0::types::Value::Concrete(license) => {
                assert_eq!(license.0.as_ref(), "MIT");
            }
            _ => panic!("Expected concrete value"),
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
        match about.homepage.as_ref().unwrap() {
            crate::stage0::types::Value::Template(t) => {
                assert_eq!(t.used_variables(), &["homepage"]);
            }
            _ => panic!("Expected template value"),
        }

        match about.summary.as_ref().unwrap() {
            crate::stage0::types::Value::Template(t) => {
                let mut vars = t.used_variables().to_vec();
                vars.sort();
                assert_eq!(vars, vec!["name", "summary"]);
            }
            _ => panic!("Expected template value"),
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
        assert!(!about.license_file.is_empty());
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
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
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
        assert!(err.message.as_ref().unwrap().contains("must be a mapping"));
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
