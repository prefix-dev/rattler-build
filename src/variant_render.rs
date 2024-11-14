use std::collections::{BTreeMap, HashMap, HashSet};

use petgraph::graph::DiGraph;

use crate::{
    hash::HashInfo,
    normalized_key::NormalizedKey,
    recipe::{custom_yaml::Node, parser::Dependency, ParsingError, Recipe},
    selectors::SelectorConfig,
    used_variables::used_vars_from_expressions,
    variant_config::{ParseErrors, VariantConfig, VariantError},
};

// pub struct VariantKey(String);

/// All the raw outputs of a single recipe.yaml
#[derive(Clone, Debug)]
pub struct RawOutputVec {
    /// The raw node (slightly preprocessed by making sure all keys are there)
    pub vec: Vec<Node>,

    /// The used variables in each of the nodes
    pub used_vars_jinja: Vec<HashSet<NormalizedKey>>,

    /// The recipe string
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
    println!("Outputs: {:?}", outputs);
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
            let config_with_variant = selector_config
                .new_with_variant(combination.clone(), selector_config.target_platform);

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

    println!("Stage 0 renders: {:?}", stage0_renders);
    Ok(stage0_renders)
}

/// Stage 1 render of a single recipe.yaml
#[derive(Debug)]
pub struct Stage1Render {
    pub(crate) variables: BTreeMap<NormalizedKey, String>,

    // Per recipe output a set of extra used variables
    pub(crate) used_variables_from_dependencies: Vec<HashSet<NormalizedKey>>,

    pub(crate) stage_0_render: Stage0Render,

    order: Vec<usize>,
}

impl Stage1Render {
    pub fn variant_for_output(&self, idx: usize) -> BTreeMap<NormalizedKey, String> {
        let idx = self.order[idx];
        // combine jinja variables and the variables from the dependencies
        let used_vars_jinja = self
            .stage_0_render
            .raw_outputs
            .used_vars_jinja
            .get(idx)
            .unwrap();

        let mut all_vars = self
            .used_variables_from_dependencies
            .get(idx)
            .unwrap()
            .clone();

        all_vars.extend(used_vars_jinja.iter().cloned());

        // extract variant
        let mut variant = BTreeMap::new();
        for var in all_vars {
            if let Some(val) = self.variables.get(&var) {
                variant.insert(var, val.clone());
            }
        }

        variant
    }

    pub fn build_string_for_output(&self, idx: usize) -> String {
        let idx = self.order[idx];

        let variant = self.variant_for_output(idx);
        let recipe = &self.stage_0_render.rendered_outputs[idx];
        let hash = HashInfo::from_variant(&variant, recipe.build().noarch());

        let build_string = recipe
            .build()
            .string()
            .resolve(&hash, recipe.build().number)
            .into_owned();

        build_string
    }

    /// sort the outputs topologically
    pub fn sort_outputs(self) -> Self {
        // Create an empty directed graph
        let mut graph = DiGraph::<_, ()>::new();

        // Create a map from output names to node indices
        let mut node_indices = Vec::new();

        let mut name_to_idx = HashMap::new();

        for output in &self.stage_0_render.rendered_outputs {
            let node_index = graph.add_node(output);
            name_to_idx.insert(output.package().name(), node_index);
            node_indices.push(node_index);
        }

        for (idx, output) in self.stage_0_render.rendered_outputs.iter().enumerate() {
            // If we find any keys that reference another output, add an edge
            for req in output.build_time_requirements() {
                let req_name = match req {
                    Dependency::Spec(x) => x.name.clone().expect("Dependency should have a name"),
                    Dependency::PinSubpackage(x) => x.pin_value().name.clone(),
                    _ => continue,
                };

                if req_name != *output.package().name() {
                    if let Some(&req_idx) = name_to_idx.get(&req_name) {
                        graph.add_edge(req_idx, node_indices[idx], ());
                    }
                }
            }
        }

        // Sort the outputs topologically
        let sorted_indices =
            petgraph::algo::toposort(&graph, None).expect("Could not sort topologically.");

        println!("Sorted indices: {:?}", sorted_indices);

        let sorted_indices = sorted_indices
            .into_iter()
            .map(|x| x.index())
            .collect::<Vec<usize>>();

        // Update the order of the outputs
        return Stage1Render {
            order: sorted_indices,
            ..self
        };
    }

    pub fn outputs(
        &self,
    ) -> impl Iterator<Item = ((&Node, &Recipe), BTreeMap<NormalizedKey, String>)> {
        let outputs = self.stage_0_render.outputs().collect::<Vec<_>>();
        for o in &outputs {
            println!("Output: {:?}", o.1.package().name());
        }
        println!("Order: {:?}", self.order);

        self.order.iter().map(move |&idx| {
            let recipe = outputs[idx];
            // WRONG
            let variant = self.variant_for_output(idx);
            (recipe, variant)
        })
    }
}

/// Render the stage 1 of the recipe by adding in variants from the dependencies
pub(crate) fn stage_1_render(
    stage0_renders: Vec<Stage0Render>,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage1Render>, VariantError> {
    let mut stage_1_renders = Vec::new();

    println!("Stage 0: {:?}", stage0_renders);
    // TODO we need to add variables from the cache output here!
    for r in stage0_renders {
        let mut extra_vars_per_output: Vec<HashSet<NormalizedKey>> = Vec::new();
        for (idx, output) in r.rendered_outputs.iter().enumerate() {
            let mut additional_variables = HashSet::<NormalizedKey>::new();
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
            for req in output.requirements.all_requirements() {
                match req {
                    Dependency::PinSubpackage(pin) => {
                        if pin.pin_value().args.exact {
                            let name = pin.pin_value().name.clone().as_normalized().to_string();
                            additional_variables.insert(name.into());
                        }
                    }
                    Dependency::Spec(spec) => {
                        // add in virtual package specs where the name starts with `__`
                        if let Some(ref name) = spec.name {
                            if name.as_normalized().starts_with("__") {
                                additional_variables.insert(name.as_normalized().into());
                            }
                        }
                    }
                    _ => {}
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
        }

        println!("All vars: {:?}", extra_vars_per_output);
        // Create the additional combinations and attach the whole variant x outputs to the stage 1 render
        let mut all_vars = extra_vars_per_output
            .iter()
            .fold(HashSet::new(), |acc, x| acc.union(x).cloned().collect());

        println!("All vars from deps: {:?}", all_vars);
        println!(
            "All variables from recipes: {:?}",
            r.variables.keys().cloned().collect::<Vec<NormalizedKey>>()
        );
        all_vars.extend(r.variables.keys().cloned());

        let all_combinations = variant_config.combinations(&all_vars, Some(&r.variables))?;

        for combination in all_combinations {
            let stage_1 = Stage1Render {
                variables: combination,
                used_variables_from_dependencies: extra_vars_per_output.clone(),
                stage_0_render: r.clone(),
                order: (0..r.rendered_outputs.len()).collect(),
            }
            .sort_outputs();

            stage_1_renders.push(stage_1);
        }
    }

    Ok(stage_1_renders)
}
