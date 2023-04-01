use core::panic;
use std::collections::BTreeMap;

use minijinja::{self, value::Value, Environment};
use serde_yaml::Value as YamlValue;

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
    use std::collections::BTreeMap;

    use minijinja::Error;

    pub fn compiler(
        lang: String,
        variant: &BTreeMap<String, minijinja::value::Value>,
    ) -> Result<String, Error> {
        let (ckey, cver) = (
            format!("{}_compiler", lang),
            format!("{}_compiler_version", lang),
        );

        if let Some(compiler) = variant.get(&ckey) {
            if let Some(version) = variant.get(&cver) {
                return Ok(format!(
                    "{}_{} {}*",
                    compiler,
                    variant.get("target_platform").unwrap(),
                    version
                ));
            }
        }

        Ok(format!("{}-compiler", lang))
    }
}

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

pub fn render_recipe(
    recipe: &YamlValue,
    variant: BTreeMap<String, String>,
) -> anyhow::Result<serde_yaml::Mapping> {
    let recipe = match recipe {
        YamlValue::Mapping(map) => map,
        _ => panic!("Expected a map"),
    };

    let mut env = Environment::new();

    if let Some(YamlValue::Mapping(map)) = &recipe.get("context") {
        let mut context = render_context(map);
        let mut recipe_modified = recipe.clone();
        recipe_modified.remove("context");

        // TODO add more appropriate values here
        context.insert("PYTHON".to_string(), "python".into());

        // add in the variant
        for (key, value) in &variant {
            context.insert(key.clone(), Value::from_safe_string(value.clone()));
        }

        context.insert(
            "target_platform".to_string(),
            "osx-arm64".to_string().into(),
        );
        let context_cloned = context.clone();
        env.add_function("compiler", move |lang| {
            functions::compiler(lang, &context_cloned)
        });

        render_recipe_recursively(&mut recipe_modified, &env, &context);
        recipe_modified = render_dependencies(&recipe_modified, &context);
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
        let recipe = render_recipe(&recipe, BTreeMap::new()).unwrap();
        insta::assert_yaml_snapshot!(recipe);
    }
}
