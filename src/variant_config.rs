use std::collections::{HashMap, HashSet};
use std::{collections::BTreeMap, path::PathBuf};

use serde::Deserialize;
use serde::Serialize;
use serde_with::formats::PreferOne;
use serde_with::serde_as;
use serde_with::OneOrMany;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

use crate::selectors::{flatten_selectors, SelectorConfig};
use crate::used_variables::extract_dependencies;
use crate::used_variables::used_vars_from_expressions;

/// This function normalizes a Map of String and List values to a List of Strings
/// For example,
///
/// ```yaml
/// compiler:
///  - gcc
///  - clang
/// python: "3.9"
/// ```
///
/// becomes
///
/// ```yaml
/// compiler:
/// - gcc
/// - clang
/// python:
/// - "3.9"
/// ```
fn value_to_map(value: serde_yaml::Value) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for (key, value) in value.as_mapping().unwrap() {
        let key = key.as_str().unwrap().to_string();
        match value {
            serde_yaml::Value::String(value) => {
                map.insert(key, vec![value.to_string()]);
                continue;
            }
            serde_yaml::Value::Sequence(value) => {
                let value = value
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                map.insert(key, value);
                continue;
            }
            _ => {
                panic!("Invalid value type");
            }
        }
    }
    map
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Pin {
    pub max_pin: Option<String>,
    pub min_pin: Option<String>,
}

#[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VariantConfig {
    pub pin_run_as_build: Option<BTreeMap<String, Pin>>,
    pub zip_keys: Option<Vec<Vec<String>>>,

    #[serde_as(deserialize_as = "BTreeMap<_, OneOrMany<_, PreferOne>>")]
    #[serde(flatten)]
    pub variants: BTreeMap<String, Vec<String>>,
}

impl VariantConfig {
    /// This function loads multiple variant configuration files and merges them into a single
    /// configuration. The configuration files are loaded in the order they are provided in the
    /// `files` argument. The `selector_config` argument is used to select the correct configuration
    /// for the target platform.
    ///
    /// A variant configuration file is a YAML file that contains a mapping of package names to
    /// a list of variants. For example:
    ///
    /// ```yaml
    /// python:
    /// - "3.9"
    /// - "3.8"
    /// ```
    ///
    /// The above configuration file will select the `python` package with the variants `3.9` and
    /// `3.8`.
    ///
    /// The `selector_config` argument is used to select the correct configuration for the target
    /// platform. For example, if the `selector_config` is `unix`, the following configuration file:
    ///
    /// ```yaml
    /// sel(unix):
    ///   python:
    ///   - "3.9"
    ///   - "3.8"
    /// sel(win):
    ///   python:
    ///   - "3.9"
    /// ```
    ///
    /// will be flattened to:
    ///
    /// ```yaml
    /// python:
    /// - "3.9"
    /// - "3.8"
    /// ```
    ///
    /// The `files` argument is a list of paths to the variant configuration files. The files are
    /// loaded in the order they are provided in the `files` argument. The keys of a later file
    /// replace keys from an earlier file (values are _not_ merged).
    ///
    /// A special key, the `zip_keys` is used to "zip" the values of two keys. For example, if the
    /// following configuration file is loaded:
    ///
    /// ```yaml
    /// compiler:
    /// - gcc
    /// - clang
    /// python:
    /// - "3.9"
    /// - "3.8"
    /// zip_keys:
    /// - [compiler, python]
    /// ```
    ///
    /// the variant configuration will be zipped so that the following variants are selected:
    ///
    /// ```txt
    /// [python=3.9, compiler=gcc]
    /// and
    /// [python=3.8, compiler=clang]
    /// ```
    pub fn from_files(files: &Vec<PathBuf>, selector_config: &SelectorConfig) -> Self {
        let mut variant_configs = Vec::new();

        for file in files {
            let file = std::fs::File::open(file).unwrap();
            let reader = std::io::BufReader::new(file);
            let mut yaml_value = serde_yaml::from_reader(reader).unwrap();

            if let Some(yaml_value) = flatten_selectors(&mut yaml_value, selector_config) {
                let config: VariantConfig = serde_yaml::from_value(yaml_value)
                    .expect("Could not deserialize variant config");
                variant_configs.push(config);
            }
        }

        let mut final_config = VariantConfig::default();
        for config in variant_configs {
            final_config.variants.extend(config.variants);
            if let Some(pin_run_as_build) = config.pin_run_as_build {
                if let Some(final_pin_run_as_build) = &mut final_config.pin_run_as_build {
                    final_pin_run_as_build.extend(pin_run_as_build);
                } else {
                    final_config.pin_run_as_build = Some(pin_run_as_build);
                }
            }
            final_config.zip_keys = config.zip_keys;
        }

        final_config
    }

    fn validate_zip_keys(&self) -> Result<(), VariantError> {
        if let Some(zip_keys) = &self.zip_keys {
            for zip in zip_keys {
                let mut len = None;
                for key in zip {
                    if let Some(l) = len {
                        if l != key.len() {
                            return Err(VariantError::InvalidZipKeyLength(key.to_string()));
                        }
                    }
                    len = Some(key.len());
                }
            }
        }
        Ok(())
    }

    pub fn combinations(
        &self,
        used_vars: &HashSet<String>,
    ) -> Result<Vec<BTreeMap<String, String>>, VariantError> {
        self.validate_zip_keys()?;
        let zip_keys = self.zip_keys.clone().unwrap_or_default();
        let used_zip_keys = zip_keys
            .iter()
            .filter(|zip| zip.iter().any(|key| used_vars.contains(key)))
            .map(|zip| {
                let mut map = HashMap::new();
                for key in zip {
                    if !used_vars.contains(key) {
                        continue;
                    }
                    if let Some(values) = self.variants.get(key) {
                        map.insert(key.clone(), values.clone());
                    }
                }
                VariantKey::ZipKey(map)
            })
            .collect::<Vec<_>>();

        let variant_keys = self
            .variants
            .iter()
            .filter(|(key, _)| used_vars.contains(*key))
            .filter(|(key, _)| !zip_keys.iter().any(|zip| zip.contains(*key)))
            .map(|(key, values)| VariantKey::Key(key.clone(), values.clone()))
            .collect::<Vec<_>>();

        let variant_keys = used_zip_keys
            .into_iter()
            .chain(variant_keys.into_iter())
            .collect::<Vec<_>>();

        // get all combinations of variant keys
        let mut combinations = Vec::new();
        let mut current = Vec::new();
        find_combinations(&variant_keys, 0, &mut current, &mut combinations);

        // zip the combinations
        let result = combinations
            .iter()
            .map(|combination| {
                combination
                    .iter()
                    .cloned()
                    .collect::<BTreeMap<String, String>>()
            })
            .collect();
        Ok(result)
    }

    /// This finds all used variables in any dependency declarations, build, host, and run sections.
    /// As well as any used variables from Jinja functions to calculate the variants of this recipe.
    pub fn find_variants(
        &self,
        recipe: &str,
        selector_config: &SelectorConfig,
    ) -> Result<Vec<BTreeMap<String, String>>, VariantError> {
        let mut used_variables = used_vars_from_expressions(recipe);

        // now render all selectors with the used variables
        let combinations = self.combinations(&used_variables)?;

        let recipe_parsed: YamlValue = serde_yaml::from_str(recipe).unwrap();
        for _ in combinations {
            let mut val = recipe_parsed.clone();
            if let Some(flattened_recipe) = flatten_selectors(&mut val, selector_config) {
                // extract all dependencies from the flattened recipe
                let dependencies = extract_dependencies(&flattened_recipe);
                for dependency in dependencies {
                    used_variables.insert(dependency);
                }
            };
        }
        self.combinations(&used_variables)
    }
}

#[derive(Debug, Clone)]
enum VariantKey {
    Key(String, Vec<String>),
    ZipKey(HashMap<String, Vec<String>>),
}

impl VariantKey {
    pub fn len(&self) -> usize {
        match self {
            VariantKey::Key(_, values) => values.len(),
            VariantKey::ZipKey(map) => map.values().next().map(|v| v.len()).unwrap_or(0),
        }
    }

    pub fn at(&self, index: usize) -> Option<Vec<(String, String)>> {
        match self {
            VariantKey::Key(key, values) => {
                values.get(index).map(|v| vec![(key.clone(), v.clone())])
            }
            VariantKey::ZipKey(map) => {
                let mut result = Vec::new();
                for (key, values) in map {
                    if let Some(value) = values.get(index) {
                        result.push((key.clone(), value.clone()));
                    }
                }
                if result.len() == map.len() {
                    Some(result)
                } else {
                    // this should never happen
                    None
                }
            }
        }
    }
}

#[derive(Error, Debug)]
pub enum VariantError {
    #[error("Zip key elements do not all have same length: {0}")]
    InvalidZipKeyLength(String),
}

fn find_combinations(
    variant_keys: &[VariantKey],
    index: usize,
    current: &mut Vec<(String, String)>,
    result: &mut Vec<Vec<(String, String)>>,
) {
    if index == variant_keys.len() {
        result.push(current.clone());
        return;
    }

    for i in 0..variant_keys[index].len() {
        if let Some(items) = variant_keys[index].at(i) {
            current.extend(items.clone());
            find_combinations(variant_keys, index + 1, current, result);
            for _ in 0..items.len() {
                current.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::selectors::{flatten_toplevel, SelectorConfig};
    use rattler_conda_types::Platform;
    use rstest::rstest;
    use serde_yaml::Value as YamlValue;

    #[rstest]
    #[case("selectors/config_1.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();

        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant: vec![("python_version".into(), "3.8.5".into())]
                .into_iter()
                .collect(),
        };

        let res = flatten_toplevel(&mut yaml, &selector_config);
        insta::assert_yaml_snapshot!(res);

        let selector_config = SelectorConfig {
            target_platform: Platform::Win64,
            build_platform: Platform::Win64,
            variant: vec![("python_version".into(), "3.8.5".into())]
                .into_iter()
                .collect(),
        };

        let res = flatten_toplevel(&mut yaml, &selector_config);
        insta::assert_yaml_snapshot!(res);
    }

    #[test]
    fn test_load_config() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("variant_files/variant_config_1.yaml");
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant: Default::default(),
        };

        let variant = VariantConfig::from_files(&vec![yaml_file], &selector_config);

        insta::assert_yaml_snapshot!(variant);
    }

    use super::*;

    #[test]
    fn test_variant_combinations() {
        let mut variants = BTreeMap::new();
        variants.insert("a".to_string(), vec!["1".to_string(), "2".to_string()]);
        variants.insert("b".to_string(), vec!["3".to_string(), "4".to_string()]);
        let zip_keys = vec![vec!["a".to_string(), "b".to_string()].into_iter().collect()];

        let used_vars = vec!["a".to_string()].into_iter().collect();
        let mut config = VariantConfig {
            variants,
            zip_keys: Some(zip_keys),
            pin_run_as_build: None,
        };

        let combinations = config.combinations(&used_vars).unwrap();
        assert_eq!(combinations.len(), 2);

        let used_vars = vec!["a".to_string(), "b".to_string()].into_iter().collect();
        let combinations = config.combinations(&used_vars).unwrap();
        assert_eq!(combinations.len(), 2);

        config.variants.insert(
            "c".to_string(),
            vec!["5".to_string(), "6".to_string(), "7".to_string()],
        );
        let used_vars = vec!["a".to_string(), "b".to_string(), "c".to_string()]
            .into_iter()
            .collect();
        let combinations = config.combinations(&used_vars).unwrap();
        assert_eq!(combinations.len(), 2 * 3);

        let used_vars = vec!["a".to_string(), "b".to_string(), "c".to_string()]
            .into_iter()
            .collect();
        config.zip_keys = None;
        let combinations = config.combinations(&used_vars).unwrap();
        assert_eq!(combinations.len(), 2 * 2 * 3);
    }
}
