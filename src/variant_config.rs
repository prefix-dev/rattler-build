//! Functions to read and parse variant configuration files.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use indexmap::IndexSet;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use serde_with::{formats::PreferOne, serde_as, OneOrMany};
use thiserror::Error;

use crate::{
    _partialerror,
    hash::compute_buildstring,
    recipe::{
        custom_yaml::{HasSpan, Node, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        parser::Recipe,
        Jinja, Render,
    },
    selectors::SelectorConfig,
    used_variables::used_vars_from_expressions,
};
use petgraph::{algo::toposort, graph::DiGraph};

type OutputVariantsTuple = (String, String, String, Node, BTreeMap<String, String>);

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Pin {
    pub max_pin: Option<String>,
    pub min_pin: Option<String>,
}

impl TryConvertNode<Pin> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Pin, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping,))
            .and_then(|map| map.try_convert(name))
    }
}

impl TryConvertNode<Pin> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Pin, PartialParsingError> {
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
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(key_str.to_string().into()),
                        help = format!("Valid fields for {name} are: max_pin, min_pin")
                    ))
                }
            }
        }

        Ok(pin)
    }
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

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum VariantConfigError {
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
                .map_err(|e| ParsingError::from_partial(&file, e))?;
            let config: VariantConfig = rendered_node
                .try_convert(filename.to_string_lossy().as_ref())
                .map_err(|e| ParsingError::from_partial(&file, e))?;

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
                    let value = self.variants.get(key);
                    if value.is_none() {
                        return Err(VariantError::InvalidZipKeyLength(key.to_string()));
                    }
                    let len = value.unwrap().len();
                    if let Some(l) = prev_len {
                        if l != len {
                            return Err(VariantError::InvalidZipKeyLength(key.to_string()));
                        }
                    }
                    prev_len = Some(len);
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

    /// This finds all used variables in any dependency declarations, build, host, and run sections.
    /// As well as any used variables from Jinja functions to calculate the variants of this recipe.
    ///
    /// We also split the recipe into multiple outputs and topologically sort them (as well as deduplicate)
    pub fn find_variants(
        &self,
        recipe: &str,
        selector_config: &SelectorConfig,
    ) -> Result<IndexSet<OutputVariantsTuple>, VariantError> {
        use crate::recipe::parser::{find_outputs_from_src, Dependency};

        // First find all outputs from the recipe
        let outputs = find_outputs_from_src(recipe)?;

        let mut outputs_map = HashMap::new();
        // sort the outputs by topological order
        for output in outputs.iter() {
            // for the topological sort we only take into account `pin_subpackage` expressions
            // in the recipe which are captured by the `used vars`
            let used_vars = used_vars_from_expressions(output);
            let parsed_recipe = Recipe::from_node(output, selector_config.clone())
                .map_err(|err| ParsingError::from_partial(recipe, err))?;

            if outputs_map
                .insert(
                    parsed_recipe.package().name().as_normalized().to_string(),
                    (output, used_vars),
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

        // Add a node for each output
        for output in outputs_map.keys() {
            let node_index = graph.add_node(output.clone());
            node_indices.insert(output.clone(), node_index);
        }

        // Add an edge for each pair of outputs where one uses a variable defined by the other
        for (output, (_, used_vars)) in &outputs_map {
            let output_node_index = *node_indices.get(output).unwrap();
            for used_var in used_vars {
                if outputs_map.contains_key(used_var) {
                    let defining_output_node_index = *node_indices.get(used_var).unwrap();
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

        println!("Sorted outputs: {:?}", outputs);

        // sort the outputs map by the topological order
        let outputs_map = outputs
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let (node, used_vars) = outputs_map[name].clone();
                (idx, (name, node, used_vars))
            })
            .collect::<BTreeMap<_, _>>();

        let mut all_build_dependencies = Vec::new();
        for (_, (name, output, used_vars)) in outputs_map.clone().iter() {
            println!("Output: {}: {:?}", name, used_vars);

            let parsed_recipe = Recipe::from_node(output, selector_config.clone())
                .map_err(|err| ParsingError::from_partial(recipe, err))?;

            let build_time_requirements = parsed_recipe.requirements().build_time().cloned();
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
                _ => None,
            })
            .collect::<HashSet<_>>();

        // also add all used variables from the outputs
        for (_, (_, _, used_vars)) in outputs_map.iter() {
            all_variables.extend(used_vars.clone());
        }
        // remove all existing outputs from all_variables
        let output_names = outputs.iter().cloned().collect::<HashSet<_>>();
        let all_variables = all_variables
            .difference(&output_names)
            .cloned()
            .collect::<HashSet<_>>();
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

        println!("All build dependencies: {:?}", all_variables);

        let combinations = self.combinations(&all_variables)?;
        println!("Combinations: {:?}", combinations);

        // Then find all used variables from the each output recipe
        // let mut variants = Vec::new();
        let mut recipes = IndexSet::new();
        for combination in combinations {
            let mut other_recipes =
                HashMap::<String, (String, String, BTreeMap<String, String>)>::new();

            for (_, (name, output, used_vars)) in outputs_map.iter() {
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

                let selector_config_with_variant =
                    selector_config.new_with_variant(combination.clone());

                let parsed_recipe = Recipe::from_node(output, selector_config_with_variant)
                    .map_err(|err| ParsingError::from_partial(recipe, err))?;

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
                used_filtered.extend(exact_pins.into_iter().map(|k| {
                    (
                        k.clone(),
                        format!("{} {}", other_recipes[&k].0, other_recipes[&k].1),
                    )
                }));

                requirements
                    .run
                    .iter()
                    .chain(requirements.run_constrained.iter())
                    .chain(parsed_recipe.build().run_exports().all())
                    .for_each(|dep| {
                        if let Dependency::PinSubpackage(pin_sub) = dep {
                            let pin = pin_sub.pin_value();
                            if pin.exact {
                                let val = pin.name.as_normalized().to_owned();
                                used_filtered.insert(
                                    val.clone(),
                                    format!("{} {}", other_recipes[&val].0, other_recipes[&val].1),
                                );
                            }
                        }
                    });

                // compute hash for the recipe
                let hash = compute_buildstring(&used_filtered, parsed_recipe.build().noarch());

                other_recipes.insert(
                    parsed_recipe.package().name().as_normalized().to_string(),
                    (
                        parsed_recipe.package().version().to_string(),
                        hash.clone(),
                        used_filtered.clone(),
                    ),
                );

                let version = parsed_recipe.package().version().to_string();

                recipes.insert((name, version, hash, output, used_filtered));
            }
        }

        for r in recipes.iter() {
            println!("Recipe: {} {} {} {:?}", r.0, r.1, r.2, r.4);
        }

        Ok(recipes
            .into_iter()
            .map(|(&r1, r2, r3, &r4, r5)| (r1.clone(), r2, r3, r4.clone(), r5))
            .collect::<IndexSet<_>>())
    }
}

impl TryConvertNode<VariantConfig> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<VariantConfig, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping))
            .and_then(|map| map.try_convert(name))
    }
}

impl TryConvertNode<VariantConfig> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<VariantConfig, PartialParsingError> {
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
                        config.variants.insert(key_str.to_string(), variants);
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
pub enum VariantError {
    #[error("Zip key elements do not all have same length: {0}")]
    InvalidZipKeyLength(String),

    #[error(transparent)]
    #[diagnostic(transparent)]
    RecipeParseError(#[from] ParsingError),

    #[error("Duplicate outputs: {0}")]
    DuplicateOutputs(String),

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
        };
        let jinja = Jinja::new(selector_config);

        let res: RenderedNode = yaml.render(&jinja, "test1").unwrap();
        let res: VariantConfig = res.try_convert("test1").unwrap();
        insta::assert_yaml_snapshot!(res);

        let selector_config = SelectorConfig {
            target_platform: Platform::Win64,
            build_platform: Platform::Win64,
            variant: Default::default(),
            hash: None,
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
            variant: Default::default(),
            hash: None,
        };

        let variant = VariantConfig::from_files(&vec![yaml_file], &selector_config).unwrap();

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
