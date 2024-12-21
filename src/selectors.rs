//! Contains the selector config, which is used to render the recipe.

use std::collections::BTreeMap;

use crate::{
    hash::HashInfo,
    normalized_key::NormalizedKey,
    recipe::jinja::{Env, Git},
};

use minijinja::value::Value;
use rattler_conda_types::Platform;
use strum::IntoEnumIterator;

/// The selector config is used to render the recipe.
#[derive(Clone, Debug)]
pub struct SelectorConfig {
    /// The target platform to render for
    pub target_platform: Platform,
    /// The host platform (relevant for `noarch`)
    pub host_platform: Platform,
    /// The build platform to render for
    pub build_platform: Platform,
    /// The hash, if available
    pub hash: Option<HashInfo>,
    /// The build number if available
    pub build_number: Option<u64>,
    /// The variant config
    pub variant: BTreeMap<NormalizedKey, String>,
    /// Enable experimental features
    pub experimental: bool,
    /// Allow undefined variables
    pub allow_undefined: bool,
}

impl SelectorConfig {
    /// Turn this selector config into a context for jinja rendering
    pub fn into_context(self) -> BTreeMap<String, Value> {
        let mut context = BTreeMap::new();

        context.insert(
            "target_platform".to_string(),
            Value::from_safe_string(self.target_platform.to_string()),
        );

        context.insert(
            "host_platform".to_string(),
            Value::from_safe_string(self.host_platform.to_string()),
        );

        for platform in Platform::iter() {
            if let Some(only_platform) = platform.only_platform() {
                context.insert(
                    only_platform.to_string(),
                    Value::from(self.host_platform == platform),
                );
            }

            if let Some(arch) = platform.arch() {
                context.insert(
                    arch.to_string(),
                    Value::from(self.host_platform.arch() == Some(arch)),
                );
            }
        }

        context.insert(
            "unix".to_string(),
            Value::from(self.host_platform.is_unix()),
        );

        context.insert(
            "build_platform".to_string(),
            Value::from_safe_string(self.build_platform.to_string()),
        );

        if let Some(hash) = self.hash {
            context.insert("hash".to_string(), Value::from_safe_string(hash.hash));
        }

        context.insert("env".to_string(), Value::from_object(Env));
        context.insert(
            "git".to_string(),
            Value::from_object(Git {
                // only enable git if experimental is enabled
                experimental: self.experimental,
            }),
        );

        for (key, v) in self.variant {
            context.insert(key.normalize(), Value::from_safe_string(v));
        }

        context
    }

    /// Create a new selector config from an existing one, replacing the variant
    pub fn with_variant(
        self,
        variant: BTreeMap<NormalizedKey, String>,
        target_platform: Platform,
    ) -> Self {
        Self {
            variant,
            target_platform,
            ..self
        }
    }

    /// Finish the selector config by adding the hash and build number. 
    /// After this, all variables are defined and `allow_undefined` is set to false.
    pub fn finish(self, hash: HashInfo, build_number: u64) -> Self {
        Self {
            hash: Some(hash),
            build_number: Some(build_number),
            allow_undefined: false,
            ..self
        }
    }
}

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            target_platform: Platform::current(),
            host_platform: Platform::current(),
            build_platform: Platform::current(),
            hash: None,
            build_number: None,
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
        }
    }
}
