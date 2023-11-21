use std::collections::BTreeMap;

use crate::recipe::jinja::Env;

use minijinja::value::Value;
use rattler_conda_types::Platform;

#[derive(Clone, Debug)]
pub struct SelectorConfig {
    pub target_platform: Platform,
    pub build_platform: Platform,
    pub hash: Option<String>,
    pub variant: BTreeMap<String, String>,
}

impl SelectorConfig {
    pub fn into_context(self) -> BTreeMap<String, Value> {
        let mut context = BTreeMap::new();

        context.insert(
            "target_platform".to_string(),
            Value::from_safe_string(self.target_platform.to_string()),
        );

        if let Some(platform) = self.target_platform.only_platform() {
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
            Value::from(self.target_platform.is_unix()),
        );

        context.insert(
            "build_platform".to_string(),
            Value::from_safe_string(self.build_platform.to_string()),
        );

        if let Some(hash) = self.hash {
            context.insert("hash".to_string(), Value::from_safe_string(hash));
        }

        context.insert("env".to_string(), Value::from_object(Env));

        for (key, v) in self.variant {
            context.insert(key, Value::from_safe_string(v));
        }

        context
    }

    pub fn new_with_variant(&self, variant: BTreeMap<String, String>) -> Self {
        Self {
            variant,
            ..self.clone()
        }
    }
}

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            target_platform: Platform::current(),
            build_platform: Platform::current(),
            hash: None,
            variant: Default::default(),
        }
    }
}
