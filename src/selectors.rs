//! Contains the selector config, which is used to render the recipe.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{
    hash::HashInfo,
    normalized_key::NormalizedKey,
    recipe::{
        jinja::{Env, Git},
        variable::Variable,
    },
};

use minijinja::value::Value;
use rattler_conda_types::Platform;
use strum::IntoEnumIterator as _;

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
    pub variant: BTreeMap<NormalizedKey, Variable>,
    /// Enable experimental features
    pub experimental: bool,
    /// Allow undefined variables
    pub allow_undefined: bool,
    /// The path to the recipe file
    pub recipe_path: Option<PathBuf>,
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
                    Value::from(self.host_platform.only_platform() == Some(only_platform)),
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
            context.insert(key.normalize(), v.clone().into());
        }

        context
    }

    /// Create a new selector config from an existing one, replacing the variant
    pub fn with_variant(
        &self,
        variant: BTreeMap<NormalizedKey, Variable>,
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
            recipe_path: None,
        }
    }
}
