//! Functions to read and parse variant configuration files.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    path::PathBuf,
    sync::Arc,
};

use indexmap::IndexSet;
use miette::Diagnostic;
use rattler_conda_types::{NoArchType, Platform};
use rattler_variants::{
    NormalizedKey, Pin, VariantConfig as ParsedVariantConfig,
    VariantConfigError as ParsedVariantConfigError, VariantContext as ParsedVariantContext,
    VariantValue,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    hash::HashInfo,
    recipe::{Recipe, custom_yaml::Node, error::ParsingError, variable::Variable},
    selectors::SelectorConfig,
    source_code::SourceCode,
    variant_render::{stage_0_render, stage_1_render},
};

#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct DiscoveredOutput {
    pub name: String,
    pub version: String,
    pub build_string: String,
    pub noarch_type: NoArchType,
    pub target_platform: Platform,
    pub node: Node,
    pub used_vars: BTreeMap<NormalizedKey, Variable>,
    pub recipe: Recipe,
    pub hash: HashInfo,
}

impl Eq for DiscoveredOutput {}

impl PartialEq for DiscoveredOutput {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.build_string == other.build_string
            && self.noarch_type == other.noarch_type
            && self.target_platform == other.target_platform
            && self.node == other.node
            && self.used_vars == other.used_vars
            && self.hash == other.hash
    }
}

impl std::hash::Hash for DiscoveredOutput {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.version.hash(state);
        self.build_string.hash(state);
        self.noarch_type.hash(state);
        self.target_platform.hash(state);
        self.node.hash(state);
        self.used_vars.hash(state);
        self.hash.hash(state);
    }
}

/// The variant configuration.
/// This is usually loaded from a YAML file and contains a mapping of package
/// names to a list of versions. Each version represents a variant of the
/// package. The variant configuration is used to create a build matrix for a
/// recipe.
///
/// Example:
///
/// ```yaml
/// python:
/// - "3.10"
/// - "3.11"
/// ```
///
/// If you depend on Python in your recipe, this will create two variants of
/// your recipe:
///
/// ```txt
/// [python=3.10]
/// and
/// [python=3.11]
/// ```
///
///
/// The variant configuration also contains a list of "zip keys". These are keys
/// that are zipped together to create a list of variants. For example, if the
/// variant configuration contains the following zip keys:
///
/// ```yaml
/// zip_keys:
/// - [python, compiler]
/// ```
///
/// and the following variants:
///
/// ```yaml
/// python:
/// - "3.9"
/// - "3.8"
/// compiler:
/// - gcc
/// - clang
/// ```
///
/// the following variants will be selected:
///
/// ```txt
/// [python=3.9, compiler=gcc]
/// and
/// [python=3.8, compiler=clang]
/// ```
///
/// **Important**: `zip_keys` must be a list of lists. A flat list like
/// `zip_keys: [python, compiler]` will result in an error.
///
/// It's also possible to specify additional pins in the variant configuration.
/// These pins are currently ignored.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VariantConfig {
    /// Pin run dependencies by using the versions from the build dependencies
    /// (and applying the pin). This is currently ignored (TODO)
    pub pin_run_as_build: Option<BTreeMap<String, Pin>>,

    /// The zip keys are used to "zip" together variants to create specific
    /// combinations.
    pub zip_keys: Option<Vec<Vec<NormalizedKey>>>,

    /// The variants are a mapping of package names to a list of versions. Each
    /// version represents a variant for the build matrix.
    #[serde(flatten)]
    pub variants: BTreeMap<NormalizedKey, Vec<Variable>>,
}

#[allow(missing_docs)]
#[derive(Debug, Error, Diagnostic)]
pub enum VariantConfigError<S: SourceCode> {
    #[error(transparent)]
    #[diagnostic(transparent)]
    ParsedConfig(#[from] ParsedVariantConfigError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    RecipeParseErrors(#[from] ParseErrors<S>),

    #[error(transparent)]
    #[diagnostic(transparent)]
    NewParseError(#[from] ParsingError<S>),
}

fn build_variant_context(selector_config: &SelectorConfig) -> ParsedVariantContext {
    let variant = selector_config
        .variant
        .iter()
        .map(|(key, value)| {
            (
                NormalizedKey::from(key.normalize()),
                variable_to_variant_value(value),
            )
        })
        .collect::<BTreeMap<_, _>>();

    ParsedVariantContext::new(
        selector_config.target_platform,
        selector_config.host_platform,
        selector_config.build_platform,
    )
    .with_variant(variant)
}

fn convert_variants(parsed: &ParsedVariantConfig) -> BTreeMap<NormalizedKey, Vec<Variable>> {
    parsed
        .variants()
        .iter()
        .map(|(key, values)| {
            let variables = values.iter().map(variant_value_to_variable).collect();
            (NormalizedKey::from(key.normalize()), variables)
        })
        .collect()
}

fn variant_value_to_variable(value: &VariantValue) -> Variable {
    match value {
        VariantValue::Bool(b) => (*b).into(),
        VariantValue::Integer(i) => (*i).into(),
        VariantValue::Float(f) => Variable::from_string(&f.to_string()),
        VariantValue::String(s) => Variable::from_string(s),
    }
}

fn variable_to_variant_value(variable: &Variable) -> VariantValue {
    use minijinja::value::ValueKind;

    match variable.as_ref().kind() {
        ValueKind::Bool => VariantValue::Bool(variable.as_ref().is_true()),
        ValueKind::Number => {
            let value = variable.to_string();
            if let Ok(i) = value.parse::<i64>() {
                VariantValue::Integer(i)
            } else if let Ok(f) = value.parse::<f64>() {
                VariantValue::Float(f)
            } else {
                VariantValue::String(value)
            }
        }
        _ => {
            if let Some(s) = variable.as_ref().as_str() {
                VariantValue::String(s.to_string())
            } else {
                VariantValue::String(variable.to_string())
            }
        }
    }
}

/// An error that indicates variant configuration is invalid.
#[allow(missing_docs)]
#[derive(Debug, Error, Diagnostic)]
pub enum VariantExpandError {
    #[error("Zip key elements do not all have same length: {0}")]
    InvalidZipKeyLength(String),

    #[error("zip_keys must be a list of lists, not a flat list")]
    InvalidZipKeyStructure,

    #[error("Duplicate outputs: {0}")]
    DuplicateOutputs(String),

    #[error("Missing output: {0} (used in pin_subpackage)")]
    MissingOutput(String),

    #[error("Found a cycle in the recipe outputs: {0}")]
    CycleInRecipeOutputs(String),
}

impl VariantConfig {
    /// Load variant configuration files and convert them into the internal representation.
    pub fn from_files(
        files: &[PathBuf],
        selector_config: &SelectorConfig,
    ) -> Result<Self, VariantConfigError<Arc<str>>> {
        let context = build_variant_context(selector_config);
        let parsed = ParsedVariantConfig::from_files(files, &context)?;
        let variants = convert_variants(&parsed);
        Ok(Self {
            pin_run_as_build: parsed.pin_run_as_build().cloned(),
            zip_keys: parsed.zip_keys().cloned(),
            variants,
        })
    }

    #[cfg(test)]
    fn from_parts_for_tests(
        variants: BTreeMap<NormalizedKey, Vec<Variable>>,
        zip_keys: Option<Vec<Vec<NormalizedKey>>>,
    ) -> Self {
        Self {
            pin_run_as_build: None,
            zip_keys,
            variants,
        }
    }

    /// Obtain mutable access to the internal variant map.
    pub fn variants_mut(&mut self) -> &mut BTreeMap<NormalizedKey, Vec<Variable>> {
        &mut self.variants
    }

    /// Return the pin configuration defined in the variant file, if any.
    pub fn pin_run_as_build(&self) -> Option<&BTreeMap<String, Pin>> {
        self.pin_run_as_build.as_ref()
    }

    /// Return the configured zip key groups, if any.
    pub fn zip_keys(&self) -> Option<&Vec<Vec<NormalizedKey>>> {
        self.zip_keys.as_ref()
    }

    /// Access the normalized variant entries.
    pub fn variants(&self) -> &BTreeMap<NormalizedKey, Vec<Variable>> {
        &self.variants
    }

    /// Look up variant values allowing hyphen/underscore spelling differences.
    fn resolve_variant_entry(&self, key: &NormalizedKey) -> Option<(NormalizedKey, Vec<Variable>)> {
        if let Some(values) = self.variants.get(key) {
            return Some((key.clone(), values.clone()));
        }

        let normalized = key.normalize();
        self.variants
            .iter()
            .find_map(|(existing_key, values)| {
                if existing_key.normalize() == normalized {
                    Some((existing_key.clone(), values.clone()))
                } else {
                    None
                }
            })
    }

    fn validate_zip_keys(&self) -> Result<(), VariantExpandError> {
        if let Some(zip_keys) = &self.zip_keys {
            for zip in zip_keys {
                if zip.len() < 2 {
                    return Err(VariantExpandError::InvalidZipKeyStructure);
                }

                let mut prev_len = None;
                for key in zip {
                    let value = match self.variants.get(key) {
                        None => {
                            return Err(VariantExpandError::InvalidZipKeyLength(key.normalize()));
                        }
                        Some(value) => value,
                    };

                    if let Some(l) = prev_len {
                        if l != value.len() {
                            return Err(VariantExpandError::InvalidZipKeyLength(key.normalize()));
                        }
                    }
                    prev_len = Some(value.len());
                }
            }
        }
        Ok(())
    }

    /// This function returns all possible combinations of variants for the
    /// given set of used variables.
    ///
    /// The `used_vars` argument is a set of variables that are used in the
    /// recipe. The `already_used_vars` argument is a mapping of variables
    /// that are already used in the recipe. This is used to remove variants
    /// that are already in other parts of the "tree".
    pub fn combinations(
        &self,
        used_vars: &HashSet<NormalizedKey>,
        already_used_vars: Option<&BTreeMap<NormalizedKey, Variable>>,
    ) -> Result<Vec<BTreeMap<NormalizedKey, Variable>>, VariantExpandError> {
        self.validate_zip_keys()?;
        let zip_keys = self.zip_keys.clone().unwrap_or_default();
        let used_vars_by_normalized: HashMap<String, &NormalizedKey> = used_vars
            .iter()
            .map(|key| (key.normalize(), key))
            .collect();
        let zip_key_normalized: HashSet<String> = zip_keys
            .iter()
            .flat_map(|zip| zip.iter().map(|key| key.normalize()))
            .collect();

        let used_zip_keys = zip_keys
            .iter()
            .filter(|zip| {
                zip.iter()
                    .any(|key| used_vars_by_normalized.contains_key(&key.normalize()))
            })
            .map(|zip| {
                let mut map = HashMap::new();
                for key in zip {
                    if !used_vars_by_normalized.contains_key(&key.normalize()) {
                        continue;
                    }
                    if let Some(values) = self.variants.get(key) {
                        map.insert(key.clone(), values.clone());
                    }
                }
                VariantKey::ZipKey(map)
            })
            .collect::<Vec<_>>();

        let variant_keys = used_vars
            .iter()
            .filter_map(|key| {
                let (variant_key, values) = self.resolve_variant_entry(key)?;
                if zip_key_normalized.contains(&variant_key.normalize()) {
                    return None;
                }
                Some(VariantKey::Key(variant_key, values))
            })
            .collect::<Vec<_>>();

        let variant_keys = used_zip_keys
            .into_iter()
            .chain(variant_keys)
            .collect::<Vec<_>>();

        // get all combinations of variant keys
        let mut combinations = Vec::new();
        let mut current = Vec::new();
        find_combinations(&variant_keys, 0, &mut current, &mut combinations);

        // zip the combinations
        let result: Vec<_> = combinations
            .iter()
            .map(|combination| {
                combination
                    .iter()
                    .cloned()
                    .collect::<BTreeMap<NormalizedKey, Variable>>()
            })
            .collect();

        if let Some(already_used_vars) = already_used_vars {
            let result = result
                .into_iter()
                .filter(|combination| {
                    if already_used_vars.is_empty() {
                        true
                    } else {
                        already_used_vars.iter().all(|(key, value)| {
                            combination
                                .get(key)
                                .or_else(|| {
                                    combination.iter().find_map(|(existing_key, existing_value)| {
                                        if existing_key.normalize() == key.normalize() {
                                            Some(existing_value)
                                        } else {
                                            None
                                        }
                                    })
                                })
                                == Some(value)
                        })
                    }
                })
                .collect();
            Ok(result)
        } else {
            Ok(result)
        }
    }

    /// This function finds all used variables in a recipe and expands the
    /// recipe to the full build matrix based on the variant configuration
    /// (loaded in the `SelectorConfig`).
    ///
    /// The result is a topologically sorted list of tuples. Each tuple contains
    /// the following elements:
    ///
    /// 1. The name of the package.
    /// 2. The version of the package.
    /// 3. The build string of the package.
    /// 4. The recipe node.
    /// 5. The used variant config.
    pub fn find_variants<S: SourceCode>(
        &self,
        outputs: &[Node],
        recipe: S,
        selector_config: &SelectorConfig,
    ) -> Result<IndexSet<DiscoveredOutput>, VariantError<S>> {
        // find all jinja variables
        let stage_0 = stage_0_render(outputs, recipe, selector_config, self)?;
        let stage_1 = stage_1_render(stage_0, selector_config, self)?;

        // Now we need to convert the stage 1 renders to DiscoveredOutputs
        let mut recipes = IndexSet::new();
        for sx in stage_1 {
            for ((node, mut recipe), variant) in sx.into_sorted_outputs()? {
                let target_platform = if recipe.build().noarch().is_none() {
                    selector_config.target_platform
                } else {
                    Platform::NoArch
                };

                let build_string = recipe
                    .build()
                    .string()
                    .as_resolved()
                    .expect("Build string has to be resolved")
                    .to_string();

                if recipe.build().python().version_independent {
                    recipe
                        .requirements
                        .ignore_run_exports
                        .from_package
                        .insert("python".parse().unwrap());
                    recipe
                        .requirements
                        .ignore_run_exports
                        .by_name
                        .insert("python".parse().unwrap());
                }

                recipes.insert(DiscoveredOutput {
                    name: recipe.package().name.as_normalized().to_string(),
                    version: recipe.package().version.to_string(),
                    build_string,
                    noarch_type: *recipe.build().noarch(),
                    target_platform,
                    node,
                    used_vars: variant.clone(),
                    recipe: recipe.clone(),
                    hash: HashInfo::from_variant(&variant, recipe.build().noarch()),
                });
            }
        }

        Ok(recipes)
    }
}

#[derive(Debug, Clone)]
enum VariantKey {
    Key(NormalizedKey, Vec<Variable>),
    ZipKey(HashMap<NormalizedKey, Vec<Variable>>),
}

impl VariantKey {
    pub fn len(&self) -> usize {
        match self {
            VariantKey::Key(_, values) => values.len(),
            VariantKey::ZipKey(map) => map.values().next().map(|v| v.len()).unwrap_or(0),
        }
    }

    pub fn at(&self, index: usize) -> Option<Vec<(NormalizedKey, Variable)>> {
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

#[derive(Error, Debug, Diagnostic)]
#[error("Failed to parse recipe")]
/// Collection of parse errors to build related diagnostics
/// TODO: also provide `Vec<PartialParsingError>` with source `&str`
/// to avoid excessive traversal
pub struct ParseErrors<S: SourceCode> {
    #[related]
    errs: Vec<ParsingError<S>>,
}

impl<S: SourceCode> From<Vec<ParsingError<S>>> for ParseErrors<S> {
    fn from(errs: Vec<ParsingError<S>>) -> Self {
        Self { errs }
    }
}

#[allow(missing_docs)]
#[derive(Error, Debug, Diagnostic)]
pub enum VariantError<S: SourceCode> {
    #[error(transparent)]
    #[diagnostic(transparent)]
    ExpandError(#[from] VariantExpandError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    ParseErrors(#[from] VariantConfigError<S>),
}

fn find_combinations(
    variant_keys: &[VariantKey],
    index: usize,
    current: &mut Vec<(NormalizedKey, Variable)>,
    result: &mut Vec<Vec<(NormalizedKey, Variable)>>,
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
    use fs_err as fs;
    use rattler_conda_types::Platform;

    use crate::selectors::SelectorConfig;

    #[test]
    fn test_load_config() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("variant_files/variant_config_1.yaml");
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        let variant = VariantConfig::from_files(&[yaml_file], &selector_config).unwrap();
        assert_eq!(
            variant.variants().get(&"noboolean".into()).unwrap(),
            &vec![Variable::from_string("true")]
        );
        insta::assert_yaml_snapshot!(variant);
    }

    #[test]
    fn test_load_config_and_find_variants() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("recipes/variants/variant_config.yaml");
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        // First find all outputs from the recipe
        let recipe_text =
            fs::read_to_string(test_data_dir.join("recipes/variants/recipe.yaml")).unwrap();
        let outputs = crate::recipe::parser::find_outputs_from_src(recipe_text.as_str()).unwrap();
        let variant_config = VariantConfig::from_files(&[yaml_file], &selector_config).unwrap();
        let outputs_and_variants = variant_config
            .find_variants(&outputs, recipe_text.as_str(), &selector_config)
            .unwrap();

        let used_variables_all: Vec<&BTreeMap<NormalizedKey, Variable>> = outputs_and_variants
            .as_slice()
            .into_iter()
            .map(|s| &s.used_vars)
            .collect();

        insta::assert_yaml_snapshot!(used_variables_all);
    }

    use super::*;

    #[test]
    fn test_zip_keys_validation() {
        let mut variants = BTreeMap::<NormalizedKey, Vec<Variable>>::new();
        variants.insert("python".into(), vec!["3.9".into(), "3.10".into()]);
        variants.insert("compiler".into(), vec!["gcc".into(), "clang".into()]);

        let used_vars = vec!["python".into()].into_iter().collect();

        let valid = VariantConfig::from_parts_for_tests(
            variants,
            Some(vec![vec!["python".into(), "compiler".into()]]),
        );
        assert!(valid.combinations(&used_vars, None).is_ok());
    }

    #[test]
    fn test_variant_combinations() {
        let mut base_variants = BTreeMap::<NormalizedKey, Vec<Variable>>::new();
        base_variants.insert("a".into(), vec!["1".into(), "2".into()]);
        base_variants.insert("b".into(), vec!["3".into(), "4".into()]);
        let zip_keys = Some(vec![vec!["a".into(), "b".into()]]);

        let config = VariantConfig::from_parts_for_tests(base_variants.clone(), zip_keys.clone());
        let used_a = vec!["a".into()].into_iter().collect();
        assert_eq!(config.combinations(&used_a, None).unwrap().len(), 2);

        let used_ab = vec!["a".into(), "b".into()].into_iter().collect();
        assert_eq!(config.combinations(&used_ab, None).unwrap().len(), 2);

        let mut extended_variants = base_variants.clone();
        extended_variants.insert("c".into(), vec!["5".into(), "6".into(), "7".into()]);
        let config_zip =
            VariantConfig::from_parts_for_tests(extended_variants.clone(), zip_keys.clone());
        let used_abc = vec!["a".into(), "b".into(), "c".into()]
            .into_iter()
            .collect();
        assert_eq!(
            config_zip.combinations(&used_abc, None).unwrap().len(),
            2 * 3
        );

        let config_no_zip = VariantConfig::from_parts_for_tests(extended_variants.clone(), None);
        assert_eq!(
            config_no_zip.combinations(&used_abc, None).unwrap().len(),
            2 * 2 * 3
        );

        let already_used = BTreeMap::from_iter(vec![("a".into(), "1".into())]);
        let filtered = config_no_zip
            .combinations(&used_abc, Some(&already_used))
            .unwrap();
        assert_eq!(filtered.len(), 2 * 3);
    }

    #[test]
    fn test_order() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        for _ in 1..3 {
            // First find all outputs from the recipe
            let recipe_text =
                fs::read_to_string(test_data_dir.join("recipes/output_order/order_1.yaml"))
                    .unwrap();
            let outputs =
                crate::recipe::parser::find_outputs_from_src(recipe_text.as_str()).unwrap();
            let variant_config = VariantConfig::from_files(&[], &selector_config).unwrap();
            let outputs_and_variants = variant_config
                .find_variants(&outputs, recipe_text.as_str(), &selector_config)
                .unwrap();

            // assert output order
            let order = vec!["some-pkg.foo-a", "some-pkg.foo", "some_pkg.foo"];
            let outputs: Vec<_> = outputs_and_variants
                .iter()
                .map(|o| o.name.clone())
                .collect();
            assert_eq!(outputs, order);
        }
    }

    #[test]
    fn test_python_is_not_used_as_variant_when_noarch() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("recipes/variants/python_variant.yaml");
        let selector_config = SelectorConfig {
            target_platform: Platform::NoArch,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        // First find all outputs from the recipe
        let recipe_text =
            fs::read_to_string(test_data_dir.join("recipes/variants/boltons_recipe.yaml")).unwrap();
        let outputs = crate::recipe::parser::find_outputs_from_src(recipe_text.as_str()).unwrap();
        let variant_config = VariantConfig::from_files(&[yaml_file], &selector_config).unwrap();
        let outputs_and_variants = variant_config
            .find_variants(&outputs, recipe_text.as_str(), &selector_config)
            .unwrap();

        let used_variables_all: Vec<&BTreeMap<NormalizedKey, Variable>> = outputs_and_variants
            .as_slice()
            .into_iter()
            .map(|s| &s.used_vars)
            .collect();

        insta::assert_yaml_snapshot!(used_variables_all);
    }
}
