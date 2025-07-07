//! Cache output structures for v1 recipes according to the CEP specification
//!
//! This module defines the cache output type which is an intermediate build artifact
//! that can be inherited by regular package outputs.

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
        parser::{
            StandardTryConvert, invalid_field_error, missing_field_error, parse_required_string,
            validate_mapping_keys,
        },
    },
};
use serde::{Deserialize, Serialize};

use super::{Script, Source};

/// A cache output that produces intermediate build artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheOutput {
    /// The name of the cache output
    pub name: String,
    /// Sources for this cache output
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,
    /// Build configuration for the cache
    pub build: CacheBuild,
    /// Requirements for building the cache (only build and host allowed)
    pub requirements: CacheRequirements,
    /// Run exports to ignore
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_run_exports: Option<super::requirements::IgnoreRunExports>,
}

/// Build configuration specific to cache outputs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheBuild {
    /// The build script - only script key is allowed for cache outputs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
}

/// Requirements specific to cache outputs (no run or run_constraints allowed)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheRequirements {
    /// Build requirements
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<super::requirements::Dependency>,
    /// Host requirements
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<super::requirements::Dependency>,
}

// Implement StandardTryConvert for automatic RenderedNode conversion
impl StandardTryConvert for CacheOutput {
    const TYPE_NAME: &'static str = "CacheOutput";

    fn from_mapping(
        mapping: &RenderedMappingNode,
        _name: &str,
    ) -> Result<Self, Vec<PartialParsingError>> {
        let mut name = None;
        let mut source = Vec::new();
        let mut build = CacheBuild::default();
        let mut requirements = CacheRequirements::default();
        let mut ignore_run_exports = None;

        for (key, value) in mapping.iter() {
            match key.as_str() {
                "cache" => {
                    // Parse the cache mapping
                    let cache_mapping = value.as_mapping().ok_or_else(|| {
                        vec![_partialerror!(*value.span(), ErrorKind::ExpectedMapping)]
                    })?;

                    // Validate that only 'name' is allowed inside cache
                    validate_mapping_keys(cache_mapping, &["name"], "cache")?;

                    // Parse the name field
                    if let Some(name_value) = cache_mapping.get("name") {
                        name = Some(parse_required_string(name_value, "name", "cache")?);
                    }
                }
                "source" => {
                    source = value.try_convert("cache.source")?;
                }
                "build" => {
                    build = parse_cache_build(value)?;
                }
                "requirements" => {
                    requirements = parse_cache_requirements(value)?;
                }
                "ignore_run_exports" => {
                    ignore_run_exports = Some(value.try_convert("cache.ignore_run_exports")?);
                }
                _ => {
                    return Err(vec![invalid_field_error(
                        *key.span(),
                        key.as_str(),
                        Some(
                            "valid fields for cache outputs are: 'cache', 'source', 'build', 'requirements', and 'ignore_run_exports'",
                        ),
                    )]);
                }
            }
        }

        let name = name.ok_or_else(|| {
            vec![missing_field_error(
                *mapping.span(),
                "name",
                "cache outputs",
            )]
        })?;

        Ok(CacheOutput {
            name,
            source,
            build,
            requirements,
            ignore_run_exports,
        })
    }
}

/// Parse cache build section, ensuring only 'script' is allowed
fn parse_cache_build(value: &RenderedNode) -> Result<CacheBuild, Vec<PartialParsingError>> {
    let build_node = value
        .as_mapping()
        .ok_or_else(|| vec![_partialerror!(*value.span(), ErrorKind::ExpectedMapping)])?;

    let mut build = CacheBuild::default();

    // Validate allowed keys
    validate_mapping_keys(
        build_node,
        &["script"],
        "cache build section (only 'script' is allowed)",
    )?;

    if let Some(script_value) = build_node.get("script") {
        build.script = Some(script_value.try_convert("cache.build.script")?);
    }

    Ok(build)
}

/// Parse cache requirements, ensuring only 'build' and 'host' are allowed
fn parse_cache_requirements(
    value: &RenderedNode,
) -> Result<CacheRequirements, Vec<PartialParsingError>> {
    let req_node = value
        .as_mapping()
        .ok_or_else(|| vec![_partialerror!(*value.span(), ErrorKind::ExpectedMapping)])?;

    let mut requirements = CacheRequirements::default();
    let mut errors = Vec::new();

    for (req_key, req_value) in req_node.iter() {
        match req_key.as_str() {
            "build" => match req_value.try_convert("cache.requirements.build") {
                Ok(deps) => requirements.build = deps,
                Err(errs) => errors.extend(errs),
            },
            "host" => match req_value.try_convert("cache.requirements.host") {
                Ok(deps) => requirements.host = deps,
                Err(errs) => errors.extend(errs),
            },
            "run" | "run_constraints" => {
                errors.push(invalid_field_error(
                    *req_key.span(),
                    req_key.as_str(),
                    Some("cache outputs cannot have 'run' or 'run_constraints' requirements"),
                ));
            }
            _ => {
                errors.push(invalid_field_error(
                    *req_key.span(),
                    req_key.as_str(),
                    Some("cache outputs can only have 'build' and 'host' requirements"),
                ));
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(requirements)
}

// For compatibility with Vec<CacheOutput> parsing
impl TryConvertNode<CacheOutput> for crate::recipe::custom_yaml::RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<CacheOutput, Vec<PartialParsingError>> {
        Err(vec![_partialerror!(
            *self.span(),
            ErrorKind::ExpectedMapping,
            help = "cache outputs must be mappings, not scalars"
        )])
    }
}

#[cfg(test)]
mod tests {
    use crate::recipe::parser::{OutputType, find_outputs_v2, resolve_inheritance};

    #[test]
    fn test_basic_cache_output() {
        let yaml = r#"
schema_version: 1

outputs:
  - cache:
      name: foo-cache

    source:
      - url: https://foo.bar/source.tar.bz2
        sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

    requirements:
      build:
        - gcc
        - cmake
        - ninja
      host:
        - libzlib
        - libfoo

    build:
      script: build_cache.sh

  - package:
      name: foo-headers

    inherit:
      from: foo-cache
      run_exports: false

    build:
      files:
        - include/

  - package:
      name: foo

    inherit: foo-cache
"#;

        let outputs = find_outputs_v2(yaml).expect("Failed to parse outputs");
        assert_eq!(outputs.len(), 3);

        match &outputs[0] {
            OutputType::Cache(cache) => {
                assert_eq!(cache.name, "foo-cache");
                assert_eq!(cache.source.len(), 1);
                assert_eq!(cache.requirements.build.len(), 3);
                assert_eq!(cache.requirements.host.len(), 2);
            }
            _ => panic!("Expected cache output"),
        }

        match &outputs[1] {
            OutputType::Package(pkg) => {
                assert_eq!(pkg.package.name().as_normalized(), "foo-headers");
                assert!(pkg.inherit.is_some());
                let inherit = pkg.inherit.as_ref().unwrap();
                assert_eq!(inherit.cache_name(), "foo-cache");
                assert!(!inherit.inherit_run_exports());
            }
            _ => panic!("Expected package output"),
        }

        match &outputs[2] {
            OutputType::Package(pkg) => {
                assert_eq!(pkg.package.name().as_normalized(), "foo");
                assert!(pkg.inherit.is_some());
                let inherit = pkg.inherit.as_ref().unwrap();
                assert_eq!(inherit.cache_name(), "foo-cache");
                assert!(inherit.inherit_run_exports());
            }
            _ => panic!("Expected package output"),
        }
    }

    #[test]
    fn test_file_patterns() {
        let yaml = r#"
outputs:
  - cache:
      name: build-cache

    build:
      script: |
        mkdir -p include lib bin
        touch include/foo.h lib/libfoo.so bin/foo

  - package:
      name: headers

    inherit: build-cache

    build:
      files:
        - include/**/*.h

  - package:
      name: libs

    inherit: build-cache

    build:
      files:
        include:
          - lib/**/*.so
        exclude:
          - lib/**/*.a
"#;

        let outputs = find_outputs_v2(yaml).expect("Failed to parse outputs");
        assert_eq!(outputs.len(), 3);
        match &outputs[1] {
            OutputType::Package(pkg) => {
                // Files are in build.files, not pkg.files
                assert!(!pkg.build.files.is_empty());
            }
            _ => panic!("Expected package output"),
        }
    }

    #[test]
    fn test_cache_without_run_requirements() {
        // This should succeed - cache outputs cannot have run requirements
        let yaml = r#"
outputs:
  - cache:
      name: test-cache

    requirements:
      build:
        - cmake
      host:
        - libfoo
"#;

        let outputs = find_outputs_v2(yaml).expect("Failed to parse outputs");
        assert_eq!(outputs.len(), 1);
    }

    #[test]
    fn test_cache_with_run_requirements_fails() {
        // This should fail - cache outputs cannot have run requirements
        let yaml = r#"
outputs:
  - cache:
      name: test-cache

    requirements:
      run:
        - python
"#;

        let result = find_outputs_v2(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_inheritance_resolution() {
        let yaml = r#"
outputs:
  - cache:
      name: common-build

    source:
      - url: https://example.com/source.tar.gz
        sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

    build:
      script: build.sh

  - package:
      name: pkg1

    inherit: common-build

  - package:
      name: pkg2

    inherit:
      from: common-build
      run_exports: false
"#;

        let mut outputs = find_outputs_v2(yaml).expect("Failed to parse outputs");
        resolve_inheritance(&mut outputs).expect("Failed to resolve inheritance");

        for output in outputs.iter().skip(1).take(2) {
            match output {
                OutputType::Package(pkg) => {
                    assert!(pkg.inherit.is_some());
                }
                _ => panic!("Expected package output"),
            }
        }
    }
}
