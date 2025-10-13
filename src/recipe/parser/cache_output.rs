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
use super::requirements::RunExports;
use super::glob_vec::GlobVec;

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
    /// Run exports declared by the cache; can be inherited by packages
    #[serde(default, skip_serializing_if = "RunExports::is_empty")]
    pub run_exports: RunExports,
    /// Run exports to ignore
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_run_exports: Option<super::requirements::IgnoreRunExports>,
    /// About information that can be inherited by packages
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<super::About>,
    /// Span of the output mapping (for diagnostics)
    #[serde(skip)]
    pub span: marked_yaml::Span,
}

/// Build configuration specific to cache outputs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheBuild {
    /// The build script - only script key is allowed for cache outputs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
    /// Include files in the cache (glob patterns to select files)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub files: GlobVec,
    /// Files that are always included in the cache
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub always_include_files: GlobVec,
}

impl CacheBuild {
    /// Get the files settings for this cache build
    pub fn files(&self) -> &GlobVec {
        &self.files
    }

    /// Get the always_include_files settings for this cache build
    pub fn always_include_files(&self) -> &GlobVec {
        &self.always_include_files
    }
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
        let mut run_exports = RunExports::default();
        let mut ignore_run_exports = None;
        let mut about = None;

        let span = *mapping.span();

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
                "run_exports" => {
                    run_exports = value.try_convert("cache.run_exports")?;
                }
                "ignore_run_exports" => {
                    ignore_run_exports = Some(value.try_convert("cache.ignore_run_exports")?);
                }
                "about" => {
                    about = Some(value.try_convert("cache.about")?);
                }
                "inherit" => {
                    return Err(vec![invalid_field_error(
                        *key.span(),
                        key.as_str(),
                        Some(
                            "cache outputs cannot use 'inherit' - caches must be built from scratch and cannot inherit from packages or other caches",
                        ),
                    )]);
                }
                _ => {
                    return Err(vec![invalid_field_error(
                        *key.span(),
                        key.as_str(),
                        Some(
                            "valid fields for cache outputs are: 'cache', 'source', 'build', 'requirements', 'run_exports', 'ignore_run_exports', and 'about'",
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

        if build.script.is_none() {
            return Err(vec![missing_field_error(
                *mapping.span(),
                "build.script",
                "cache outputs (cache outputs require an explicit build script)",
            )]);
        }

        Ok(CacheOutput {
            name,
            source,
            build,
            requirements,
            run_exports,
            ignore_run_exports,
            about,
            span,
        })
    }
}

/// Parse cache build section, ensuring only allowed keys are present
fn parse_cache_build(value: &RenderedNode) -> Result<CacheBuild, Vec<PartialParsingError>> {
    let build_node = value
        .as_mapping()
        .ok_or_else(|| vec![_partialerror!(*value.span(), ErrorKind::ExpectedMapping)])?;

    let mut build = CacheBuild::default();

    // Validate allowed keys
    validate_mapping_keys(
        build_node,
        &["script", "files", "always_include_files"],
        "cache build section (only 'script', 'files', and 'always_include_files' are allowed)",
    )?;

    if let Some(script_value) = build_node.get("script") {
        build.script = Some(script_value.try_convert("cache.build.script")?);
    }

    if let Some(files_value) = build_node.get("files") {
        build.files = files_value.try_convert("cache.build.files")?;
    }

    if let Some(always_include_value) = build_node.get("always_include_files") {
        build.always_include_files =
            always_include_value.try_convert("cache.build.always_include_files")?;
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
