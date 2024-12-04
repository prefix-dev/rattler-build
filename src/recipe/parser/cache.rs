use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
        parser::FlattenErrors,
    },
    validate_keys,
};
use serde::{Deserialize, Serialize};

use super::{Build, Requirements, Source};

/// A cache build that can be used to split up a build into multiple outputs
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Cache {
    /// Sources that are used in the cache build and subsequent output builds
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,
    /// The build configuration for the cache
    pub build: Build,
    /// The requirements for building the cache
    pub requirements: Requirements,
}

impl TryConvertNode<Cache> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Cache, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Cache> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<Cache, Vec<PartialParsingError>> {
        let mut cache = Cache::default();

        validate_keys! {
            cache,
            self.iter(),
            source,
            build,
            requirements
        };

        Ok(cache)
    }
}
