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

pub fn render_recipe(recipe: &YamlValue) -> serde_yaml::Mapping {
    let recipe = match recipe {
        YamlValue::Mapping(map) => map,
        _ => panic!("Expected a map"),
    };

    let mut env = Environment::new();
    env.add_function("compiler", functions::compiler);
    let mut context = HashMap::<String, Value>::new();

    if let Some(YamlValue::Mapping(map)) = &recipe.get("context") {
        for (key, v) in map.iter() {
            if let YamlValue::String(key) = key {
                context.insert(
                    key.to_string(),
                    Value::from_safe_string(v.as_str().unwrap().to_string()),
                );
            }
        }
        let mut recipe_modified = recipe.clone();
        recipe_modified.remove("context");
        render_recipe_recursively(&mut recipe_modified, &env, &context);
        recipe_modified
    } else {
        eprintln!("Did not find context");
        recipe.clone()
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
        let recipe = render_recipe(&recipe);
        insta::assert_yaml_snapshot!(recipe);
    }
}
