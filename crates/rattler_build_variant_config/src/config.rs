//! Main variant configuration structure

use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::combination::compute_combinations;
#[cfg(not(target_arch = "wasm32"))]
use crate::error::VariantConfigError;
use crate::error::VariantExpandError;

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
    ///
    /// This parser supports conditionals (`if/then/else`) and Jinja templates (`${{ }}`).
    /// Conditionals are evaluated using a default JinjaConfig based on the current platform.
    ///
    /// For more control over the evaluation context, use `from_file_with_context` instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(path: &Path) -> Result<Self, VariantConfigError> {
        // Use a default JinjaConfig for evaluation
        let jinja_config = rattler_build_jinja::JinjaConfig::default();
        Self::from_file_with_context(path, &jinja_config)
    }

    /// Load variant configuration from a YAML file with a JinjaConfig context
    ///
    /// This allows evaluation of conditionals and templates in the variant file.
    /// The `jinja_config` provides platform information and other context needed for evaluation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rattler_build_variant_config::VariantConfig;
    /// use rattler_build_jinja::JinjaConfig;
    /// use rattler_conda_types::Platform;
    /// use std::path::Path;
    ///
    /// let mut jinja_config = JinjaConfig::default();
    /// jinja_config.target_platform = Platform::Linux64;
    ///
    /// let config = VariantConfig::from_file_with_context(
    ///     Path::new("variants.yaml"),
    ///     &jinja_config
    /// ).unwrap();
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file_with_context(
        path: &Path,
        jinja_config: &rattler_build_jinja::JinjaConfig,
    ) -> Result<Self, VariantConfigError> {
        // Parse the file
        let stage0 = crate::yaml_parser::parse_variant_file(path)?;

        // Evaluate with the provided context
        crate::evaluate::evaluate_variant_config(&stage0, jinja_config).map_err(|e| {
            VariantConfigError::ParseError {
                path: path.to_path_buf(),
                source: rattler_build_yaml_parser::ParseError::generic(
                    e.to_string(),
                    marked_yaml::Span::new_blank(),
                ),
            }
        })
    }

    /// Parse variant configuration from a YAML string
    ///
    /// This parser supports conditionals (`if/then/else`) and Jinja templates (`${{ }}`).
    /// Conditionals are evaluated using a default JinjaConfig based on the current platform.
    ///
    /// For more control over the evaluation context, use `from_yaml_str_with_context` instead.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, String> {
        // Use a default JinjaConfig for evaluation
        let jinja_config = rattler_build_jinja::JinjaConfig::default();
        Self::from_yaml_str_with_context(yaml, &jinja_config)
    }

    /// Parse variant configuration from a YAML string with a JinjaConfig context
    ///
    /// This allows evaluation of conditionals and templates in the variant YAML.
    /// The `jinja_config` provides platform information and other context needed for evaluation.
    pub fn from_yaml_str_with_context(
        yaml: &str,
        jinja_config: &rattler_build_jinja::JinjaConfig,
    ) -> Result<Self, String> {
        // Parse using the new marked_yaml parser
        let stage0 = crate::yaml_parser::parse_variant_str(yaml, None)
            .map_err(|e| format!("Failed to parse variant config: {}", e))?;

        // Evaluate with the provided context
        crate::evaluate::evaluate_variant_config(&stage0, jinja_config)
            .map_err(|e| format!("Failed to evaluate variant config: {}", e))
    }

    /// The name of the conda_build_config.yaml file (legacy format with `# [selector]` syntax)
    #[cfg(not(target_arch = "wasm32"))]
    const CONDA_BUILD_CONFIG_FILENAME: &'static str = "conda_build_config.yaml";

    /// Load multiple variant configuration files and merge them
    ///
    /// Files are merged in order, with later files taking precedence.
    /// The `zip_keys` from the last file that specifies them will be used.
    ///
    /// Files named `conda_build_config.yaml` are loaded using the legacy loader
    /// which supports `# [selector]` syntax. Other files use the modern loader
    /// with `if/then/else` conditionals.
    ///
    /// The `target_platform` is used for evaluating platform-specific selectors.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_files(
        paths: &[impl AsRef<Path>],
        target_platform: rattler_conda_types::Platform,
    ) -> Result<Self, VariantConfigError> {
        let jinja_config = rattler_build_jinja::JinjaConfig {
            target_platform,
            host_platform: target_platform,
            ..Default::default()
        };
        Self::from_files_with_context(paths, &jinja_config)
    }

    /// Load multiple variant configuration files with a JinjaConfig context and merge them
    ///
    /// Files are merged in order, with later files taking precedence.
    /// The `zip_keys` from the last file that specifies them will be used.
    ///
    /// Files named `conda_build_config.yaml` are loaded using the legacy loader
    /// which supports `# [selector]` syntax. Other files use the modern loader.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_files_with_context(
        paths: &[impl AsRef<Path>],
        jinja_config: &rattler_build_jinja::JinjaConfig,
    ) -> Result<Self, VariantConfigError> {
        let mut final_config = VariantConfig::new();

        for path in paths {
            let path = path.as_ref();
            tracing::info!("Loading variant config from: {}", path.display());

            // Use the correct loader based on filename
            let config = if path
                .file_name()
                .map(|f| f == Self::CONDA_BUILD_CONFIG_FILENAME)
                .unwrap_or(false)
            {
                // Use legacy loader for conda_build_config.yaml files
                crate::conda_build_config::load_conda_build_config(path, jinja_config)?
            } else {
                Self::from_file_with_context(path, jinja_config)?
            };
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
    ///
    /// # Returns
    ///
    /// A vector of variant combinations, where each combination is a map from key to value
    pub fn combinations(
        &self,
        used_vars: &HashSet<NormalizedKey>,
    ) -> Result<Vec<BTreeMap<NormalizedKey, Variable>>, VariantExpandError> {
        let zip_keys = self.zip_keys.as_deref().unwrap_or(&[]);
        compute_combinations(&self.variants, zip_keys, used_vars)
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

        let combos = config.combinations(&used_vars).unwrap();
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
quoted_string:
  - "hello"
  - "3.9.10"  # Version string
unquoted_bool:
  - true
unquoted_int:
  - 5
float_val:
  - 1.23
"#;
        let config = VariantConfig::from_yaml_str(yaml).unwrap();

        // Quoted string values - pure strings should remain strings
        let quoted_values = config.get(&"quoted_string".into()).unwrap();
        let quoted_hello = &quoted_values[0];
        assert_eq!(quoted_hello.to_string(), "hello");
        assert!(quoted_hello.as_ref().as_str().is_some());

        // Quoted version string - should be a string
        let quoted_version = &quoted_values[1];
        assert_eq!(quoted_version.to_string(), "3.9.10");
        assert!(quoted_version.as_ref().as_str().is_some());

        // Unquoted values should be their respective types
        let unquoted_bool = &config.get(&"unquoted_bool".into()).unwrap()[0];
        assert!(unquoted_bool.as_ref().is_true());

        let unquoted_int = &config.get(&"unquoted_int".into()).unwrap()[0];
        assert!(unquoted_int.as_ref().is_number());

        // Float should be a string (to preserve version numbers like "1.23")
        let float_val = &config.get(&"float_val".into()).unwrap()[0];
        assert_eq!(float_val.to_string(), "1.23");
        assert!(float_val.as_ref().as_str().is_some());
    }
}
