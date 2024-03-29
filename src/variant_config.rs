//! Functions to read and parse variant configuration files.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use indexmap::IndexSet;
use miette::Diagnostic;
use rattler_conda_types::{NoArchType, ParseVersionError, Platform};
use serde::{Deserialize, Serialize};

use thiserror::Error;

use crate::{
    _partialerror,
    hash::HashInfo,
    recipe::{
        custom_yaml::{HasSpan, Node, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        parser::Recipe,
        Jinja, Render,
    },
    selectors::SelectorConfig,
    used_variables::used_vars_from_expressions,
};
use crate::{recipe::parser::Dependency, utils::NormalizedKeyBTreeMap};
use petgraph::{algo::toposort, graph::DiGraph};

#[allow(missing_docs)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct DiscoveredOutput {
    pub name: String,
    pub version: String,
    pub build_string: String,
    pub noarch_type: NoArchType,
    pub target_platform: Platform,
    pub node: Node,
    pub used_vars: BTreeMap<String, String>,
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
                    )])
                }
            }
        }

        Ok(pin)
    }
}

// #[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
/// The variant configuration.
/// This is usually loaded from a YAML file and contains a mapping of package names to a list of
/// versions. Each version represents a variant of the package. The variant configuration is
/// used to create a build matrix for a recipe.
///
/// Example:
///
/// ```yaml
/// python:
/// - "3.10"
/// - "3.11"
/// ```
///
/// If you depend on Python in your recipe, this will create two variants of your recipe:
///
/// ```txt
/// [python=3.10]
/// and
/// [python=3.11]
/// ```
///
///
/// The variant configuration also contains a list of "zip keys". These are keys that are zipped
/// together to create a list of variants. For example, if the variant configuration contains the
/// following zip keys:
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
///
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
/// It's also possible to specify additional pins in the variant configuration. These pins are
/// currently ignored.
pub struct VariantConfig {
    /// Pin run dependencies by using the versions from the build dependencies (and applying the pin).
    /// This is currently ignored (TODO)
    pub pin_run_as_build: Option<BTreeMap<String, Pin>>,

    /// The zip keys are used to "zip" together variants to create specific combinations.
    pub zip_keys: Option<Vec<Vec<String>>>,

    /// The variants are a mapping of package names to a list of versions. Each version represents
    /// a variant for the build matrix.
    #[serde(flatten)]
    pub variants: NormalizedKeyBTreeMap,
}

#[allow(missing_docs)]
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum VariantConfigError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    RecipeParseErrors(#[from] ParseErrors),

    #[error("Could not parse variant config file ({0}): {1}")]
    ParseError(PathBuf, serde_yaml::Error),

    #[error("Could not open file ({0}): {1}")]
    IOError(PathBuf, std::io::Error),

    #[error(transparent)]
    #[diagnostic(transparent)]
    NewParseError(#[from] ParsingError),
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
    pub fn from_files(
        files: &Vec<PathBuf>,
        selector_config: &SelectorConfig,
    ) -> Result<Self, VariantConfigError> {
        let mut variant_configs = Vec::new();

        for filename in files {
            let file = std::fs::read_to_string(filename)
                .map_err(|e| VariantConfigError::IOError(filename.clone(), e))?;
            let yaml_node = Node::parse_yaml(0, &file)?;
            let jinja = Jinja::new(selector_config.clone());
            let rendered_node: RenderedNode = yaml_node
                .render(&jinja, filename.to_string_lossy().as_ref())
                .map_err(|e| ParseErrors::from_partial_vec(&file, e))?;
            let config: VariantConfig = rendered_node
                .try_convert(filename.to_string_lossy().as_ref())
                .map_err(|e| {
                    let parse_errors: ParseErrors = ParsingError::from_partial_vec(&file, e).into();
                    parse_errors
                })?;

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
            vec![selector_config.target_platform.to_string()],
        );
        final_config.variants.insert(
            "build_platform".into(),
            vec![selector_config.build_platform.to_string()],
        );

        Ok(final_config)
    }

    fn validate_zip_keys(&self) -> Result<(), VariantError> {
        if let Some(zip_keys) = &self.zip_keys {
            for zip in zip_keys {
                let mut prev_len = None;
                for key in zip {
                    let value = match self.variants.get(key) {
                        None => return Err(VariantError::InvalidZipKeyLength(key.to_string())),
                        Some(value) => value,
                    };

                    if let Some(l) = prev_len {
                        if l != value.len() {
                            return Err(VariantError::InvalidZipKeyLength(key.to_string()));
                        }
                    }
                    prev_len = Some(value.len());
                }
            }
        }
        Ok(())
    }

    /// This function returns all possible combinations of variants for the given set of used
    /// variables.
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

    /// This function finds all used variables in a recipe and expands the recipe to the full
    /// build matrix based on the variant configuration (loaded in the `SelectorConfig`).
    ///
    /// The result is a topologically sorted list of tuples. Each tuple contains the following
    /// elements:
    ///
    /// 1. The name of the package.
    /// 2. The version of the package.
    /// 3. The build string of the package.
    /// 4. The recipe node.
    /// 5. The used variant config.
    pub fn find_variants(
        &self,
        outputs: &[Node],
        recipe: &str,
        selector_config: &SelectorConfig,
    ) -> Result<IndexSet<DiscoveredOutput>, VariantError> {
        let mut outputs_map = HashMap::new();

        // sort the outputs by topological order
        for output in outputs.iter() {
            // for the topological sort we only take into account `pin_subpackage` expressions
            // in the recipe which are captured by the `used vars`
            let mut used_vars = used_vars_from_expressions(output, recipe).map_err(|e| {
                let errs: ParseErrors = e.into();
                errs
            })?;
            let parsed_recipe =
                Recipe::from_node(output, selector_config.clone()).map_err(|err| {
                    let errs: ParseErrors = err
                        .into_iter()
                        .map(|err| ParsingError::from_partial(recipe, err))
                        .collect::<Vec<ParsingError>>()
                        .into();
                    errs
                })?;
            let noarch_type = parsed_recipe.build().noarch();
            // add in any host and build dependencies
            used_vars.extend(parsed_recipe.requirements().all().filter_map(|dep| {
                match dep {
                    Dependency::Spec(spec) => {
                        // here we filter python as a variant and don't take it's passed variants
                        // when noarch is python
                        spec.name.as_ref().and_then(|name| {
                            let normalized_name = name.as_normalized();
                            if normalized_name == "python" && noarch_type.is_python() {
                                return None;
                            }
                            normalized_name.to_string().into()
                        })
                    }
                    Dependency::PinSubpackage(pin) => {
                        Some(pin.pin_value().name.as_normalized().to_string())
                    }
                    _ => None,
                }
            }));

            let use_keys = &parsed_recipe.build().variant().use_keys;
            used_vars.extend(use_keys.iter().cloned());

            let target_platform = if noarch_type.is_none() {
                selector_config.target_platform
            } else {
                Platform::NoArch
            };
            if outputs_map
                .insert(
                    parsed_recipe.package().name().as_normalized().to_string(),
                    (output, used_vars, target_platform),
                )
                .is_some()
            {
                return Err(VariantError::DuplicateOutputs(
                    parsed_recipe.package().name().as_normalized().to_string(),
                ));
            }
        }

        // now topologically sort the outputs and find cycles

        // Create an empty directed graph
        let mut graph = DiGraph::<_, ()>::new();

        // Create a map from output names to node indices
        let mut node_indices = HashMap::new();

        // TODO: this code can be improved in general
        // Add a node for each output
        for output in outputs_map.keys() {
            let node_index = graph.add_node(output.clone());
            node_indices.insert(output.clone(), node_index);
        }

        // Add an edge for each pair of outputs where one uses a variable defined by the other
        for (output, (_, used_vars, _)) in &outputs_map {
            let output_node_index = *node_indices
                .get(output)
                .expect("unreachable, we insert keys in the loop above");
            for used_var in used_vars {
                if outputs_map.contains_key(used_var) {
                    let defining_output_node_index = *node_indices
                        .get(used_var)
                        .expect("unreachable, we insert keys in the loop above");
                    // self referencing is possible, but not a cycle
                    if defining_output_node_index == output_node_index {
                        continue;
                    }
                    graph.add_edge(defining_output_node_index, output_node_index, ());
                }
            }
        }

        // Perform a topological sort
        let outputs: Vec<_> = match toposort(&graph, None) {
            Ok(sorted_node_indices) => {
                // Replace the original list of outputs with the sorted list
                sorted_node_indices
                    .into_iter()
                    .map(|node_index| graph[node_index].clone())
                    .collect()
            }
            Err(err) => {
                // There is a cycle in the graph
                return Err(VariantError::CycleInRecipeOutputs(
                    graph[err.node_id()].clone(),
                ));
            }
        };

        // sort the outputs map by the topological order
        let outputs_map = outputs
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let (node, used_vars, target_platform) = outputs_map[name].clone();
                (idx, (name, node, used_vars, target_platform))
            })
            .collect::<BTreeMap<_, _>>();

        let mut all_build_dependencies = Vec::new();
        for (_, (_, output, _, _)) in outputs_map.iter() {
            let parsed_recipe =
                Recipe::from_node(output, selector_config.clone()).map_err(|err| {
                    let errs: ParseErrors = err
                        .into_iter()
                        .map(|err| ParsingError::from_partial(recipe, err))
                        .collect::<Vec<ParsingError>>()
                        .into();
                    errs
                })?;
            let noarch_type = parsed_recipe.build().noarch();
            let build_time_requirements = parsed_recipe
                .requirements()
                .build_time()
                .cloned()
                .filter_map(|dep| {
                    // here we filter python as a variant and don't take it's passed variants
                    // when noarch is python
                    if let Dependency::Spec(spec) = &dep {
                        if let Some(name) = &spec.name {
                            if name.as_normalized() == "python" && noarch_type.is_python() {
                                return None;
                            }
                        }
                    }
                    Some(dep)
                });
            all_build_dependencies.extend(build_time_requirements);
        }

        let mut all_variables = all_build_dependencies
            .iter()
            .filter_map(|dep| match dep {
                Dependency::Spec(spec) => {
                    // filter all matchspecs that override the version or build
                    let is_simple = spec.version.is_none() && spec.build.is_none();
                    if is_simple && spec.name.is_some() {
                        spec.name
                            .as_ref()
                            .and_then(|name| name.as_normalized().to_string().into())
                    } else {
                        None
                    }
                }
                Dependency::PinSubpackage(pin_sub) => {
                    Some(pin_sub.pin_value().name.as_normalized().to_string())
                }
                _ => None,
            })
            .collect::<HashSet<_>>();

        // also add all used variables from the outputs
        for (_, (_, _, used_vars, _)) in outputs_map.iter() {
            all_variables.extend(used_vars.clone());
        }

        // remove all existing outputs from all_variables
        let output_names = outputs.iter().cloned().collect::<HashSet<_>>();
        let mut all_variables = all_variables
            .difference(&output_names)
            .cloned()
            .collect::<HashSet<_>>();

        // special handling of CONDA_BUILD_SYSROOT
        if all_variables.contains("c_compiler") || all_variables.contains("cxx_compiler") {
            all_variables.insert("CONDA_BUILD_SYSROOT".to_string());
        }

        // also always add `target_platform` and `channel_targets`
        all_variables.insert("target_platform".to_string());
        all_variables.insert("channel_targets".to_string());

        let combinations = self.combinations(&all_variables)?;

        // Then find all used variables from the each output recipe
        // let mut variants = Vec::new();
        let mut recipes = IndexSet::new();
        for combination in combinations {
            let mut other_recipes =
                HashMap::<String, (String, String, BTreeMap<String, String>)>::new();

            for (_, (name, output, used_vars, target_platform)) in outputs_map.iter() {
                let mut used_variables = used_vars.clone();
                let mut exact_pins = HashSet::new();

                // special handling of CONDA_BUILD_SYSROOT
                if used_variables.contains("c_compiler") || used_variables.contains("cxx_compiler")
                {
                    used_variables.insert("CONDA_BUILD_SYSROOT".to_string());
                }

                // also always add `target_platform` and `channel_targets`
                used_variables.insert("target_platform".to_string());
                used_variables.insert("channel_targets".to_string());

                let mut combination = combination.clone();
                // we need to overwrite the target_platform in case of `noarch`.
                combination.insert("target_platform".to_string(), target_platform.to_string());

                let selector_config_with_variant =
                    selector_config.new_with_variant(combination.clone(), *target_platform);

                let parsed_recipe = Recipe::from_node(output, selector_config_with_variant.clone())
                    .map_err(|err| {
                        let errs: ParseErrors = err
                            .into_iter()
                            .map(|err| ParsingError::from_partial(recipe, err))
                            .collect::<Vec<ParsingError>>()
                            .into();
                        errs
                    })?;

                // find the variables that were actually used in the recipe and that count towards the hash
                let requirements = parsed_recipe.requirements();
                requirements.build_time().for_each(|dep| match dep {
                    Dependency::Spec(spec) => {
                        if let Some(name) = &spec.name {
                            let val = name.as_normalized().to_owned();
                            used_variables.insert(val);
                        }
                    }
                    Dependency::PinSubpackage(pin_sub) => {
                        let pin = pin_sub.pin_value();
                        let val = pin.name.as_normalized().to_owned();
                        if pin.exact {
                            exact_pins.insert(val);
                        }
                    }
                    Dependency::PinCompatible(pin_compatible) => {
                        let pin = pin_compatible.pin_value();
                        let val = pin.name.as_normalized().to_owned();
                        if pin.exact {
                            exact_pins.insert(val);
                        }
                    }
                    // Be explicit about the other cases, so we can add them later
                    Dependency::Compiler(_) => (),
                });

                // actually used vars
                let mut used_filtered = combination
                    .clone()
                    .into_iter()
                    .filter(|(k, _)| used_variables.contains(k))
                    .collect::<BTreeMap<_, _>>();

                // exact pins
                for p in exact_pins {
                    match other_recipes.get(&p) {
                        Some((version, build, _)) => {
                            used_filtered.insert(p.clone(), format!("{} {}", version, build));
                        }
                        None => {
                            return Err(VariantError::MissingOutput(p));
                        }
                    }
                }

                requirements
                    .run
                    .iter()
                    .chain(requirements.run_constraints.iter())
                    .chain(requirements.run_exports().all())
                    .try_for_each(|dep| -> Result<(), VariantError> {
                        if let Dependency::PinSubpackage(pin_sub) = dep {
                            let pin = pin_sub.pin_value();
                            if pin.exact {
                                let val = pin.name.as_normalized();
                                if val != *name {
                                    // if other_recipes does not contain val, throw an error
                                    match other_recipes.get(val) {
                                        Some((version, build, _)) => {
                                            used_filtered.insert(
                                                val.to_owned(),
                                                format!("{} {}", version, build),
                                            );
                                        }
                                        None => {
                                            return Err(VariantError::MissingOutput(val.into()));
                                        }
                                    }
                                }
                            }
                        }
                        Ok(())
                    })?;

                // compute hash for the recipe
                let hash = HashInfo::from_variant(&used_filtered, parsed_recipe.build().noarch());
                // TODO(wolf) can we make this computation better by having some nice API on Output?
                // get the real build string from the recipe
                let selector_config_with_hash = SelectorConfig {
                    hash: Some(hash.clone()),
                    ..selector_config_with_variant
                };
                let parsed_recipe =
                    Recipe::from_node(output, selector_config_with_hash).map_err(|err| {
                        let errs: ParseErrors = err
                            .into_iter()
                            .map(|err| ParsingError::from_partial(recipe, err))
                            .collect::<Vec<ParsingError>>()
                            .into();
                        errs
                    })?;

                let build_string = parsed_recipe
                    .build()
                    .string()
                    .unwrap_or(&hash.to_string())
                    .to_string();

                other_recipes.insert(
                    parsed_recipe.package().name().as_normalized().to_string(),
                    (
                        parsed_recipe.package().version().to_string(),
                        parsed_recipe
                            .build()
                            .string()
                            .unwrap_or(&hash.to_string())
                            .to_string(),
                        used_filtered.clone(),
                    ),
                );
                let version = parsed_recipe.package().version().to_string();

                let ignore_keys = &parsed_recipe.build().variant().ignore_keys;
                used_filtered.retain(|k, _| ignore_keys.is_empty() || !ignore_keys.contains(k));

                recipes.insert(DiscoveredOutput {
                    name: name.to_string(),
                    version,
                    build_string,
                    noarch_type: *parsed_recipe.build().noarch(),
                    target_platform: *target_platform,
                    node: (*output).to_owned(),
                    used_vars: used_filtered,
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
                    let variants: Option<Vec<_>> = value.try_convert(key_str)?;
                    if let Some(variants) = variants {
                        config
                            .variants
                            .insert(key_str.to_string(), variants.clone());
                    }
                }
            }
        }

        Ok(config)
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

#[derive(Error, Debug, Diagnostic)]
#[error("Failed to parse recipe")]
/// Collection of parse errors to build related diagnostics
/// TODO: also provide `Vec<PartialParsingError>` with source `&str`
/// to avoid excessive traversal
pub struct ParseErrors {
    #[related]
    errs: Vec<ParsingError>,
}
impl ParseErrors {
    fn from_partial_vec(file: &str, errs: Vec<PartialParsingError>) -> Self {
        Self {
            errs: ParsingError::from_partial_vec(file, errs),
        }
    }
}

impl From<Vec<ParsingError>> for ParseErrors {
    fn from(errs: Vec<ParsingError>) -> Self {
        Self { errs }
    }
}

#[allow(missing_docs)]
#[derive(Error, Debug, Diagnostic)]
pub enum VariantError {
    #[error("Zip key elements do not all have same length: {0}")]
    InvalidZipKeyLength(String),

    #[error("Failed to parse version: {0}")]
    RecipeParseVersionError(#[from] ParseVersionError),

    #[error(transparent)]
    #[diagnostic(transparent)]
    RecipeParseErrors(#[from] ParseErrors),

    #[error(transparent)]
    #[diagnostic(transparent)]
    RecipeParseError(#[from] ParsingError),

    #[error("Duplicate outputs: {0}")]
    DuplicateOutputs(String),

    #[error("Missing output: {0} (used in pin_subpackage)")]
    MissingOutput(String),

    #[error("Found a cycle in the recipe outputs: {0}")]
    CycleInRecipeOutputs(String),
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
    use crate::selectors::SelectorConfig;
    use rattler_conda_types::Platform;
    use rstest::rstest;

    #[rstest]
    #[case("selectors/config_1.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(dbg!(test_data_dir.join(filename))).unwrap();
        let yaml = Node::parse_yaml(0, &yaml_file).unwrap();

        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
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
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        let variant = VariantConfig::from_files(&vec![yaml_file], &selector_config).unwrap();

        insta::assert_yaml_snapshot!(variant);
    }

    #[test]
    fn test_load_config_and_find_variants() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("recipes/variants/variant_config.yaml");
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        // First find all outputs from the recipe
        let recipe_text =
            std::fs::read_to_string(test_data_dir.join("recipes/variants/recipe.yaml")).unwrap();
        let outputs = crate::recipe::parser::find_outputs_from_src(&recipe_text).unwrap();
        let variant_config = VariantConfig::from_files(&vec![yaml_file], &selector_config).unwrap();
        let outputs_and_variants = variant_config
            .find_variants(&outputs, &recipe_text, &selector_config)
            .unwrap();

        let used_variables_all: Vec<&BTreeMap<String, String>> = outputs_and_variants
            .as_slice()
            .into_iter()
            .map(|s| &s.used_vars)
            .collect();

        insta::assert_yaml_snapshot!(used_variables_all);
    }

    use super::*;

    #[test]
    fn test_variant_combinations() {
        let mut variants = NormalizedKeyBTreeMap::new();
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

    #[test]
    fn test_order() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        for _ in 1..3 {
            // First find all outputs from the recipe
            let recipe_text =
                std::fs::read_to_string(test_data_dir.join("recipes/output_order/order_1.yaml"))
                    .unwrap();
            let outputs = crate::recipe::parser::find_outputs_from_src(&recipe_text).unwrap();
            let variant_config = VariantConfig::from_files(&vec![], &selector_config).unwrap();
            let outputs_and_variants = variant_config
                .find_variants(&outputs, &recipe_text, &selector_config)
                .unwrap();

            // assert output order
            let order = vec!["some-pkg-a", "some-pkg", "some_pkg"];
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
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        // First find all outputs from the recipe
        let recipe_text =
            std::fs::read_to_string(test_data_dir.join("recipes/variants/boltons_recipe.yaml"))
                .unwrap();
        let outputs = crate::recipe::parser::find_outputs_from_src(&recipe_text).unwrap();
        let variant_config = VariantConfig::from_files(&vec![yaml_file], &selector_config).unwrap();
        let outputs_and_variants = variant_config
            .find_variants(&outputs, &recipe_text, &selector_config)
            .unwrap();

        let used_variables_all: Vec<&BTreeMap<String, String>> = outputs_and_variants
            .as_slice()
            .into_iter()
            .map(|s| &s.used_vars)
            .collect();

        insta::assert_yaml_snapshot!(used_variables_all);
    }
}
