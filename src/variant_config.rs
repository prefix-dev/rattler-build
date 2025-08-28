//! Functions to read and parse variant configuration files.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexSet;
use miette::Diagnostic;
use rattler_conda_types::{NoArchType, Platform};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    _partialerror,
    conda_build_config::{ParseConfigBuildConfigError, load_conda_build_config},
    consts::CONDA_BUILD_CONFIG_FILE,
    hash::HashInfo,
    normalized_key::NormalizedKey,
    recipe::{
        Jinja, Recipe, Render,
        custom_yaml::{HasSpan, Node, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        variable::Variable,
    },
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
/// Represents a pin configuration for a package.
pub struct Pin {
    /// The maximum pin (a string like "x.x.x").
    pub max_pin: Option<String>,
    /// The minimum pin (a string like "x.x.x").
    pub min_pin: Option<String>,
}

impl TryConvertNode<Pin> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Pin, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping,))
            .map_err(|e| vec![e])
            .and_then(|map| map.try_convert(name))
    }
}

impl TryConvertNode<Pin> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Pin, Vec<PartialParsingError>> {
        let mut pin = Pin::default();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "max_pin" => {
                    pin.max_pin = value.try_convert(key_str)?;
                }
                "min_pin" => {
                    pin.min_pin = value.try_convert(key_str)?;
                }
                _ => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(key_str.to_string().into()),
                        help = format!("Valid fields for {name} are: max_pin, min_pin")
                    )]);
                }
            }
        }

        Ok(pin)
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

/// An error that can occur while parsing a variant configuration file.
#[allow(missing_docs)]
#[derive(Debug, Error, Diagnostic)]
pub enum VariantConfigError<S: SourceCode> {
    #[error(transparent)]
    #[diagnostic(transparent)]
    RecipeParseErrors(#[from] ParseErrors<S>),

    #[error("Could not parse variant config file ({0}): {1}")]
    ParseError(PathBuf, serde_yaml::Error),

    #[error("Could not open file ({0}): {1}")]
    IOError(PathBuf, std::io::Error),

    #[error(transparent)]
    #[diagnostic(transparent)]
    NewParseError(#[from] ParsingError<S>),
}

/// An error that indicates variant configuration is invalid.
#[allow(missing_docs)]
#[derive(Debug, Error, Diagnostic)]
pub enum VariantExpandError {
    #[error("Zip key elements do not all have same length: {0}")]
    InvalidZipKeyLength(String),

    #[error("Duplicate outputs: {0}")]
    DuplicateOutputs(String),

    #[error("Missing output: {0} (used in pin_subpackage)")]
    MissingOutput(String),

    #[error("Found a cycle in the recipe outputs: {0}")]
    CycleInRecipeOutputs(String),
}

impl<S: SourceCode> From<ParseConfigBuildConfigError> for VariantConfigError<S> {
    fn from(e: ParseConfigBuildConfigError) -> Self {
        match e {
            ParseConfigBuildConfigError::ParseError(path, err) => {
                VariantConfigError::ParseError(path, err)
            }
            ParseConfigBuildConfigError::IOError(path, e) => VariantConfigError::IOError(path, e),
        }
    }
}

impl VariantConfig {
    /// This function loads a single variant configuration file and returns the
    /// configuration.
    fn load_file(
        path: &Path,
        selector_config: &SelectorConfig,
    ) -> Result<VariantConfig, VariantConfigError<Arc<str>>> {
        if path.file_name() == Some(CONDA_BUILD_CONFIG_FILE.as_ref()) {
            Ok(Self::load_conda_build_config(path, selector_config)?)
        } else {
            Self::load_variant_config(path, selector_config)
        }
    }

    fn load_variant_config(
        path: &Path,
        selector_config: &SelectorConfig,
    ) -> Result<VariantConfig, VariantConfigError<Arc<str>>> {
        let file = fs_err::read_to_string(path)
            .map_err(|e| VariantConfigError::IOError(path.to_path_buf(), e))?;
        let source = Arc::<str>::from(file.as_str());
        let yaml_node = Node::parse_yaml(0, source.clone())?;
        let jinja = Jinja::new(selector_config.clone());
        let rendered_node: RenderedNode = yaml_node
            .render(&jinja, path.to_string_lossy().as_ref())
            .map_err(|e| ParseErrors::from_partial_vec(source.clone(), e))?;

        let config: VariantConfig = rendered_node
            .try_convert(path.to_string_lossy().as_ref())
            .map_err(|e| {
                let parse_errors: ParseErrors<_> = ParsingError::from_partial_vec(source, e).into();
                parse_errors
            })?;
        Ok(config)
    }

    /// This function loads an old-style variant configuration file and returns
    /// the configuration.
    fn load_conda_build_config(
        path: &Path,
        selector_config: &SelectorConfig,
    ) -> Result<VariantConfig, ParseConfigBuildConfigError> {
        load_conda_build_config(path, selector_config)
    }

    /// This function loads multiple variant configuration files and merges them
    /// into a single configuration. The configuration files are loaded in
    /// the order they are provided in the `files` argument. The
    /// `selector_config` argument is used to select the correct configuration
    /// for the target platform.
    ///
    /// A variant configuration file is a YAML file that contains a mapping of
    /// package names to a list of variants. For example:
    ///
    /// ```yaml
    /// python:
    /// - "3.9"
    /// - "3.8"
    /// ```
    ///
    /// The above configuration file will select the `python` package with the
    /// variants `3.9` and `3.8`.
    ///
    /// The `selector_config` argument is used to select the correct
    /// configuration for the target platform. For example, if the
    /// `selector_config` is `unix`, the following configuration file:
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
    /// The `files` argument is a list of paths to the variant configuration
    /// files. The files are loaded in the order they are provided in the
    /// `files` argument. The keys of a later file replace keys from an
    /// earlier file (values are _not_ merged).
    ///
    /// A special key, the `zip_keys` is used to "zip" the values of two keys.
    /// For example, if the following configuration file is loaded:
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
    /// the variant configuration will be zipped so that the following variants
    /// are selected:
    ///
    /// ```txt
    /// [python=3.9, compiler=gcc]
    /// and
    /// [python=3.8, compiler=clang]
    /// ```
    pub fn from_files(
        files: &[PathBuf],
        selector_config: &SelectorConfig,
    ) -> Result<Self, VariantConfigError<Arc<str>>> {
        let mut variant_configs = Vec::new();

        for filename in files {
            tracing::info!("Loading variant config file: {:?}", filename);
            let config = Self::load_file(filename, selector_config)?;
            variant_configs.push(config);
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

        // always insert target_platform and build_platform
        final_config.variants.insert(
            "target_platform".into(),
            vec![selector_config.target_platform.to_string().into()],
        );
        final_config.variants.insert(
            "build_platform".into(),
            vec![selector_config.build_platform.to_string().into()],
        );

        Ok(final_config)
    }

    fn validate_zip_keys(&self) -> Result<(), VariantExpandError> {
        if let Some(zip_keys) = &self.zip_keys {
            for zip in zip_keys {
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

        let variant_keys = used_vars
            .iter()
            .filter_map(|key| {
                if let Some(values) = self.variants.get(key) {
                    if !zip_keys.iter().any(|zip| zip.contains(key)) {
                        return Some(VariantKey::Key(key.clone(), values.clone()));
                    }
                }
                None
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
                        already_used_vars
                            .iter()
                            .all(|(key, value)| combination.get(key) == Some(value))
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

impl TryConvertNode<VariantConfig> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<VariantConfig, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|map| map.try_convert(name))
    }
}

impl TryConvertNode<VariantConfig> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<VariantConfig, Vec<PartialParsingError>> {
        let mut config = VariantConfig::default();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "pin_run_as_build" => {
                    config.pin_run_as_build = value.try_convert(key_str)?;
                }
                "zip_keys" => {
                    config.zip_keys = value.try_convert(key_str)?;
                }
                _ => {
                    let variants: Option<Vec<Variable>> = value.try_convert(key_str)?;
                    if let Some(variants) = variants {
                        config.variants.insert(key_str.into(), variants);
                    }
                }
            }
        }

        Ok(config)
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

impl<S: SourceCode> ParseErrors<S> {
    fn from_partial_vec(source: S, errs: Vec<PartialParsingError>) -> Self
    where
        S: Clone + AsRef<str>,
    {
        Self {
            errs: ParsingError::from_partial_vec(source, errs),
        }
    }
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
    use rstest::rstest;

    use crate::{normalized_key::NormalizedKey, selectors::SelectorConfig};

    #[rstest]
    #[case("selectors/config_1.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let yaml = Node::parse_yaml(0, yaml_file.as_str()).unwrap();

        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant: Default::default(),
            hash: None,
            ..Default::default()
        };
        let jinja = Jinja::new(selector_config);

        let res: RenderedNode = yaml.render(&jinja, "test1").unwrap();
        let res: VariantConfig = res.try_convert("test1").unwrap();
        insta::assert_yaml_snapshot!(res);

        let selector_config = SelectorConfig {
            target_platform: Platform::Win64,
            host_platform: Platform::Win64,
            build_platform: Platform::Win64,
            ..Default::default()
        };
        let jinja = Jinja::new(selector_config);

        let res: RenderedNode = yaml.render(&jinja, "test2").unwrap();
        let res: VariantConfig = res.try_convert("test2").unwrap();
        insta::assert_yaml_snapshot!(res);
    }

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
            variant.variants.get(&"noboolean".into()).unwrap(),
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
    fn test_variant_combinations() {
        let mut variants = BTreeMap::<NormalizedKey, Vec<Variable>>::new();
        variants.insert("a".into(), vec!["1".into(), "2".into()]);
        variants.insert("b".into(), vec!["3".into(), "4".into()]);
        let zip_keys = vec![vec!["a".into(), "b".into()].into_iter().collect()];

        let used_vars = vec!["a".into()].into_iter().collect();
        let mut config = VariantConfig {
            variants,
            zip_keys: Some(zip_keys),
            pin_run_as_build: None,
        };

        let combinations = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combinations.len(), 2);

        let used_vars = vec!["a".into(), "b".into()].into_iter().collect();
        let combinations = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combinations.len(), 2);

        config
            .variants
            .insert("c".into(), vec!["5".into(), "6".into(), "7".into()]);
        let used_vars = vec!["a".into(), "b".into(), "c".into()]
            .into_iter()
            .collect();
        let combinations = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combinations.len(), 2 * 3);

        let used_vars = vec!["a".into(), "b".into(), "c".into()]
            .into_iter()
            .collect();
        config.zip_keys = None;
        let combinations = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combinations.len(), 2 * 2 * 3);

        let already_used_vars = BTreeMap::from_iter(vec![("a".into(), "1".into())]);
        let c2 = config
            .combinations(&used_vars, Some(&already_used_vars))
            .unwrap();
        println!("{:?}", c2);
        // for c in &c2 {
        //     assert!(c.get(&"a".into()).unwrap() == "1");
        // }
        assert!(c2.len() == 2 * 3);
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
