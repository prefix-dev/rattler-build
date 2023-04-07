use core::panic;
use std::collections::BTreeMap;

use minijinja::{self, value::Value, Environment};
use serde_yaml::Value as YamlValue;

use crate::metadata::RenderedRecipe;

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

mod functions {
    use std::str::FromStr;

    use minijinja::Error;

    use crate::render::pin::{Pin, PinExpression};

    pub fn compiler(lang: String) -> Result<String, Error> {
        // we translate the compiler into a YAML string
        Ok(format!("__COMPILER {}", lang.to_lowercase()))
    }

    pub fn pin_subpackage(
        name: String,
        kwargs: Option<minijinja::value::Value>,
    ) -> Result<String, Error> {
        // we translate the compiler into a YAML string
        let mut pin_subpackage = Pin {
            name,
            max_pin: None,
            min_pin: None,
            exact: false,
        };

        let pin_expr_from_value = |pin_expr: &minijinja::value::Value| {
            PinExpression::from_str(&pin_expr.to_string()).map_err(|e| {
                Error::new(
                    minijinja::ErrorKind::SyntaxError,
                    format!("Invalid pin expression: {}", e),
                )
            })
        };

        if let Some(kwargs) = kwargs {
            let max_pin = kwargs.get_attr("max_pin")?;
            if max_pin != minijinja::value::Value::UNDEFINED {
                let pin_expr = pin_expr_from_value(&max_pin)?;
                pin_subpackage.max_pin = Some(pin_expr);
            }
            let min = kwargs.get_attr("min_pin")?;
            if min != minijinja::value::Value::UNDEFINED {
                let pin_expr = pin_expr_from_value(&min)?;
                pin_subpackage.min_pin = Some(pin_expr);
            }
            let exact = kwargs.get_attr("exact")?;
            if exact != minijinja::value::Value::UNDEFINED {
                pin_subpackage.exact = exact.is_true();
            }
        }

        Ok(format!(
            "__PIN_SUBPACKAGE {}",
            pin_subpackage.internal_repr()
        ))
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
    let env = Environment::new();
    for (key, v) in yaml_context.iter() {
        if let YamlValue::String(key) = key {
            let rendered = env.render_str(v.as_str().unwrap(), &context).unwrap();
            context.insert(key.to_string(), Value::from_safe_string(rendered));
        }
    }
    context
}

fn render_dependencies(
    recipe: &serde_yaml::Mapping,
    context: &BTreeMap<String, Value>,
) -> serde_yaml::Mapping {
    let mut recipe = recipe.clone();

    if let Some(requirements) = recipe.get_mut("requirements") {
        ["build", "host", "run"].iter().for_each(|section| {
            if let Some(YamlValue::Sequence(section)) = requirements.get_mut(section) {
                for item in section {
                    if let YamlValue::String(item) = item {
                        if context.contains_key(item) {
                            let pin = context.get(item).unwrap().as_str().unwrap().to_string();
                            *item = format!("{} {}", item, pin);
                        }
                    }
                }
            }
        });
    }

    recipe
}

/// This renders a recipe, given a variant
/// This evaluates all selectors and jinja statements in the recipe
/// It does _not_ apply the variants to the dependency list yet
pub fn render_recipe(
    recipe: &YamlValue,
    variant: &BTreeMap<String, String>,
) -> anyhow::Result<RenderedRecipe> {
    let recipe = match recipe {
        YamlValue::Mapping(map) => map,
        _ => panic!("Expected a map"),
    };

    let mut env = Environment::new();
    env.add_function("compiler", functions::compiler);
    env.add_function("pin_subpackage", functions::pin_subpackage);
    let (mut recipe_modified, context) =
        if let Some(YamlValue::Mapping(map)) = &recipe.get("context") {
            let mut context = render_context(map);
            let mut recipe_modified = recipe.clone();
            recipe_modified.remove("context");

            // TODO add more appropriate values here
            context.insert("PYTHON".to_string(), "python".into());

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
    recipe_modified = render_dependencies(&recipe_modified, &context);
    Ok(serde_yaml::from_value(YamlValue::from(recipe_modified))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_context() {
        let context = r#"
        name: "foo"
        version: "1.0"
        version_name: "{{ name }}-{{ version }}"
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
        build:
            - name: "{{ name }}-{{ version }}"
              version: "{{ version }}"
              url: "https://example.com/{{ name }}-{{ version }}.tar.gz"
              sha256: "1234567890"
              patches:
                - url: "https://example.com/{{ name }}-{{ version }}.patch"
                  sha256: "1234567890"
        "#;
        let recipe = serde_yaml::from_str(recipe).unwrap();
        let recipe = render_recipe(&recipe, &BTreeMap::new()).unwrap();
        insta::assert_yaml_snapshot!(recipe);
    }
}
