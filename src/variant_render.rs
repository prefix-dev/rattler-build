use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use petgraph::graph::DiGraph;
use rattler_conda_types::PackageName;

use crate::{
    env_vars,
    hash::HashInfo,
    normalized_key::NormalizedKey,
    recipe::{custom_yaml::Node, parser::Dependency, Jinja, ParsingError, Recipe},
    selectors::SelectorConfig,
    used_variables::used_vars_from_expressions,
    variant_config::{ParseErrors, VariantConfig, VariantError},
};

/// All the raw outputs of a single recipe.yaml
#[derive(Clone, Debug)]
pub struct RawOutputVec {
    /// The raw node (slightly preprocessed by making sure all keys are there)
    pub vec: Vec<Node>,

    /// The used variables in each of the nodes
    pub used_vars_jinja: Vec<HashSet<NormalizedKey>>,

    /// The recipe string
    #[allow(unused)]
    pub recipe: String,
}

/// Stage 0 render of a single recipe.yaml
#[derive(Clone, Debug)]
pub struct Stage0Render {
    /// The used variables with their values
    pub variables: BTreeMap<NormalizedKey, String>,

    /// The raw outputs of the recipe
    pub raw_outputs: RawOutputVec,

    // Pre-rendered recipe nodes
    pub rendered_outputs: Vec<Recipe>,
}

impl Stage0Render {
    pub fn outputs(&self) -> impl Iterator<Item = (&Node, &Recipe)> {
        self.raw_outputs
            .vec
            .iter()
            .zip(self.rendered_outputs.iter())
    }
}

pub(crate) fn stage_0_render(
    outputs: &[Node],
    recipe: &str,
    selector_config: &SelectorConfig,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage0Render>, VariantError> {
    let used_vars = outputs
        .iter()
        .map(|output| {
            used_vars_from_expressions(output, recipe)
                .map(|x| x.into_iter().map(Into::into).collect())
        })
        .collect::<Result<Vec<HashSet<NormalizedKey>>, Vec<ParsingError>>>();

    // If there are any parsing errors, return them
    if let Err(errors) = used_vars {
        let err: ParseErrors = errors.into();
        return Err(VariantError::RecipeParseErrors(err));
    }

    let raw_output_vec = RawOutputVec {
        vec: outputs.to_vec(),
        used_vars_jinja: used_vars.unwrap(),
        recipe: recipe.to_string(),
    };

    // find all the jinja variables from all the expressions
    let mut used_vars = HashSet::<NormalizedKey>::new();
    for output in outputs {
        used_vars.extend(
            used_vars_from_expressions(output, recipe)
                .unwrap()
                .into_iter()
                .map(Into::into),
        );
    }

    // Now we need to create all the combinations of the variables x variant config
    let mut stage0_renders = Vec::new();
    let combinations = variant_config.combinations(&used_vars, None)?;

    for combination in combinations {
        let mut rendered_outputs = Vec::new();
        // TODO: figure out if we can pre-compute the `noarch` value.
        for output in outputs {
            let config_with_variant =
                selector_config.with_variant(combination.clone(), selector_config.target_platform);

            let parsed_recipe = Recipe::from_node(output, config_with_variant).map_err(|err| {
                let errs: ParseErrors = err
                    .into_iter()
                    .map(|err| ParsingError::from_partial(recipe, err))
                    .collect::<Vec<ParsingError>>()
                    .into();
                errs
            })?;

            rendered_outputs.push(parsed_recipe);
        }

        stage0_renders.push(Stage0Render {
            variables: combination,
            raw_outputs: raw_output_vec.clone(),
            rendered_outputs,
        });
    }

    Ok(stage0_renders)
}

/// Stage 1 render of a single recipe.yaml
#[derive(Debug)]
pub struct Stage1Render {
    pub(crate) variables: BTreeMap<NormalizedKey, String>,

    pub(crate) inner: Vec<Stage1Inner>,

    pub(crate) stage_0_render: Stage0Render,

    order: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Stage1Inner {
    pub(crate) used_vars_from_dependencies: HashSet<NormalizedKey>,
    pub(crate) exact_pins: HashSet<PackageName>,
    pub(crate) recipe: Recipe,
    pub(crate) selector_config: SelectorConfig,
}

impl Stage1Render {
    pub fn index_from_name(&self, package_name: &PackageName) -> Option<usize> {
        self.inner
            .iter()
            .position(|x| x.recipe.package().name() == package_name)
    }

    pub fn variant_for_output(&self, idx: usize) -> BTreeMap<NormalizedKey, String> {
        tracing::info!("Getting variant for output {}", idx);
        let idx = self.order[idx];
        let inner = &self.inner[idx];
        // combine jinja variables and the variables from the dependencies
        let self_name = self.stage_0_render.rendered_outputs[idx].package().name();
        let used_vars_jinja = &self.stage_0_render.raw_outputs.used_vars_jinja[idx];

        let mut all_vars = inner.used_vars_from_dependencies.clone();

        all_vars.extend(used_vars_jinja.iter().cloned());

        // extract variant
        let mut variant = BTreeMap::new();
        for var in all_vars {
            if let Some(val) = self.variables.get(&var) {
                variant.insert(var, val.clone());
            }
        }

        for pin in &inner.exact_pins {
            if pin == self_name {
                continue;
            }
            let other_idx = self.index_from_name(pin).unwrap();
            // find the referenced output
            let build_string = self.build_string_for_output(other_idx);
            let version = self.inner[other_idx].recipe.package().version();
            variant.insert(
                pin.as_normalized().into(),
                format!("{} {}", version, build_string),
            );
        }

        // fix target_platform value here
        if !self.inner[idx].recipe.build().noarch().is_none() {
            variant.insert("target_platform".into(), "noarch".into());
        }

        variant
    }

    pub fn build_string_for_output(&self, idx: usize) -> String {
        let variant = self.variant_for_output(idx);
        let recipe = &self.stage_0_render.rendered_outputs[self.order[idx]];
        let hash = HashInfo::from_variant(&variant, recipe.build().noarch());
        let inner = &self.inner[self.order[idx]];

        let mut selector_config = inner.selector_config.clone();
        selector_config.hash = Some(hash.clone());
        let jinja = Jinja::new(selector_config.clone()).with_context(&recipe.context);

        recipe
            .build()
            .string()
            .resolve(&hash, recipe.build().number, &jinja)
            .into_owned()
    }

    /// sort the outputs topologically
    pub fn sort_outputs(self) -> Self {
        // Create an empty directed graph
        let mut graph = DiGraph::<_, ()>::new();
        let mut node_indices = Vec::new();
        let mut name_to_idx = HashMap::new();

        for output in &self.stage_0_render.rendered_outputs {
            let node_index = graph.add_node(output);
            name_to_idx.insert(output.package().name(), node_index);
            node_indices.push(node_index);
        }

        for (idx, output) in self.stage_0_render.rendered_outputs.iter().enumerate() {
            let output_name = output.package().name();
            let current_node = node_indices[idx];

            // Helper closure to add edges for dependencies
            let mut add_edge = |req_name: &PackageName| {
                if req_name != output_name {
                    if let Some(&req_idx) = name_to_idx.get(req_name) {
                        graph.add_edge(req_idx, current_node, ());
                    }
                }
            };

            // If we find any keys that reference another output, add an edge
            for req in output.build_time_requirements() {
                if let Dependency::Spec(spec) = req {
                    add_edge(spec.name.as_ref().expect("Dependency should have a name"));
                };
            }

            for pin in output.requirements().all_pin_subpackage() {
                add_edge(&pin.name);
            }
        }

        // Sort the outputs topologically
        let sorted_indices =
            petgraph::algo::toposort(&graph, None).expect("Could not sort topologically.");

        let sorted_indices = sorted_indices
            .into_iter()
            .map(|x| x.index())
            .collect::<Vec<usize>>();

        // Update the order of the outputs
        Stage1Render {
            order: sorted_indices,
            ..self
        }
    }

    pub fn outputs(
        &self,
    ) -> impl Iterator<Item = ((&Node, &Recipe), BTreeMap<NormalizedKey, String>)> {
        // zip node from stage0 and final render output
        let raw_nodes = self.stage_0_render.raw_outputs.vec.iter();
        let outputs: Vec<&Recipe> = self.inner.iter().map(|i| &i.recipe).collect();

        let zipped = raw_nodes.zip(outputs).collect::<Vec<_>>();

        (0..zipped.len()).map(move |idx| {
            let recipe = zipped[self.order[idx]];
            let variant = self.variant_for_output(idx);
            (recipe, variant)
        })
    }
}

/// Render the stage 1 of the recipe by adding in variants from the dependencies
pub(crate) fn stage_1_render(
    stage0_renders: Vec<Stage0Render>,
    selector_config: &SelectorConfig,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage1Render>, VariantError> {
    let mut stage_1_renders = Vec::new();

    // TODO we need to add variables from the cache output here!
    for r in stage0_renders {
        let mut extra_vars_per_output: Vec<HashSet<NormalizedKey>> = Vec::new();
        let mut exact_pins_per_output: Vec<HashSet<PackageName>> = Vec::new();
        for (idx, output) in r.rendered_outputs.iter().enumerate() {
            let mut additional_variables = HashSet::<NormalizedKey>::new();
            let mut exact_pins = HashSet::<PackageName>::new();
            // Add in variants from the dependencies as we find them
            for dep in output.build_time_requirements() {
                if let Dependency::Spec(spec) = dep {
                    let is_simple = spec.version.is_none() && spec.build.is_none();
                    // add in the variant key for this dependency that has no version specifier
                    if is_simple {
                        if let Some(ref name) = spec.name {
                            additional_variables.insert(name.as_normalized().into());
                        }
                    }
                }
            }

            // We want to add something to packages that are requiring a subpackage _exactly_ because
            // that creates additional variants
            for pin in output.requirements.all_pin_subpackage() {
                if pin.args.exact {
                    let name = pin.name.clone().as_normalized().to_string();
                    additional_variables.insert(name.into());
                    exact_pins.insert(pin.name.clone());
                }
            }

            // add in virtual package run specs where the name starts with `__`
            for run_req in output.requirements().run() {
                if let Dependency::Spec(spec) = run_req {
                    if let Some(ref name) = spec.name {
                        if name.as_normalized().starts_with("__") {
                            additional_variables.insert(name.as_normalized().into());
                        }
                    }
                }
            }

            // Add in extra `use` keys from the output
            let extra_use_keys = output
                .build()
                .variant()
                .use_keys
                .clone()
                .into_iter()
                .map(Into::into)
                .collect::<Vec<NormalizedKey>>();

            additional_variables.extend(extra_use_keys);

            // If the recipe is `noarch: python` we can remove an empty python key that comes from the dependencies
            if output.build().noarch().is_python() {
                additional_variables.remove(&"python".into());
            }

            // special handling of CONDA_BUILD_SYSROOT
            let jinja_variables = r.raw_outputs.used_vars_jinja.get(idx).unwrap();
            if jinja_variables.contains(&"c_compiler".into())
                || jinja_variables.contains(&"cxx_compiler".into())
            {
                additional_variables.insert("CONDA_BUILD_SYSROOT".into());
            }

            // also always add `target_platform` and `channel_targets`
            additional_variables.insert("target_platform".into());
            additional_variables.insert("channel_targets".into());

            // Environment variables can be overwritten by the variant configuration
            let env_vars = env_vars::os_vars(&PathBuf::new(), &selector_config.target_platform);
            additional_variables.extend(env_vars.keys().cloned().map(Into::into));

            // filter out any ignore keys
            let extra_ignore_keys: HashSet<NormalizedKey> = output
                .build()
                .variant()
                .ignore_keys
                .clone()
                .into_iter()
                .map(Into::into)
                .collect();

            additional_variables.retain(|x| !extra_ignore_keys.contains(x));

            extra_vars_per_output.push(additional_variables);
            exact_pins_per_output.push(exact_pins);
        }

        // Create the additional combinations and attach the whole variant x outputs to the stage 1 render
        let mut all_vars = extra_vars_per_output
            .iter()
            .fold(HashSet::new(), |acc, x| acc.union(x).cloned().collect());

        all_vars.extend(r.variables.keys().cloned());

        let all_combinations = variant_config.combinations(&all_vars, Some(&r.variables))?;

        for combination in all_combinations {
            let mut inner = Vec::new();

            // TODO: figure out if we can pre-compute the `noarch` value.
            for (idx, output) in r.raw_outputs.vec.iter().enumerate() {
                // use the correct target_platform here?
                let config_with_variant = selector_config
                    .with_variant(combination.clone(), selector_config.target_platform);

                let parsed_recipe = Recipe::from_node(output, config_with_variant.clone()).unwrap();

                inner.push(Stage1Inner {
                    used_vars_from_dependencies: extra_vars_per_output[idx].clone(),
                    exact_pins: exact_pins_per_output[idx].clone(),
                    recipe: parsed_recipe,
                    selector_config: config_with_variant,
                })
            }

            let stage_1 = Stage1Render {
                inner,
                variables: combination,
                stage_0_render: r.clone(),
                order: (0..r.rendered_outputs.len()).collect(),
            }
            .sort_outputs();

            stage_1_renders.push(stage_1);
        }
    }

    Ok(stage_1_renders)
}
