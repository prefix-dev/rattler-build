//! Parser for the Requirements section

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ParseError, ParseResult},
    stage0::{
        parser::{helpers::get_span, list::parse_conditional_list},
        requirements::{IgnoreRunExports, Requirements, RunExports},
    },
};

/// Parse a Requirements section from YAML
///
/// The Requirements section specifies build, host, run, and other dependencies.
///
/// Example YAML:
/// ```yaml
/// requirements:
///   build:
///     - gcc
///     - make
///   host:
///     - python
///   run:
///     - python
///   run_constraints:
///     - numpy >=1.19
/// ```
pub fn parse_requirements(yaml: &MarkedNode) -> ParseResult<Requirements> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("Requirements section must be a mapping")
    })?;

    let mut requirements = Requirements::default();

    // Parse each optional field
    if let Some(build) = mapping.get("build") {
        requirements.build = parse_conditional_list(build)?;
    }

    if let Some(host) = mapping.get("host") {
        requirements.host = parse_conditional_list(host)?;
    }

    if let Some(run) = mapping.get("run") {
        requirements.run = parse_conditional_list(run)?;
    }

    if let Some(run_constraints) = mapping.get("run_constraints") {
        requirements.run_constraints = parse_conditional_list(run_constraints)?;
    }

    if let Some(run_exports) = mapping.get("run_exports") {
        requirements.run_exports = parse_run_exports(run_exports)?;
    }

    if let Some(ignore_run_exports) = mapping.get("ignore_run_exports") {
        requirements.ignore_run_exports = parse_ignore_run_exports(ignore_run_exports)?;
    }

    // Check for unknown fields
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(
            key_str,
            "build" | "host" | "run" | "run_constraints" | "run_exports" | "ignore_run_exports"
        ) {
            return Err(ParseError::invalid_value(
                "requirements",
                &format!("unknown field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion(
                "valid fields are: build, host, run, run_constraints, run_exports, ignore_run_exports",
            ));
        }
    }

    Ok(requirements)
}

/// Parse a RunExports section
///
/// Supports two forms:
/// 1. Direct list (defaults to weak): `run_exports: [pkg1, pkg2]`
/// 2. Mapping with fields: `run_exports: { strong: [pkg1], weak: [pkg2] }`
fn parse_run_exports(yaml: &MarkedNode) -> ParseResult<RunExports> {
    // Check if it's a direct list (defaults to weak)
    if yaml.as_sequence().is_some() {
        let weak = parse_conditional_list(yaml)?;
        return Ok(RunExports {
            weak,
            ..Default::default()
        });
    }

    // Otherwise, parse as mapping
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping or list", "other", get_span(yaml))
            .with_message("run_exports must be either a list or a mapping")
    })?;

    let mut run_exports = RunExports::default();

    if let Some(noarch) = mapping.get("noarch") {
        run_exports.noarch = parse_conditional_list(noarch)?;
    }

    if let Some(strong) = mapping.get("strong") {
        run_exports.strong = parse_conditional_list(strong)?;
    }

    if let Some(strong_constraints) = mapping.get("strong_constraints") {
        run_exports.strong_constraints = parse_conditional_list(strong_constraints)?;
    }

    if let Some(weak) = mapping.get("weak") {
        run_exports.weak = parse_conditional_list(weak)?;
    }

    if let Some(weak_constraints) = mapping.get("weak_constraints") {
        run_exports.weak_constraints = parse_conditional_list(weak_constraints)?;
    }

    // Check for unknown fields
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(
            key_str,
            "noarch" | "strong" | "strong_constraints" | "weak" | "weak_constraints"
        ) {
            return Err(ParseError::invalid_value(
                "run_exports",
                &format!("unknown field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion(
                "valid fields are: noarch, strong, strong_constraints, weak, weak_constraints",
            ));
        }
    }

    Ok(run_exports)
}

/// Parse an IgnoreRunExports section
fn parse_ignore_run_exports(yaml: &MarkedNode) -> ParseResult<IgnoreRunExports> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("ignore_run_exports must be a mapping")
    })?;

    let mut ignore = IgnoreRunExports::default();

    if let Some(by_name) = mapping.get("by_name") {
        ignore.by_name = parse_conditional_list(by_name)?;
    }

    if let Some(from_package) = mapping.get("from_package") {
        ignore.from_package = parse_conditional_list(from_package)?;
    }

    // Check for unknown fields
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(key_str, "by_name" | "from_package") {
            return Err(ParseError::invalid_value(
                "ignore_run_exports",
                &format!("unknown field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion("valid fields are: by_name, from_package"));
        }
    }

    Ok(ignore)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml_requirements(yaml_str: &str) -> MarkedNode {
        let wrapped = format!("requirements:\n{}", yaml_str);
        let root = marked_yaml::parse_yaml(0, &wrapped).expect("Failed to parse test YAML");
        let mapping = root.as_mapping().expect("Expected mapping");
        mapping
            .get("requirements")
            .expect("Field not found")
            .clone()
    }

    #[test]
    fn test_parse_empty_requirements() {
        let yaml = parse_yaml_requirements("  {}");
        let reqs = parse_requirements(&yaml).unwrap();
        assert!(reqs.is_empty());
    }

    #[test]
    fn test_parse_requirements_with_build() {
        let yaml_str = r#"
  build:
    - gcc
    - make"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert_eq!(reqs.build.len(), 2);
        assert!(reqs.host.is_empty());
        assert!(reqs.run.is_empty());
    }

    #[test]
    fn test_parse_requirements_all_fields() {
        let yaml_str = r#"
  build:
    - gcc
  host:
    - python
  run:
    - python
  run_constraints:
    - numpy >=1.19"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert_eq!(reqs.build.len(), 1);
        assert_eq!(reqs.host.len(), 1);
        assert_eq!(reqs.run.len(), 1);
        assert_eq!(reqs.run_constraints.len(), 1);
    }

    #[test]
    fn test_parse_requirements_with_templates() {
        let yaml_str = r#"
  build:
    - '${{ compiler("c") }}'
  run:
    - '${{ pin_subpackage("mylib", max_pin="x.x") }}'"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        let vars = reqs.used_variables();
        assert!(vars.contains(&"compiler".to_string()));
        assert!(vars.contains(&"pin_subpackage".to_string()));
    }

    #[test]
    fn test_parse_requirements_with_conditionals() {
        let yaml_str = r#"
  build:
    - gcc
    - if: win
      then: vs2019
      else: clang"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert_eq!(reqs.build.len(), 2);

        let vars = reqs.used_variables();
        assert!(vars.contains(&"win".to_string()));
    }

    #[test]
    fn test_parse_requirements_with_run_exports() {
        let yaml_str = r#"
  build:
    - gcc
  run_exports:
    strong:
      - mylib"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert!(!reqs.run_exports.is_empty());
        assert_eq!(reqs.run_exports.strong.len(), 1);
    }

    #[test]
    fn test_parse_run_exports_all_fields() {
        let yaml_str = r#"
  run_exports:
    noarch:
      - dep1
    strong:
      - dep2
    strong_constraints:
      - dep3 >=1.0
    weak:
      - dep4
    weak_constraints:
      - dep5 <2.0"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        let exports = &reqs.run_exports;
        assert_eq!(exports.noarch.len(), 1);
        assert_eq!(exports.strong.len(), 1);
        assert_eq!(exports.strong_constraints.len(), 1);
        assert_eq!(exports.weak.len(), 1);
        assert_eq!(exports.weak_constraints.len(), 1);
    }

    #[test]
    fn test_parse_requirements_with_ignore_run_exports() {
        let yaml_str = r#"
  build:
    - gcc
  ignore_run_exports:
    by_name:
      - libfoo
    from_package:
      - bar"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert!(!reqs.ignore_run_exports.is_empty());
        assert_eq!(reqs.ignore_run_exports.by_name.len(), 1);
        assert_eq!(reqs.ignore_run_exports.from_package.len(), 1);
    }

    #[test]
    fn test_parse_requirements_unknown_field() {
        let yaml_str = r#"
  build:
    - gcc
  unknown_field:
    - value"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let result = parse_requirements(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
    }

    #[test]
    fn test_parse_run_exports_unknown_field() {
        let yaml_str = r#"
  run_exports:
    strong:
      - dep
    unknown:
      - value"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let result = parse_requirements(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
    }

    #[test]
    fn test_parse_ignore_run_exports_unknown_field() {
        let yaml_str = r#"
  ignore_run_exports:
    by_name:
      - dep
    unknown:
      - value"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let result = parse_requirements(&yaml);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
    }
}
