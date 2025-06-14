//! Enhanced output structures that support inheritance from cache outputs

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};
use serde::{Deserialize, Serialize};

use super::common_output::InheritSpec;
use super::{About, Build, OutputPackage, Requirements, Source, TestType};

/// An output that can inherit from a cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputWithInherit {
    /// Package information for this output
    pub package: OutputPackage,

    /// Optional inheritance specification
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit: Option<InheritSpec>,

    /// Sources for this output
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,

    /// Build configuration
    #[serde(default)]
    pub build: Build,

    /// Requirements for this output
    #[serde(default)]
    pub requirements: Requirements,

    /// Tests for this output
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestType>,

    /// About information
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<About>,
}

/// Represents the type of output in a multi-output recipe
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputType {
    /// A cache output that produces intermediate artifacts
    Cache(Box<super::cache_output::CacheOutput>),
    /// A regular package output that may inherit from cache
    Package(Box<OutputWithInherit>),
}

impl TryConvertNode<OutputType> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<OutputType, Vec<PartialParsingError>> {
        let mapping = self
            .as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])?;

        if mapping.contains_key("cache") {
            let cache_node = mapping.get("cache").ok_or_else(|| {
                vec![_partialerror!(
                    *self.span(),
                    ErrorKind::MissingField("cache".into())
                )]
            })?;
            let cache_output = cache_node.try_convert("outputs.cache")?;
            return Ok(OutputType::Cache(Box::new(cache_output)));
        }

        if mapping.contains_key("package") {
            let output = mapping.try_convert(name)?;
            return Ok(OutputType::Package(Box::new(output)));
        }

        Err(vec![_partialerror!(
            *self.span(),
            ErrorKind::ExpectedMapping,
            help = "output must have either 'cache' or 'package' key"
        )])
    }
}

impl TryConvertNode<OutputWithInherit> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<OutputWithInherit, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<OutputWithInherit> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<OutputWithInherit, Vec<PartialParsingError>> {
        let mut package = None;
        let mut inherit = None;
        let mut source = Vec::new();
        let mut build = Build::default();
        let mut requirements = Requirements::default();
        let mut tests = Vec::new();
        let mut about = None;

        for (key, value) in self.iter() {
            match key.as_str() {
                "package" => {
                    package = Some(value.try_convert("output.package")?);
                }
                "inherit" => {
                    inherit = Some(value.try_convert("output.inherit")?);
                }
                "source" => {
                    source = value.try_convert("output.source")?;
                }
                "build" => {
                    build = value.try_convert("output.build")?;
                }
                "requirements" => {
                    requirements = value.try_convert("output.requirements")?;
                }
                "tests" => {
                    tests = value.try_convert("output.tests")?;
                }
                "about" => {
                    about = Some(value.try_convert("output.about")?);
                }
                _ => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(key.as_str().to_string().into())
                    )]);
                }
            }
        }

        let package = package.ok_or_else(|| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("package".to_string().into()),
                help = "outputs must have a 'package' field"
            )]
        })?;

        Ok(OutputWithInherit {
            package,
            inherit,
            source,
            build,
            requirements,
            tests,
            about,
        })
    }
}

impl OutputWithInherit {
    /// Check if this output inherits from top-level
    pub fn inherits_from_toplevel(&self) -> bool {
        self.inherit.is_none()
    }

    /// Apply inheritance from a cache output
    pub fn apply_cache_inheritance(&mut self, cache: &super::cache_output::CacheOutput) {
        let script_backup = self.build.script.clone();
        self.build.script = script_backup;

        if let Some(inherit_spec) = &self.inherit {
            if inherit_spec.inherit_run_exports() {
                if let Some(cache_ignore) = &cache.ignore_run_exports {
                    self.requirements.ignore_run_exports =
                        self.requirements.ignore_run_exports(Some(cache_ignore));
                }
            }
        }
    }
}
