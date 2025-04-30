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
    recipe::{
        Jinja, ParsingError, Recipe,
        custom_yaml::Node,
        parser::{BuildString, Dependency},
        variable::Variable,
    },
    selectors::SelectorConfig,
    source_code::SourceCode,
    used_variables::used_vars_from_expressions,
    variant_config::{
        ParseErrors, VariantConfig, VariantConfigError, VariantError, VariantExpandError,
    },
};

/// All the raw outputs of a single recipe.yaml
#[derive(Clone, Debug)]
pub struct RawOutputVec {
    /// The raw node (slightly preprocessed by making sure all keys are there)
    pub vec: Vec<Node>,

    /// The used variables in each of the nodes
    pub used_vars_jinja: Vec<HashSet<NormalizedKey>>,
}

/// Stage 0 render of a single recipe.yaml
#[derive(Clone, Debug)]
pub struct Stage0Render<S: SourceCode> {
    /// The used variables with their values
    pub variables: BTreeMap<NormalizedKey, Variable>,

    /// The raw outputs of the recipe
    pub raw_outputs: RawOutputVec,

    /// Pre-rendered recipe nodes
    pub rendered_outputs: Vec<Recipe>,

    /// The source code of the recipe
    pub source: S,
}

pub(crate) fn stage_0_render<S: SourceCode>(
    outputs: &[Node],
    source: S,
    selector_config: &SelectorConfig,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage0Render<S>>, VariantError<S>> {
    let used_vars = outputs
        .iter()
        .map(|output| {
            used_vars_from_expressions(output, source.clone())
                .map(|x| x.into_iter().map(Into::into).collect())
        })
        .collect::<Result<_, _>>()
        .map_err(|errs| {
            let errs: ParseErrors<S> = errs.into();
            VariantConfigError::RecipeParseErrors(errs)
        })?;

    let raw_output_vec = RawOutputVec {
        vec: outputs.to_vec(),
        used_vars_jinja: used_vars,
    };

    // find all the jinja variables from all the expressions
    let mut used_vars = HashSet::<NormalizedKey>::new();
    for output in outputs {
        used_vars.extend(
            used_vars_from_expressions(output, source.clone())
                .map_err(|errs| {
                    let errs: ParseErrors<_> = errs.into();
                    VariantConfigError::RecipeParseErrors(errs)
                })?
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

            let parsed_recipe = Recipe::from_node(output, config_with_variant)
                .map_err(|err| {
                    let errs: ParseErrors<_> = err
                        .into_iter()
                        .map(|err| ParsingError::from_partial(source.clone(), err))
                        .collect::<Vec<ParsingError<_>>>()
                        .into();
                    errs
                })
                .map_err(VariantConfigError::from)?;

            rendered_outputs.push(parsed_recipe);
        }

        stage0_renders.push(Stage0Render {
            variables: combination,
            raw_outputs: raw_output_vec.clone(),
            rendered_outputs,
            source: source.clone(),
        });
    }

    Ok(stage0_renders)
}

#[derive(Debug, Clone)]
pub struct Stage1Inner {
    pub(crate) used_vars_from_dependencies: HashSet<NormalizedKey>,
    pub(crate) exact_pins: HashSet<PackageName>,
    pub(crate) recipe: Recipe,
    pub(crate) selector_config: SelectorConfig,
}

/// Stage 1 render of a single recipe.yaml
#[derive(Debug)]
pub struct Stage1Render<S: SourceCode> {
    pub(crate) variables: BTreeMap<NormalizedKey, Variable>,

    pub(crate) inner: Vec<Stage1Inner>,

    pub(crate) stage_0_render: Stage0Render<S>,
}

impl<S: SourceCode> Stage1Render<S> {
    pub fn index_from_name(&self, package_name: &PackageName) -> Option<usize> {
        self.inner
            .iter()
            .position(|x| x.recipe.package().name() == package_name)
    }

    pub fn variant_for_output(
        &self,
        idx: usize,
    ) -> Result<BTreeMap<NormalizedKey, Variable>, VariantExpandError> {
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

        // Add in virtual packages
        let recipe = &self.inner[idx].recipe;
        for run_requirement in recipe.requirements().run() {
            if let Dependency::Spec(spec) = run_requirement {
                if let Some(ref name) = spec.name {
                    if name.as_normalized().starts_with("__") {
                        variant.insert(name.as_normalized().into(), spec.to_string().into());
                    }
                }
            }
        }

        for pin in &inner.exact_pins {
            if pin == self_name {
                continue;
            }
            let Some(other_idx) = self.index_from_name(pin) else {
                return Err(VariantExpandError::MissingOutput(
                    pin.as_source().to_string(),
                ));
            };
            // find the referenced output
            let build_string = self.build_string_for_output(other_idx)?;
            let version = self.inner[other_idx].recipe.package().version();
            variant.insert(
                pin.as_normalized().into(),
                format!("{} {}", version, build_string).into(),
            );
        }

        // fix target_platform value here
        if !recipe.build().noarch().is_none() {
            variant.insert("target_platform".into(), "noarch".into());
        }

        Ok(variant)
    }

    pub fn build_string_for_output(&self, idx: usize) -> Result<String, VariantExpandError> {
        let variant = self.variant_for_output(idx)?;
        let recipe = &self.stage_0_render.rendered_outputs[idx];
        let hash = HashInfo::from_variant(&variant, recipe.build().noarch());
        let inner = &self.inner[idx];

        let mut selector_config = inner.selector_config.clone();
        selector_config.hash = Some(hash.clone());
        let jinja = Jinja::new(selector_config.clone()).with_context(&recipe.context);

        Ok(recipe
            .build()
            .string()
            .resolve(&hash, recipe.build().number, &jinja)
            .into_owned())
    }

    /// sort the outputs topologically
    pub fn sorted_indices(&self) -> Result<Vec<usize>, VariantExpandError> {
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
        let sorted_indices = match petgraph::algo::toposort(&graph, None) {
            Ok(sorted_indices) => sorted_indices,
            Err(cycle) => {
                let cycle = cycle.node_id();
                let cycle_name = graph[cycle].package().name();
                return Err(VariantExpandError::CycleInRecipeOutputs(
                    cycle_name.as_source().to_string(),
                ));
            }
        };

        let sorted_indices = sorted_indices
            .into_iter()
            .map(|x| x.index())
            .collect::<Vec<usize>>();

        // Update the order of the outputs
        Ok(sorted_indices)
    }

    #[allow(clippy::type_complexity)]
    pub fn into_sorted_outputs(
        self,
    ) -> Result<Vec<((Node, Recipe), BTreeMap<NormalizedKey, Variable>)>, VariantExpandError> {
        // zip node from stage0 and final render output
        let sorted_indices = self.sorted_indices()?;

        let raw_nodes = self.stage_0_render.raw_outputs.vec.clone().into_iter();
        let outputs = self.inner.clone().into_iter().map(|i| i.recipe);

        let zipped = raw_nodes.zip(outputs).collect::<Vec<_>>();
        let mut result = Vec::new();
        for idx in sorted_indices {
            let mut recipe = zipped[idx].clone();
            let build_string = self.build_string_for_output(idx)?;
            // Resolve the build string and store the resolved one in the recipe
            recipe.1.build.string = BuildString::Resolved(build_string);
            let variant = self.variant_for_output(idx)?;
            result.push((recipe, variant));
        }

        Ok(result)
    }
}

/// Render the stage 1 of the recipe by adding in variants from the dependencies
pub(crate) fn stage_1_render<S: SourceCode>(
    stage0_renders: Vec<Stage0Render<S>>,
    selector_config: &SelectorConfig,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage1Render<S>>, VariantError<S>> {
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

            // We want to add something to packages that are requiring a subpackage
            // _exactly_ because that creates additional variants
            for pin in output.requirements.all_pin_subpackage() {
                if pin.args.exact {
                    let name = pin.name.clone().as_normalized().to_string();
                    additional_variables.insert(name.into());
                    exact_pins.insert(pin.name.clone());
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

            // If the recipe is `noarch: python` we can remove an empty python key that
            // comes from the dependencies
            if output.build().noarch().is_python() {
                additional_variables.remove(&"python".into());
            }

            // special handling of CONDA_BUILD_SYSROOT
            let jinja_variables = &r.raw_outputs.used_vars_jinja[idx];
            if jinja_variables.contains(&"c_compiler".into())
                || jinja_variables.contains(&"cxx_compiler".into())
            {
                additional_variables.insert("CONDA_BUILD_SYSROOT".into());
            }

            // also always add `target_platform`, `channel_sources` and `channel_targets`
            additional_variables.insert("target_platform".into());
            additional_variables.insert("channel_sources".into());
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

        // Create the additional combinations and attach the whole variant x outputs to
        // the stage 1 render
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

                let parsed_recipe = Recipe::from_node(output, config_with_variant.clone())
                    .map_err(|err| {
                        let errs: ParseErrors<_> = err
                            .into_iter()
                            .map(|err| ParsingError::from_partial(r.source.clone(), err))
                            .collect::<Vec<_>>()
                            .into();
                        errs
                    })
                    .map_err(VariantConfigError::from)?;

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
            };

            stage_1_renders.push(stage_1);
        }
    }

    Ok(stage_1_renders)
}
