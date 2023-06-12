use std::collections::BTreeMap;

use minijinja::{self, value::Value, Environment};
use serde_yaml::Value as YamlValue;

use crate::metadata::RenderedRecipe;

use super::jinja::jinja_environment;

/// Given a YAML recipe, this renders all strings it encounters using Jinja
/// templating.
fn render_recipe_recursively(
    recipe: &mut serde_yaml::Mapping,
    jinja_env: &Environment,
    context: &BTreeMap<String, Value>,
) {
    for (_, v) in recipe.iter_mut() {
        match v {
            YamlValue::String(var) => {
                *v = YamlValue::String(jinja_env.render_str(var, context).unwrap());
            }
            YamlValue::Sequence(var) => {
                render_recipe_recursively_seq(var, jinja_env, context);
            }
            YamlValue::Mapping(var) => {
                render_recipe_recursively(var, jinja_env, context);
            }
            _ => {}
        }
    }
}

fn render_recipe_recursively_seq(
    recipe: &mut serde_yaml::Sequence,
    environment: &Environment,
    context: &BTreeMap<String, Value>,
) {
    for v in recipe {
        match v {
            YamlValue::String(var) => {
                *v = YamlValue::String(environment.render_str(var, context).unwrap());
            }
            YamlValue::Sequence(var) => {
                render_recipe_recursively_seq(var, environment, context);
            }
            YamlValue::Mapping(var) => {
                render_recipe_recursively(var, environment, context);
            }
            _ => {}
        }
    }
}

/// This iteratively renderes the "context" object of a recipe
/// Later values can reference earlier values in the context, for example
///
/// ```yaml
/// context:
///   version: "3.0"
///   version_min: "min_{{ version }}"
/// ```
fn render_context(yaml_context: &serde_yaml::Mapping) -> BTreeMap<String, Value> {
    let mut context = BTreeMap::<String, Value>::new();
    let env = jinja_environment();

    for (key, v) in yaml_context.iter() {
        if let YamlValue::String(key) = key {
            let rendered = env.render_str(v.as_str().unwrap(), &context).unwrap();
            context.insert(key.to_string(), Value::from_safe_string(rendered));
        }
    }
    context
}

#[derive(Debug, thiserror::Error)]
pub enum RecipeRenderError {
    #[error("Invalid YAML")]
    InvalidYaml(#[from] serde_yaml::Error),

    #[error(
        "Invalid recipe file format. The recipe file YAML does not follow regular recipe structure (map with build, requirements, outputs...)"
    )]
    YamlNotMapping,
}

/// This renders a recipe, given a variant
/// This evaluates all selectors and jinja statements in the recipe
/// It does _not_ apply the variants to the dependency list yet
pub fn render_recipe(
    recipe: &YamlValue,
    variant: &BTreeMap<String, String>,
    pkg_hash: &str,
) -> Result<RenderedRecipe, RecipeRenderError> {
    let recipe = match recipe {
        YamlValue::Mapping(map) => map,
        _ => return Err(RecipeRenderError::YamlNotMapping),
    };

    let env = jinja_environment();

    let (mut recipe_modified, context) =
        if let Some(YamlValue::Mapping(map)) = &recipe.get("context") {
            let mut context = render_context(map);
            let mut recipe_modified = recipe.clone();
            recipe_modified.remove("context");

            context.insert("PKG_HASH".to_string(), pkg_hash.into());
            // add in the variant
            for (key, value) in variant {
                context.insert(key.clone(), Value::from_safe_string(value.clone()));
            }
            (recipe_modified, context)
        } else {
            tracing::info!("Did not find context");
            (recipe.clone(), BTreeMap::new())
        };

    render_recipe_recursively(&mut recipe_modified, &env, &context);

    let mut recipe: RenderedRecipe = serde_yaml::from_value(YamlValue::from(recipe_modified))?;

    // Set the build string to the package hash if it is not set
    if recipe.build.string.is_none() {
        recipe.build.string = Some(format!("{}_{}", pkg_hash, recipe.build.number));
    }
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_context() {
        let context = r#"
        name: "foo"
        version: "1.0"
        version_name: "${{ name }}-${{ version }}"
        "#;
        let context = serde_yaml::from_str(context).unwrap();
        let context = render_context(&context);
        insta::assert_yaml_snapshot!(context);
    }

    #[test]
    fn test_render() {
        let recipe = r#"
        context:
            name: "foo"
            version: "1.0"
        source:
            - url: "https://example.com/${{ name }}-${{ version }}.tar.gz"
              sha256: "1234567890"
              patches:
            - url: "https://example.com/${{ name }}-${{ version }}.patch"
              sha256: "1234567890"
        package:
            name: ${{ name }}-${{ version }}
            version: ${{ version }}
        requirements:
        about:
        "#;
        let recipe = serde_yaml::from_str(recipe).unwrap();
        let recipe = render_recipe(&recipe, &BTreeMap::new(), "h12341234");
        assert!(recipe.is_ok());
        insta::assert_yaml_snapshot!(recipe.expect("could not render recipe"));
    }
}
