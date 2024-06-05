//! Functions to deal with the build cache
use std::collections::{BTreeMap, HashSet};

use sha2::{Digest, Sha256};

use crate::{metadata::Output, recipe::parser::Dependency};

/// Error type for cache key generation
#[derive(Debug, thiserror::Error)]
pub enum CacheKeyError {
    /// No cache key available (when no `cache` section is present in the recipe)
    #[error("No cache key available")]
    NoCacheKeyAvailable,
    /// Error serializing cache key with serde_json
    #[error("Error serializing cache: {0}")]
    Serde(#[from] serde_json::Error),
}

impl Output {
    /// Compute a cache key that contains all the information that was used to build the cache,
    /// including the relevant variant information.
    pub fn cache_key(&self) -> Result<String, CacheKeyError> {
        // we have a variant, and we need to find the used variables that are used in the cache to create a
        // hash for the cache ...
        if let Some(cache) = &self.recipe.cache {
            // we need to apply the variant to the cache requirements though
            let mut requirement_names = cache
                .requirements
                .build_time()
                .filter_map(|x| {
                    if let Dependency::Spec(spec) = x {
                        if spec.version.is_none() && spec.build.is_none() {
                            if let Some(name) = spec.name.as_ref() {
                                return Some(name.as_normalized().to_string());
                            }
                        }
                    }
                    None
                })
                .collect::<HashSet<_>>();
            // always insert the target platform and build platform
            requirement_names.insert("target_platform".to_string());
            requirement_names.insert("build_platform".to_string());

            // intersect variant with requirements
            let mut selected_variant = BTreeMap::new();
            for key in requirement_names.iter() {
                if let Some(value) = self.variant().get(key) {
                    selected_variant.insert(key, value.clone());
                }
            }

            let cache_key = (cache, selected_variant);
            // serialize to json and hash
            let mut hasher = Sha256::new();
            let serialized = serde_json::to_string(&cache_key)?;
            hasher.update(serialized.as_bytes());
            let result = hasher.finalize();
            Ok(format!("{:x}", result))
        } else {
            Err(CacheKeyError::NoCacheKeyAvailable)
        }
    }
}
