//! Parser for the Requirements section

use marked_yaml::Node as MarkedNode;
use rattler_build_yaml_parser::{
    NodeConverter, ParseMapping, parse_conditional_list_with_converter,
};
use rattler_conda_types::{MatchSpec, PackageName, ParseStrictness};

use crate::{
    error::{ParseError, ParseResult},
    stage0::{
        SerializableMatchSpec,
        parser::ParseConfig,
        parser::helpers::get_span,
        requirements::{IgnoreRunExports, Requirements, RunExports},
    },
};

struct MatchSpecConverter {
    v3: bool,
}

impl NodeConverter<SerializableMatchSpec> for MatchSpecConverter {
    fn convert_scalar(
        &self,
        node: &MarkedNode,
        field_name: &str,
    ) -> ParseResult<SerializableMatchSpec> {
        let scalar = node
            .as_scalar()
            .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", get_span(node)))?;

        SerializableMatchSpec::parse_with_v3(scalar.as_str(), ParseStrictness::Strict, self.v3)
            .map_err(|e| ParseError::invalid_value(field_name, e.to_string(), *scalar.span()))
    }
}

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
#[cfg(test)]
fn parse_requirements(yaml: &MarkedNode) -> ParseResult<Requirements> {
    parse_requirements_with_config(yaml, ParseConfig::default())
}

pub(crate) fn parse_requirements_with_config(
    yaml: &MarkedNode,
    config: ParseConfig,
) -> ParseResult<Requirements> {
    // Validate field names first
    yaml.validate_keys(
        "requirements",
        &[
            "build",
            "host",
            "run",
            "run_constraints",
            "extras",
            "run_exports",
            "ignore_run_exports",
        ],
    )?;

    let mut requirements = Requirements::default();
    let matchspec_converter = MatchSpecConverter { v3: config.v3 };

    if let Some(build) = yaml.try_get_conditional_list_with("build", &matchspec_converter)? {
        requirements.build = build;
    }

    if let Some(host) = yaml.try_get_conditional_list_with("host", &matchspec_converter)? {
        requirements.host = host;
    }

    if let Some(run) = yaml.try_get_conditional_list_with("run", &matchspec_converter)? {
        requirements.run = run;
    }

    if let Some(run_constraints) =
        yaml.try_get_conditional_list_with("run_constraints", &matchspec_converter)?
    {
        requirements.run_constraints = run_constraints;
    }

    // Handle run_exports and ignore_run_exports with special parsing
    let mapping = yaml
        .as_mapping()
        .ok_or_else(|| ParseError::expected_type("mapping", "non-mapping", get_span(yaml)))?;

    if let Some(extras) = mapping.get("extras") {
        requirements.extras = parse_extras(extras, &matchspec_converter)?;
    }

    if let Some(run_exports) = mapping.get("run_exports") {
        requirements.run_exports = parse_run_exports(run_exports, &matchspec_converter)?;
    }

    if let Some(ignore_run_exports) = mapping.get("ignore_run_exports") {
        requirements.ignore_run_exports = parse_ignore_run_exports(ignore_run_exports)?;
    }

    Ok(requirements)
}

fn parse_extras(
    yaml: &MarkedNode,
    converter: &MatchSpecConverter,
) -> ParseResult<
    std::collections::BTreeMap<
        String,
        rattler_build_yaml_parser::ConditionalList<SerializableMatchSpec>,
    >,
> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("requirements.extras must be a mapping")
    })?;

    let mut extras = std::collections::BTreeMap::new();
    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();
        let deps = parse_conditional_list_with_converter(value_node, converter)?;
        extras.insert(key, deps);
    }

    Ok(extras)
}

/// Parse a RunExports section
///
/// Supports two forms:
/// 1. Direct list (defaults to weak): `run_exports: [pkg1, pkg2]`
/// 2. Mapping with fields: `run_exports: { strong: [pkg1], weak: [pkg2] }`
fn parse_run_exports(yaml: &MarkedNode, converter: &MatchSpecConverter) -> ParseResult<RunExports> {
    // Check if it's a direct list (defaults to weak)
    if yaml.as_sequence().is_some() {
        let weak = parse_conditional_list_with_converter(yaml, converter)?;
        return Ok(RunExports {
            weak,
            ..Default::default()
        });
    }

    // Otherwise, parse as mapping with validation
    yaml.validate_keys(
        "run_exports",
        &[
            "noarch",
            "strong",
            "strong_constraints",
            "weak",
            "weak_constraints",
        ],
    )?;

    let mut run_exports = RunExports::default();

    if let Some(noarch) = yaml.try_get_conditional_list_with("noarch", converter)? {
        run_exports.noarch = noarch;
    }

    if let Some(strong) = yaml.try_get_conditional_list_with("strong", converter)? {
        run_exports.strong = strong;
    }

    if let Some(strong_constraints) =
        yaml.try_get_conditional_list_with("strong_constraints", converter)?
    {
        run_exports.strong_constraints = strong_constraints;
    }

    if let Some(weak) = yaml.try_get_conditional_list_with("weak", converter)? {
        run_exports.weak = weak;
    }

    if let Some(weak_constraints) =
        yaml.try_get_conditional_list_with("weak_constraints", converter)?
    {
        run_exports.weak_constraints = weak_constraints;
    }

    Ok(run_exports)
}

struct IgnoreListConverter;

impl NodeConverter<PackageName> for IgnoreListConverter {
    /// Convert a scalar YAML node to a PackageName (via MatchSpec to make it more lenient)
    ///
    /// # Arguments
    /// * `node` - The YAML node to convert (must be a scalar)
    /// * `field_name` - Field name for error messages (e.g., "build.number")
    ///
    /// # Returns
    /// The converted PackageName or a parse error
    fn convert_scalar(&self, node: &MarkedNode, field_name: &str) -> ParseResult<PackageName> {
        let scalar = node
            .as_scalar()
            .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", get_span(node)))?;

        let s = scalar.as_str();
        let span = *scalar.span();

        let as_match_spec = MatchSpec::from_str(s, rattler_conda_types::ParseStrictness::Strict)
            .map_err(|e| ParseError::invalid_value(field_name, e.to_string(), span))?;

        as_match_spec
            .name
            .into_exact()
            .ok_or(ParseError::invalid_value(
                field_name,
                format!("Could not find exact package name in \"{}\"", s),
                span,
            ))
    }
}

/// Parse an IgnoreRunExports section
pub(crate) fn parse_ignore_run_exports(yaml: &MarkedNode) -> ParseResult<IgnoreRunExports> {
    // Validate field names first
    yaml.validate_keys("ignore_run_exports", &["by_name", "from_package"])?;

    let mut ignore = IgnoreRunExports::default();

    // Parse each optional field using custom converter for PackageName
    if let Some(by_name) = yaml.try_get_conditional_list_with("by_name", &IgnoreListConverter)? {
        ignore.by_name = by_name;
    }

    if let Some(from_package) =
        yaml.try_get_conditional_list_with("from_package", &IgnoreListConverter)?
    {
        ignore.from_package = from_package;
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
    fn test_parse_requirements_with_empty_run() {
        // Test that `run:` with no value is treated as empty, not as an error
        // This is a common pattern in recipes where run dependencies are optional
        let yaml_str = r#"
  host:
    - m2-conda-epoch =20250515
  run:"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert_eq!(reqs.host.len(), 1);
        assert!(
            reqs.run.is_empty(),
            "Empty run: should be treated as empty list"
        );
    }

    #[test]
    fn test_parse_requirements_with_null_run() {
        // Test that `run: null` is treated as empty
        let yaml_str = r#"
  host:
    - pkg
  run: null"#;
        let yaml = parse_yaml_requirements(yaml_str);
        let reqs = parse_requirements(&yaml).unwrap();

        assert_eq!(reqs.host.len(), 1);
        assert!(
            reqs.run.is_empty(),
            "run: null should be treated as empty list"
        );
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
    fn test_parse_v3_matchspec_requires_config() {
        let yaml_str = r#"
  run:
    - 'foo[extras=[bar], flags=[cuda], when="python >=3.11"]'"#;
        let yaml = parse_yaml_requirements(yaml_str);

        assert!(parse_requirements(&yaml).is_err());

        let reqs = parse_requirements_with_config(&yaml, ParseConfig { v3: true }).unwrap();
        assert_eq!(reqs.run.len(), 1);
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
        // compiler("c") expands to c_compiler and c_compiler_version
        assert!(vars.contains(&"c_compiler".to_string()));
        assert!(vars.contains(&"c_compiler_version".to_string()));
        // pin_subpackage doesn't expand the function name, only extracts variables from arguments
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
        let err_string = err.to_string();
        assert!(err_string.contains("unknown field"));
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
        let err_string = err.to_string();
        assert!(err_string.contains("unknown field"));
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
        let err_string = err.to_string();
        assert!(err_string.contains("unknown field"));
    }
}
