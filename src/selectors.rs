//! Contains the selector config, which is used to render the recipe.

use std::collections::BTreeMap;

use crate::{hash::HashInfo, recipe::jinja::Env, recipe::jinja::Git};

use minijinja::value::Value;
use rattler_conda_types::Platform;

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
    /// The variant config
    pub variant: BTreeMap<String, String>,
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

        if let Some(platform) = self.host_platform.only_platform() {
            context.insert(
                platform.to_string(),
                Value::from_safe_string(platform.to_string()),
            );
        }

        if let Some(arch) = self.target_platform.arch() {
            context.insert(arch.to_string(), Value::from(true));
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
            match v.to_lowercase().as_str() {
                "true" => context.insert(key.clone(), Value::from(true)),
                "false" => context.insert(key.clone(), Value::from(false)),
                _ => context.insert(key, Value::from_safe_string(v)),
            };
        }

        context
    }

    /// Create a new selector config from an existing one, replacing the variant
    pub fn new_with_variant(
        &self,
        variant: BTreeMap<String, String>,
        target_platform: Platform,
    ) -> Self {
        Self {
            variant,
            target_platform,
            ..self.clone()
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
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
        }
    }
}
