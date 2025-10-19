//! Cache output structures for v1 recipes according to the CEP specification
//!
//! This module defines the cache output type which is an intermediate build artifact
//! that can be inherited by regular package outputs.
//!
//! Cache outputs enable the "fast path" optimization where package outputs that inherit
//! from a cache and specify no explicit build script can skip environment installation
//! and script execution, significantly speeding up the build process.

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
        parser::{
            StandardTryConvert, build::VariantKeyUsage, invalid_field_error, missing_field_error,
            validate_mapping_keys,
        },
    },
};
use marked_yaml::Span;
use rattler_conda_types::PackageName;
use serde::de::{self, Deserialize as DeserializeTrait, Deserializer};
use serde::{Deserialize, Serialize};

use super::glob_vec::GlobVec;
use super::requirements::RunExports;
use super::{Script, Source};

/// A cache output that produces intermediate build artifacts
///
/// Cache outputs can be inherited by package outputs using the `inherit` key in the
/// package output definition. This enables the "fast path" optimization where package
/// outputs that inherit from a cache and have no explicit build script can skip
/// environment installation and script execution, significantly improving build performance.
#[derive(Debug, Clone, Serialize)]
pub struct CacheOutput {
    /// The name of the cache output
    pub name: PackageName,
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

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "lowercase")]
enum Field {
    Name,
    Source,
    Build,
    Requirements,
    RunExports,
    IgnoreRunExports,
    About,
}

fn set_once<T, E: de::Error>(slot: &mut Option<T>, value: T, field: &'static str) -> Result<(), E> {
    if slot.replace(value).is_some() {
        Err(de::Error::duplicate_field(field))
    } else {
        Ok(())
    }
}

fn ensure_cache_build_script<E: de::Error>(build: &CacheBuild) -> Result<(), E> {
    if build.script.is_none() {
        Err(E::custom(
            "cache outputs require an explicit build script (build.script field is required)",
        ))
    } else {
        Ok(())
    }
}

// Manual implementation of Deserialize for CacheOutput
impl<'de> DeserializeTrait<'de> for CacheOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CacheOutputVisitor;

        impl<'de> de::Visitor<'de> for CacheOutputVisitor {
            type Value = CacheOutput;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct CacheOutput")
            }

            fn visit_map<V>(self, mut map: V) -> Result<CacheOutput, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut name = None;
                let mut source = None;
                let mut build: Option<CacheBuild> = None;
                let mut requirements = None;
                let mut run_exports = None;
                let mut ignore_run_exports = None;
                let mut about = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => {
                            set_once(&mut name, map.next_value()?, "name")?;
                        }
                        Field::Source => {
                            set_once(&mut source, map.next_value()?, "source")?;
                        }
                        Field::Build => {
                            set_once(&mut build, map.next_value()?, "build")?;
                        }
                        Field::Requirements => {
                            set_once(&mut requirements, map.next_value()?, "requirements")?;
                        }
                        Field::RunExports => {
                            set_once(&mut run_exports, map.next_value()?, "run_exports")?;
                        }
                        Field::IgnoreRunExports => {
                            set_once(
                                &mut ignore_run_exports,
                                map.next_value()?,
                                "ignore_run_exports",
                            )?;
                        }
                        Field::About => {
                            set_once(&mut about, map.next_value()?, "about")?;
                        }
                    }
                }

                let name = name.ok_or_else(|| de::Error::missing_field("name"))?;
                let source = source.unwrap_or_default();
                let build = build.ok_or_else(|| de::Error::missing_field("build"))?;
                ensure_cache_build_script(&build)?;
                let requirements = requirements.unwrap_or_default();
                let run_exports = run_exports.unwrap_or_default();

                Ok(CacheOutput {
                    name,
                    source,
                    build,
                    requirements,
                    run_exports,
                    ignore_run_exports,
                    about,
                    span: Span::new_blank(),
                })
            }
        }

        deserializer.deserialize_map(CacheOutputVisitor)
    }
}

/// Build configuration specific to cache outputs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheBuild {
    /// The build script - only script key is allowed for cache outputs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,

    /// Variant ignore and use keys for cache outputs
    #[serde(default)]
    pub variant: VariantKeyUsage,

    /// Include files in the cache
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub files: GlobVec,

    /// Setting to control whether to always include a file (even if it is already present in the host env)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub always_include_files: GlobVec,
}

impl CacheBuild {}

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
                        name = Some(name_value.try_convert("cache.name")?);
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
                "inherit" => {}
                _ => {
                    return Err(vec![invalid_field_error(
                        *key.span(),
                        key.as_str(),
                        Some(
                            "valid fields for cache outputs are: 'cache', 'source', 'build', 'requirements', 'run_exports', 'ignore_run_exports', 'about', and 'inherit'",
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

/// Parse cache build section
fn parse_cache_build(value: &RenderedNode) -> Result<CacheBuild, Vec<PartialParsingError>> {
    let build_node = value
        .as_mapping()
        .ok_or_else(|| vec![_partialerror!(*value.span(), ErrorKind::ExpectedMapping)])?;

    let mut build = CacheBuild::default();

    let allowed_keys = ["script", "variant", "files", "always_include_files"];
    validate_mapping_keys(
        build_node,
        &allowed_keys,
        "cache build section (valid keys are 'script', 'variant', 'files', 'always_include_files')",
    )?;

    if let Some(script_value) = build_node.get("script") {
        build.script = Some(script_value.try_convert("cache.build.script")?);
    }

    if let Some(variant_value) = build_node.get("variant") {
        build.variant = variant_value.try_convert("cache.build.variant")?;
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
