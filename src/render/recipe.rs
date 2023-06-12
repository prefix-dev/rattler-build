use std::collections::BTreeMap;
use std::str::FromStr;

use minijinja::{self, value::Value, Environment};
use rattler_conda_types::Version;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use serde_yaml::Value as YamlValue;
use crate::metadata;

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

    let mut env = Environment::new();
    env.add_function("compiler", functions::compiler);
    env.add_function("pin_subpackage", functions::pin_subpackage);

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

#[derive(Deserialize)]
pub struct Recipe {
    pub package: Config<Package>,
}

#[derive(Deserialize)]
pub struct Package {
    pub name: Config<String>,
    pub version: Config<String>,
}

/// A trait that is implemented for types that can be rendered using minijinja.
trait Renderable {
    /// The type after rendering
    type Target;

    /// The error type that is returned in case of problems.
    type Error;

    /// Evaluates this instance using the specified environment.
    fn render<S: Serialize>(&self, environment: &minijinja::Environment, ctx: &S) -> Result<Self::Target, Self::Error>;
}

impl Renderable for String {
    type Target = String;
    type Error = minijinja::Error;

    fn render<S: Serialize>(&self, environment: &minijinja::Environment, ctx: &S) -> Result<Self::Target, Self::Error> {
        environment.render_str(&self, ctx)
    }
}

impl Renderable for Package {
    type Target = metadata::Package;
    type Error = ControlFlowError;

    fn render<S: Serialize>(&self, environment: &minijinja::Environment, ctx: &S) -> Result<Self::Target, Self::Error> {
        Ok(metadata::Package {
            name: self.name.render(environment, ctx)?,
            version: self.version.render(environment, ctx)?,
        })
    }
}

/// A configurable value
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Config<T: Configurable>(T::DeserializableValue);

impl<T: Configurable> Renderable for Config<T>
    where
        T::DeserializableValue: Renderable
{
    type Target = <T::DeserializableValue as Renderable>::Target;
    type Error = <T::DeserializableValue as Renderable>::Error;

    fn render<S: Serialize>(&self, environment: &minijinja::Environment, ctx: &S) -> Result<Self::Target, Self::Error> {
        self.0.render(environment, ctx)
    }
}

pub trait Configurable {
    /// The type that is read from the recipe.
    type DeserializableValue: DeserializeOwned;
}

impl Configurable for String { type DeserializableValue = ControlFlowValue<String>; }

#[derive(Deserialize)]
#[serde(untagged)]
pub enum ControlFlowValue<T> {
    Expression(T),
    Selectors(Vec<IfStatement<T>>),
}

impl<T: DeserializeOwned + Renderable> Renderable for ControlFlowValue<T>
    where
        T::Error: Into<ControlFlowError>
{
    type Target = T::Target;
    type Error = ControlFlowError;

    fn render<S: Serialize>(&self, environment: &Environment, ctx: &S) -> Result<Self::Target, Self::Error> {
        let expr = match self {
            ControlFlowValue::Expression(expr) => expr,
            ControlFlowValue::Selectors(branches) => {
                let mut selected_value = None;
                for branch in branches {
                    dbg!(&branch.if_expr);

                    let rendered = environment.render_str(&format!("{{{{ {} }}}}", branch.if_expr), ctx)?;
                    dbg!(&rendered);

                    if Value::from_safe_string(rendered).is_true() {
                        match selected_value {
                            Some(_) => return Err(ControlFlowError::MultipleSelectors),
                            None => {
                                selected_value = Some(&branch.value);
                            }
                        }
                    }
                }

                match selected_value {
                    Some(expr) => expr,
                    None => return Err(ControlFlowError::EmptyBranchError),
                }
            }
        };

        expr.render(environment, ctx).map_err(Into::into)
    }
}

/// An error that can occur during rendering of a
#[derive(thiserror::Error, Debug)]
enum ControlFlowError {
    #[error(transparent)]
    MiniJinjaError(#[from] minijinja::Error),

    #[error("none of the selectors match")]
    EmptyBranchError,

    #[error("multiple matching selectors")]
    MultipleSelectors,
}

#[derive(Deserialize)]
pub struct IfStatement<T> {
    #[serde(rename = "if")]
    if_expr: String,
    value: T,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    #[test]
    fn test_renderable() {
        let recipe = r#"
        name: "{{ name }}"
        version:
        - if: win
          value: "1.0"
        - if: osx
          value: "2.0"
        "#;

        let package: Package = serde_yaml::from_str(recipe).unwrap();

        let context = HashMap::from([
            ("name", "foo"),
            ("win", "true"),
        ]);
        let environment = Environment::new();
        let rendered_package = package.render(&environment, &context).unwrap();

        insta::assert_yaml_snapshot!(rendered_package);
    }

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
        source:
            - url: "https://example.com/{{ name }}-{{ version }}.tar.gz"
              sha256: "1234567890"
              patches:
            - url: "https://example.com/{{ name }}-{{ version }}.patch"
              sha256: "1234567890"
        package:
            name: "{{ name }}-{{ version }}"
            version: "{{ version }}"
        requirements:
        about:
        "#;
        let recipe = serde_yaml::from_str(recipe).unwrap();
        let recipe = render_recipe(&recipe, &BTreeMap::new(), "h12341234");
        assert!(recipe.is_ok());
        insta::assert_yaml_snapshot!(recipe.expect("could not render recipe"));
    }
}
