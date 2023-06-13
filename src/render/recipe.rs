use futures::StreamExt;
use itertools::Either;
use std::collections::BTreeMap;
use std::error::Error;
use std::str::FromStr;

use crate::metadata;
use minijinja::{self, value::Value, Environment};
use rattler_conda_types::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::formats::{Format, PreferOne};
use serde_with::{serde_as, OneOrMany};
use serde_yaml::Value as YamlValue;
use url::Url;

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

#[derive(Deserialize, Serialize)]
pub struct Recipe {
    #[serde(default)]
    pub context: BTreeMap<String, String>,

    /// Information about the package
    pub package: Package,

    /// The source section of the recipe
    #[serde(default, skip_serializing_if = "ListExpr::is_empty")]
    pub source: ListExpr<Source>,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct About {
    #[serde(default, skip_serializing_if = "ListExpr::is_empty")]
    pub home: ListExpr<Url>,
    pub license: Option<Expr<String>>,
    #[serde(default, skip_serializing_if = "ListExpr::is_empty")]
    pub license_file: ListExpr<Url>,
    pub license_family: Option<Expr<String>>,
    pub summary: Option<Expr<String>>,
    pub description: Option<Expr<String>>,
    #[serde(default, skip_serializing_if = "ListExpr::is_empty")]
    pub doc_url: ListExpr<Url>,
    #[serde(default, skip_serializing_if = "ListExpr::is_empty")]
    pub dev_url: ListExpr<Url>,
}

impl Recipe {
    pub fn render(
        &self,
        variant: &BTreeMap<String, String>,
        pkg_hash: &str,
    ) -> Result<RenderedRecipe, RenderError> {
        // Construct an environment for the recipe
        let mut env = Environment::new();
        env.add_function("compiler", functions::compiler);
        env.add_function("pin_subpackage", functions::pin_subpackage);

        // Build the context based on variables defined in the recipe.
        let mut context = BTreeMap::<String, Value>::new();
        for (key, v) in self.context.iter() {
            let rendered = env
                .render_str(v, &context)
                .map_err(RenderError::MiniJinjaError)?;
            context.insert(key.clone(), Value::from_safe_string(rendered));
        }

        // Add the PKG_HASH to the context
        context.insert("PKG_HASH".to_string(), pkg_hash.into());

        // Add the variants to the context
        for (key, value) in variant {
            context.insert(key.clone(), Value::from_safe_string(value.clone()));
        }

        Ok(RenderedRecipe {
            package: self.package.render(&env, &context)?,
            source: None,
            build: metadata::BuildOptions::default(),
            requirements: Default::default(),
            about: Default::default(),
            test: None,
        })
    }
}

#[derive(Deserialize, Serialize)]
pub struct Package {
    /// The name of the package
    pub name: Expr<String>,

    /// The version of the package
    pub version: Expr<Version>,
}

impl Package {
    fn render<S: Serialize>(
        &self,
        env: &Environment,
        ctx: &S,
    ) -> Result<metadata::Package, RenderError> {
        Ok(metadata::Package {
            name: self.name.render(env, ctx)?,
            version: self.version.render(env, ctx)?,
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Source {
    // Git(GitSrc),
    Url(UrlSrc),
    // Path(PathSrc),
}

/// A url source (usually a tar.gz or tar.bz2 archive). A compressed file
/// will be extracted to the `work` (or `work/<folder>` directory).
#[derive(Serialize, Deserialize, Clone)]
pub struct UrlSrc {
    /// Url to the source code (usually a tar.gz or tar.bz2 etc. file)
    pub url: Expr<Url>,
    // /// Optionally a checksum to verify the downloaded file
    // #[serde(flatten)]
    // pub checksum: Checksum,
    //
    // /// Patches to apply to the source code
    // pub patches: Option<Vec<PathBuf>>,
    //
    // /// Optionally a folder name under the `work` directory to place the source code
    // pub folder: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    MiniJinjaError(#[from] minijinja::Error),

    /// An error that happened when converting from a jinja value.
    #[error(transparent)]
    FromStr(Box<dyn std::error::Error>),
}

/// A dynamic value in the yaml file.
#[derive(Deserialize, Serialize, Clone, Default)]
#[serde(transparent)]
pub struct Expr<T: ExprType>(T::Value);

pub trait ExprType: Sized {
    type Value: DeserializeOwned;

    fn render<S: Serialize>(
        value: &Self::Value,
        environment: &Environment,
        ctx: &S,
    ) -> Result<Self, RenderError>;
}

impl ExprType for String {
    type Value = StringExpr;

    fn render<S: Serialize>(
        value: &Self::Value,
        environment: &Environment,
        ctx: &S,
    ) -> Result<Self, RenderError> {
        value.render(environment, ctx)
    }
}

impl ExprType for Version {
    type Value = StringExpr;

    fn render<S: Serialize>(
        value: &Self::Value,
        environment: &Environment,
        ctx: &S,
    ) -> Result<Self, RenderError> {
        value.render(environment, ctx)
    }
}

impl ExprType for Url {
    type Value = StringExpr;

    fn render<S: Serialize>(
        value: &Self::Value,
        environment: &Environment,
        ctx: &S,
    ) -> Result<Self, RenderError> {
        value.render(environment, ctx)
    }
}

impl<T: ExprType> Expr<T> {
    fn render<S: Serialize>(&self, environment: &Environment, ctx: &S) -> Result<T, RenderError> {
        T::render(&self.0, environment, ctx)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(transparent)]
pub struct StringExpr(String);

impl StringExpr {
    fn render<S: Serialize, T: FromStr>(
        &self,
        environment: &Environment,
        ctx: &S,
    ) -> Result<Option<T>, RenderError>
    where
        <T as FromStr>::Err: Into<Box<dyn Error>>,
    {
        let str_repr = environment
            .render_str(&self.0, ctx)
            .map_err(RenderError::MiniJinjaError)?;
        if str_repr.trim().is_empty() {
            Ok(None)
        } else {
            T::from_str(&str_repr)
                .map_err(Into::into)
                .map_err(RenderError::FromStr)
        }
    }
}

#[serde_as]
#[derive(Deserialize, Serialize)]
#[serde(
    transparent,
    bound(
        deserialize = "T: serde::Deserialize<'de>",
        serialize = "T: serde::Serialize"
    )
)]
pub struct ListExpr<T>(#[serde_as(as = "OneOrMany<_, PreferOne>")] pub Vec<ListExprEntry<T>>);

impl<T> Default for ListExpr<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> ListExpr<T> {
    pub fn eval<S: Serialize>(
        &self,
        environment: &Environment,
        ctx: &S,
    ) -> Result<Vec<T>, RenderError> {
        let mut result = Vec::new();
        Ok(result)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum ListExprEntry<T> {
    ControlFlow(ControlFlowBlock<T>),
    Single(T),
}

impl<T: Clone> ListExprEntry<T> {
    pub fn eval<S: Serialize>(
        &self,
        environment: &Environment,
        ctx: &S,
    ) -> Result<impl Iterator<Item = &'_ T> + '_, RenderError> {
        match self {
            ListExprEntry::ControlFlow(ctrl) => {
                Ok(Either::Left(ctrl.eval(environment, ctx)?.into_iter()))
            }
            ListExprEntry::Single(value) => Ok(Either::Right(std::iter::once(value))),
        }
    }
}

#[serde_as]
#[derive(Deserialize, Serialize)]
#[serde(bound(
    deserialize = "T: serde::Deserialize<'de>",
    serialize = "T: serde::Serialize"
))]
pub struct ControlFlowBlock<T> {
    #[serde(rename = "if")]
    pub expr: String,
    #[serde_as(as = "OneOrMany<_, PreferOne>")]
    pub then: Vec<T>,
    #[serde(
        default = "Default::default",
        rename = "else",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[serde_as(as = "OneOrMany<_, PreferOne>")]
    pub otherwise: Vec<T>,
}

impl<T: Clone> ControlFlowBlock<T> {
    /// Evaluates the `if` part of the block and returns either the `then` result or the `else`.
    pub fn eval<S: Serialize>(
        &self,
        environment: &Environment,
        ctx: &S,
    ) -> Result<&[T], RenderError> {
        let expr = environment
            .compile_expression(&self.expr)
            .map_err(RenderError::MiniJinjaError)?;
        let evaluated_expr = expr.eval(ctx).map_err(RenderError::MiniJinjaError)?;
        if evaluated_expr.is_true() {
            Ok(&self.then)
        } else {
            Ok(&self.otherwise)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_renderable() {
        let recipe = r#"
        package:
            name: "{{ name }}"
            version: "1.9.2"

        context:
          name: xtensor
          version: "0.24.6"

        package:
          name: "{{ name|lower }}"
          version: "{{ version }}"

        source:
            - if: win
              then:
              - url: "blabla"
            - if: unix
              then:
                url: "ok"
        "#;

        let recipe: Recipe = serde_yaml::from_str(recipe).unwrap();
        let rendered_recipe = recipe.render(&Default::default(), "bla").unwrap();

        insta::assert_yaml_snapshot!(rendered_recipe);
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
