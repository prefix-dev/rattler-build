//! Parser for the Package section

use marked_yaml::Node as MarkedNode;

use std::str::FromStr;

use crate::{
    error::{ParseError, ParseResult},
    span::SpannedString,
    stage0::{package::Package, parser::helpers::get_span},
};

/// Parse a Package section from YAML
///
/// The Package section contains the package name and version.
///
/// Example YAML:
/// ```yaml
/// package:
///   name: my-package
///   version: 1.0.0
/// ```
pub fn parse_package(yaml: &MarkedNode) -> ParseResult<Package> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("Package section must be a mapping")
    })?;

    // Parse required 'name' field
    let name_node = mapping
        .get("name")
        .ok_or_else(|| ParseError::missing_field("name", get_span(yaml)))?;

    let name_scalar = name_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(name_node))
            .with_message("Package name must be a scalar")
    })?;

    let name_spanned = SpannedString::from(name_scalar);
    let name_str = name_spanned.as_str();

    // Parse the name - check if it's a template or concrete value
    let name = if name_str.contains("${{") && name_str.contains("}}") {
        // Template
        let template = crate::stage0::types::JinjaTemplate::new(name_str.to_string())
            .map_err(|e| ParseError::jinja_error(e, name_spanned.span()))?;
        crate::stage0::types::Value::Template(template)
    } else {
        // Concrete package name
        let package_name = rattler_conda_types::PackageName::try_from(name_str)
            .map_err(|e| ParseError::invalid_value("name", &e.to_string(), name_spanned.span()))?;
        crate::stage0::types::Value::Concrete(crate::stage0::package::PackageName(package_name))
    };

    // Parse required 'version' field
    let version_node = mapping
        .get("version")
        .ok_or_else(|| ParseError::missing_field("version", get_span(yaml)))?;

    let version_scalar = version_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(version_node))
            .with_message("Package version must be a scalar")
    })?;

    let version_spanned = SpannedString::from(version_scalar);
    let version_str = version_spanned.as_str();

    // Parse the version - check if it's a template or concrete value
    let version = if version_str.contains("${{") && version_str.contains("}}") {
        // Template
        let template = crate::stage0::types::JinjaTemplate::new(version_str.to_string())
            .map_err(|e| ParseError::jinja_error(e, version_spanned.span()))?;
        crate::stage0::types::Value::Template(template)
    } else {
        // Concrete version
        let version_with_source = rattler_conda_types::VersionWithSource::from_str(version_str)
            .map_err(|e| {
                ParseError::invalid_value("version", &e.to_string(), version_spanned.span())
            })?;
        crate::stage0::types::Value::Concrete(version_with_source)
    };

    // Check for unknown fields
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(key_str, "name" | "version") {
            return Err(ParseError::invalid_value(
                "package",
                &format!("unknown field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion("valid fields are: name, version"));
        }
    }

    Ok(Package { name, version })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml_package(yaml_str: &str) -> MarkedNode {
        let wrapped = format!("package:\n{}", yaml_str);
        let root = marked_yaml::parse_yaml(0, &wrapped).expect("Failed to parse test YAML");
        let mapping = root.as_mapping().expect("Expected mapping");
        mapping.get("package").expect("Field not found").clone()
    }

    #[test]
    fn test_parse_package_concrete() {
        let yaml_str = r#"
  name: my-package
  version: 1.0.0"#;
        let yaml = parse_yaml_package(yaml_str);
        let package = parse_package(&yaml).unwrap();

        // Check that both fields are concrete
        assert!(package.name.is_concrete());
        assert!(package.version.is_concrete());

        // Check name
        match package.name {
            crate::stage0::types::Value::Concrete(ref pkg_name) => {
                assert_eq!(pkg_name.to_string(), "my-package");
            }
            _ => panic!("Expected concrete name"),
        }
    }

    #[test]
    fn test_parse_package_with_templates() {
        let yaml_str = r#"
  name: '${{ name }}'
  version: '${{ version }}'"#;
        let yaml = parse_yaml_package(yaml_str);
        let package = parse_package(&yaml).unwrap();

        // Check templates
        assert!(package.name.is_template());
        assert!(package.version.is_template());

        // Check variables
        let vars = package.used_variables();
        let mut sorted_vars = vars.clone();
        sorted_vars.sort();
        assert_eq!(sorted_vars, vec!["name", "version"]);
    }

    #[test]
    fn test_parse_package_missing_name() {
        let yaml_str = r#"
  version: 1.0.0"#;
        let yaml = parse_yaml_package(yaml_str);
        let result = parse_package(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("missing"));
        assert!(err.message.as_ref().unwrap().contains("name"));
    }

    #[test]
    fn test_parse_package_missing_version() {
        let yaml_str = r#"
  name: my-package"#;
        let yaml = parse_yaml_package(yaml_str);
        let result = parse_package(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("missing"));
        assert!(err.message.as_ref().unwrap().contains("version"));
    }

    #[test]
    fn test_parse_package_invalid_name() {
        let yaml_str = r#"
  name: "Invalid Name With Spaces"
  version: 1.0.0"#;
        let yaml = parse_yaml_package(yaml_str);
        let result = parse_package(&yaml);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_package_unknown_field() {
        let yaml_str = r#"
  name: my-package
  version: 1.0.0
  unknown: field"#;
        let yaml = parse_yaml_package(yaml_str);
        let result = parse_package(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
    }
}
