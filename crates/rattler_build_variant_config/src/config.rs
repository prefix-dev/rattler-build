//! Main variant configuration structure

use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::{
    combination::compute_combinations,
    error::{VariantConfigError, VariantExpandError},
};

/// The variant configuration structure.
///
/// This represents a build matrix configuration, typically loaded from a
/// `variants.yaml` or `conda_build_config.yaml` file.
///
/// # Example
///
/// ```yaml
/// python:
///   - "3.9"
///   - "3.10"
///   - "3.11"
/// numpy:
///   - "1.20"
///   - "1.21"
/// zip_keys:
///   - [python, numpy]
/// ```
///
/// This creates a build matrix where python and numpy versions are zipped together.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VariantConfig {
    /// Keys that should be "zipped" together when creating the build matrix.
    /// Each inner vector represents a group of keys that should be synchronized.
    ///
    /// Example: `[[python, numpy]]` means python=3.9 goes with numpy=1.20,
    /// python=3.10 goes with numpy=1.21, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip_keys: Option<Vec<Vec<NormalizedKey>>>,

    /// The variant values - a mapping of keys to lists of possible values.
    /// Each key represents a variable in the build matrix.
    #[serde(flatten)]
    pub variants: BTreeMap<NormalizedKey, Vec<Variable>>,
}

impl VariantConfig {
    /// Create a new empty variant configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Load variant configuration from a YAML file
    pub fn from_file(path: &Path) -> Result<Self, VariantConfigError> {
        let content = fs_err::read_to_string(path)
            .map_err(|e| VariantConfigError::IoError(path.to_path_buf(), e))?;

        Self::from_yaml_str(&content)
            .map_err(|e| VariantConfigError::ParseError(path.to_path_buf(), e))
    }

    /// Parse variant configuration from a YAML string
    pub fn from_yaml_str(yaml: &str) -> Result<Self, String> {
        // Parse YAML twice:
        // 1. With serde_yaml to get proper type information (quoted vs unquoted)
        // 2. With marked_yaml to have span information available for future error messages
        let serde_value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;
        let marked_node = marked_yaml::parse_yaml(0, yaml).map_err(|e| e.to_string())?;

        // Convert using serde types, but keep marked_node for future span-based errors
        Self::from_yaml_with_types(&serde_value, &marked_node)
    }

    /// Convert YAML to VariantConfig using type info from serde_yaml and spans from marked_yaml
    fn from_yaml_with_types(
        serde_value: &serde_yaml::Value,
        _marked_node: &marked_yaml::Node,
    ) -> Result<Self, String> {
        let mapping = serde_value
            .as_mapping()
            .ok_or_else(|| "Variant config must be a YAML mapping".to_string())?;

        let mut zip_keys = None;
        let mut variants = BTreeMap::new();

        for (key, val) in mapping {
            let key_str = key
                .as_str()
                .ok_or_else(|| "Variant config keys must be strings".to_string())?;

            if key_str == "zip_keys" {
                // Parse zip_keys using serde
                zip_keys = Some(serde_yaml::from_value(val.clone()).map_err(|e| e.to_string())?);
            } else {
                // Parse variant values using serde types
                let values = Self::parse_serde_variant_values(val)?;
                variants.insert(key_str.into(), values);
            }
        }

        Ok(VariantConfig { zip_keys, variants })
    }

    /// Parse variant values from serde_yaml::Value
    fn parse_serde_variant_values(value: &serde_yaml::Value) -> Result<Vec<Variable>, String> {
        let sequence = value
            .as_sequence()
            .ok_or_else(|| "Variant values must be a list".to_string())?;

        sequence.iter().map(Self::serde_value_to_variable).collect()
    }

    /// Convert serde_yaml::Value to Variable, treating floats as strings
    fn serde_value_to_variable(value: &serde_yaml::Value) -> Result<Variable, String> {
        match value {
            serde_yaml::Value::String(s) => {
                // String in YAML (was quoted) - keep as string
                Ok(Variable::from_string(s))
            }
            serde_yaml::Value::Bool(b) => {
                // Boolean (unquoted true/false)
                Ok(Variable::from(*b))
            }
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    // Integer - keep as integer
                    Ok(Variable::from(i))
                } else {
                    // Float - convert to string to preserve version numbers like "1.23"
                    Ok(Variable::from_string(&n.to_string()))
                }
            }
            _ => Err(format!("Unsupported variant value type: {:?}", value)),
        }
    }

    /// Load multiple variant configuration files and merge them
    ///
    /// Files are merged in order, with later files taking precedence.
    /// The `zip_keys` from the last file that specifies them will be used.
    pub fn from_files(paths: &[impl AsRef<Path>]) -> Result<Self, VariantConfigError> {
        let mut final_config = VariantConfig::new();

        for path in paths {
            let path = path.as_ref();
            tracing::info!("Loading variant config from: {}", path.display());
            let config = Self::from_file(path)?;
            final_config.merge(config);
        }

        Ok(final_config)
    }

    /// Merge another variant configuration into this one
    ///
    /// Variant values are replaced (not merged), and zip_keys from `other` take precedence.
    pub fn merge(&mut self, other: VariantConfig) {
        // Extend variants (later values replace earlier ones)
        self.variants.extend(other.variants);

        // Replace zip_keys if provided
        if other.zip_keys.is_some() {
            self.zip_keys = other.zip_keys;
        }
    }

    /// Insert or update a variant key
    pub fn insert(&mut self, key: impl Into<NormalizedKey>, values: Vec<Variable>) {
        self.variants.insert(key.into(), values);
    }

    /// Get the values for a variant key
    pub fn get(&self, key: &NormalizedKey) -> Option<&Vec<Variable>> {
        self.variants.get(key)
    }

    /// Compute all possible combinations for the given set of used variables
    ///
    /// # Arguments
    ///
    /// * `used_vars` - Set of variable keys that are actually used in the recipe
    /// * `already_used_vars` - Optional filter to only return combinations matching these values
    ///
    /// # Returns
    ///
    /// A vector of variant combinations, where each combination is a map from key to value
    pub fn combinations(
        &self,
        used_vars: &HashSet<NormalizedKey>,
        already_used_vars: Option<&BTreeMap<NormalizedKey, Variable>>,
    ) -> Result<Vec<BTreeMap<NormalizedKey, Variable>>, VariantExpandError> {
        let zip_keys = self.zip_keys.as_deref().unwrap_or(&[]);
        compute_combinations(&self.variants, zip_keys, used_vars, already_used_vars)
    }

    /// Get all variant keys
    pub fn keys(&self) -> impl Iterator<Item = &NormalizedKey> {
        self.variants.keys()
    }

    /// Check if configuration is empty
    pub fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    /// Get the number of variant keys
    pub fn len(&self) -> usize {
        self.variants.len()
    }

    /// Serialize to YAML string
    pub fn to_yaml_string(&self) -> Result<String, String> {
        serde_yaml::to_string(self).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
"#;
        let config = VariantConfig::from_yaml_str(yaml).unwrap();
        assert_eq!(config.variants.len(), 2);
        assert_eq!(config.get(&"python".into()).unwrap().len(), 2);
    }

    #[test]
    fn test_parse_with_zip_keys() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
zip_keys:
  - [python, numpy]
"#;
        let config = VariantConfig::from_yaml_str(yaml).unwrap();
        assert!(config.zip_keys.is_some());
        assert_eq!(config.zip_keys.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_merge_configs() {
        let mut config1 = VariantConfig::new();
        config1.insert("python", vec!["3.9".into(), "3.10".into()]);

        let mut config2 = VariantConfig::new();
        config2.insert("numpy", vec!["1.20".into(), "1.21".into()]);
        config2.insert("python", vec!["3.11".into()]); // Should override

        config1.merge(config2);

        assert_eq!(config1.variants.len(), 2);
        assert_eq!(config1.get(&"python".into()).unwrap().len(), 1); // Overridden
        assert_eq!(
            config1.get(&"python".into()).unwrap()[0].to_string(),
            "3.11"
        );
    }

    #[test]
    fn test_combinations() {
        let mut config = VariantConfig::new();
        config.insert("python", vec!["3.9".into(), "3.10".into()]);
        config.insert("numpy", vec!["1.20".into(), "1.21".into()]);

        let mut used_vars = HashSet::new();
        used_vars.insert("python".into());
        used_vars.insert("numpy".into());

        let combos = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combos.len(), 4); // 2x2 combinations
    }

    #[test]
    fn test_serialization() {
        let mut config = VariantConfig::new();
        config.insert("python", vec!["3.9".into(), "3.10".into()]);
        config.zip_keys = Some(vec![vec!["python".into()]]);

        let yaml = config.to_yaml_string().unwrap();
        let parsed = VariantConfig::from_yaml_str(&yaml).unwrap();

        assert_eq!(parsed.variants.len(), config.variants.len());
        assert_eq!(parsed.zip_keys, config.zip_keys);
    }

    #[test]
    fn test_quoted_vs_unquoted() {
        let yaml = r#"
quoted_bool:
  - "true"
quoted_int:
  - "5"
unquoted_bool:
  - true
unquoted_int:
  - 5
float_val:
  - 1.23
"#;
        let config = VariantConfig::from_yaml_str(yaml).unwrap();

        // Quoted values should be strings
        let quoted_bool = &config.get(&"quoted_bool".into()).unwrap()[0];
        assert_eq!(quoted_bool.to_string(), "true");
        // Check if it's actually a string, not a boolean
        assert!(quoted_bool.as_ref().as_str().is_some());

        let quoted_int = &config.get(&"quoted_int".into()).unwrap()[0];
        assert_eq!(quoted_int.to_string(), "5");
        assert!(quoted_int.as_ref().as_str().is_some());

        // Unquoted values should be their respective types
        let unquoted_bool = &config.get(&"unquoted_bool".into()).unwrap()[0];
        assert!(unquoted_bool.as_ref().is_true());

        let unquoted_int = &config.get(&"unquoted_int".into()).unwrap()[0];
        assert!(unquoted_int.as_ref().is_number());

        // Float should be a string
        let float_val = &config.get(&"float_val".into()).unwrap()[0];
        assert_eq!(float_val.to_string(), "1.23");
        assert!(float_val.as_ref().as_str().is_some());
    }
}
