use std::collections::{BTreeMap, HashSet};

use crate::{
    recipe::{
        custom_yaml::Node,
        parser::{Dependency, PinCompatible, PinSubpackage},
        ParsingError, Recipe,
    },
    selectors::SelectorConfig,
    used_variables::used_vars_from_expressions,
    variant_config::{ParseErrors, VariantConfig, VariantError},
};

/// All the raw outputs of a single recipe.yaml
#[derive(Clone, Debug)]
struct RawOutputVec {
    /// The raw node (slightly preprocessed by making sure all keys are there)
    vec: Vec<Node>,

    /// The used variables in each of the nodes
    used_vars_jinja: Vec<HashSet<String>>,

    /// The recipe string
    recipe: String,
}

/// Stage 0 render of a single recipe.yaml
#[derive(Clone, Debug)]
pub(crate) struct Stage0Render {
    /// The used variables with their values
    variables: BTreeMap<String, String>,

    /// The raw outputs of the recipe
    raw_outputs: RawOutputVec,

    // Pre-rendered recipe nodes
    rendered_outputs: Vec<Recipe>,
}

pub(crate) fn stage_0_render(
    outputs: &[Node],
    recipe: &str,
    selector_config: &SelectorConfig,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage0Render>, VariantError> {
    let used_vars = outputs
        .iter()
        .map(|output| used_vars_from_expressions(output, recipe))
        .collect::<Result<Vec<HashSet<String>>, Vec<ParsingError>>>();

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
    let mut used_vars = HashSet::<String>::new();
    for output in outputs {
        used_vars.extend(used_vars_from_expressions(output, recipe).unwrap());
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

    println!("{:?}", stage0_renders);
    Ok(stage0_renders)
}

pub struct Stage1Render {
    variables: BTreeMap<String, String>,
    stage_0_render: Stage0Render,
}

/// Render the stage 1 of the recipe by adding in variants from the dependencies
pub(crate) fn stage_1_render(
    stage0_renders: Vec<Stage0Render>,
    variant_config: &VariantConfig,
) -> Result<Vec<Stage1Render>, VariantError> {
    let mut stage_1_renders = Vec::new();

    for r in stage0_renders {
        for output in r.rendered_outputs {
            println!("{:?}", output.build_time_requirements().collect::<Vec<_>>());
            let mut additional_variables = HashSet::<String>::new();
            // Add in variants from the dependencies as we find them
            for dep in output.build_time_requirements() {
                if let Dependency::Spec(spec) = dep {
                    let is_simple = spec.version.is_none() && spec.build.is_none();
                    // add in the variant key for this dependency that has no version specifier
                    if is_simple {
                        if let Some(ref name) = spec.name {
                            additional_variables.insert(name.as_normalized().to_string());
                        }
                    }
                }
            }

            // We wanna add something to packages that are requiring a subpackage _exactly_ because
            // that creates additional variants
            for req in output.requirements.all_requirements() {
                match req {
                    Dependency::PinSubpackage(pin) => {}
                    _ => {}
                }
            }

            // println!("{:?}", output.)
        }
    }

    Ok(stage_1_renders)
}
