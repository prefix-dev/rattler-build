use core::panic;
use std::collections::HashMap;

use minijinja::{self, value::Value, Environment};
use serde_yaml::Value as YamlValue;

fn render_recipe_recursively(
    recipe: &mut serde_yaml::Mapping,
    jinja_env: &Environment,
    context: &HashMap<String, Value>,
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
    context: &HashMap<String, Value>,
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
    use minijinja::Error;

    pub fn compiler(lang: String) -> Result<String, Error> {
        Ok(format!("{}-compiler", lang))
    }
}

fn render_context(yaml_context: &serde_yaml::Mapping) -> HashMap<String, Value> {
    let mut context = HashMap::<String, Value>::new();
    for (key, v) in yaml_context.iter() {
        if let YamlValue::String(key) = key {
            // TODO actually render the value with minijinja and known values
            context.insert(
                key.to_string(),
                Value::from_safe_string(v.as_str().unwrap().to_string()),
            );
        }
    }

    // TODO add more appropriate values here
    context.insert("PYTHON".to_string(), "python".into());

    context
}

pub fn render_recipe(recipe: &YamlValue) -> anyhow::Result<serde_yaml::Mapping> {
    let recipe = match recipe {
        YamlValue::Mapping(map) => map,
        _ => panic!("Expected a map"),
    };

    let mut env = Environment::new();
    env.add_function("compiler", functions::compiler);
    if let Some(YamlValue::Mapping(map)) = &recipe.get("context") {
        let context = render_context(map);
        let mut recipe_modified = recipe.clone();
        recipe_modified.remove("context");
        render_recipe_recursively(&mut recipe_modified, &env, &context);
        Ok(recipe_modified)
    } else {
        tracing::info!("Did not find context");
        Ok(recipe.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let recipe = render_recipe(&recipe).unwrap();
        insta::assert_yaml_snapshot!(recipe);
    }
}
