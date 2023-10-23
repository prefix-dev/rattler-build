//! Second and final stage of the recipe parser pipeline.
//!
//! This stage takes the [`RawRecipe`] from the first stage and parses it into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.
use minijinja::Value;
use serde::Serialize;

use crate::{
    _error, _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, ScalarNode, TryConvertNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        jinja::Jinja,
        Render,
    },
    selectors::SelectorConfig,
};

mod about;
mod build;
mod package;
mod requirements;
mod source;
mod test;

pub use self::{
    about::About,
    build::{Build, RunExports, ScriptEnv},
    package::Package,
    requirements::{Compiler, Dependency, PinSubpackage, Requirements},
    source::{Checksum, GitSource, GitUrl, PathSource, Source, UrlSource},
    test::Test,
};

use super::{custom_yaml::Node, error::marker_span_to_span};

/// A recipe that has been parsed and validated.
#[derive(Debug, Clone, Serialize)]
pub struct Recipe {
    package: Package,
    source: Vec<Source>,
    build: Build,
    requirements: Requirements,
    test: Test,
    about: About,
    extra: (),
}

impl Recipe {
    /// Build a recipe from a YAML string.
    pub fn from_yaml(yaml: &str, jinja_opt: SelectorConfig) -> Result<Self, ParsingError> {
        let yaml_root = marked_yaml::parse_yaml(0, yaml)
            .map_err(|err| super::error::load_error_handler(yaml, err))?;

        let yaml_root = Node::try_from(yaml_root)
            .map_err(|err| _error!(yaml, marker_span_to_span(yaml, err.span), err.kind))?;

        Self::from_node(&yaml_root, jinja_opt)
            .map(|mut v| v.remove(0)) // TODO: handle multiple recipe outputs
            .map_err(|err| ParsingError::from_partial(yaml, err))
    }

    /// Build a recipe from a YAML string and use a given package hash string as default value.
    pub fn from_yaml_with_default_hash_str(
        yaml: &str,
        default_pkg_hash: &str,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, ParsingError> {
        let mut recipe = Self::from_yaml(yaml, jinja_opt)?;

        // Set the build string to the package hash if it is not set
        if recipe.build.string.is_none() {
            recipe.build.string = Some(format!("{}_{}", default_pkg_hash, recipe.build.number));
        }

        Ok(recipe)
    }

    /// WIP
    pub fn from_node(
        root_node: &Node,
        jinja_opt: SelectorConfig,
    ) -> Result<Vec<Self>, PartialParsingError> {
        let mut jinja = Jinja::new(jinja_opt);

        let root_node = root_node.as_mapping().ok_or_else(|| {
            _partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                label = "expected mapping"
            )
        })?;

        // add context values
        if let Some(context) = root_node.get("context") {
            let context = context.as_mapping().ok_or_else(|| {
                _partialerror!(
                    *context.span(),
                    ErrorKind::ExpectedMapping,
                    help = "`context` must always be a mapping"
                )
            })?;

            for (k, v) in context.iter() {
                let val = v.as_scalar().ok_or_else(|| {
                    _partialerror!(
                        *v.span(),
                        ErrorKind::ExpectedScalar,
                        help = "`context` values must always be scalars"
                    )
                })?;
                let rendered: Option<ScalarNode> =
                    val.render(&jinja, &format!("context.{}", k.as_str()))?;

                if let Some(rendered) = rendered {
                    jinja.context_mut().insert(
                        k.as_str().to_owned(),
                        Value::from_safe_string(rendered.as_str().to_string()),
                    );
                }
            }
        }

        let rendered_node: RenderedMappingNode = root_node.render(&jinja, "root")?;

        // TODO: handle outputs to produce multiple recipes

        let mut package = None;
        let mut build = Build::default();
        let mut source = Vec::new();
        let mut requirements = Requirements::default();
        let mut test = Test::default();
        let mut about = About::default();

        for (key, value) in rendered_node.iter() {
            match key.as_str() {
                "package" => package = Some(value.try_convert("package")?),
                "source" => source = value.try_convert("source")?,
                "build" => build = value.try_convert("build")?,
                "requirements" => requirements = value.try_convert("requirements")?,
                "test" => test = value.try_convert("test")?,
                "about" => about = value.try_convert("about")?,
                "outputs" => {}
                "context" => {}
                "extra" => {}
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_string().into()),
                    ))
                }
            }
        }

        let recipe = Recipe {
            package: package.ok_or_else(|| {
                _partialerror!(
                    *root_node.span(),
                    ErrorKind::Other,
                    label = "missing required key `package`"
                )
            })?,
            build,
            source,
            requirements,
            test,
            about,
            extra: (),
        };

        Ok(vec![recipe])
    }

    /// Get the package information.
    pub const fn package(&self) -> &Package {
        &self.package
    }

    /// Get the source information.
    pub fn sources(&self) -> &[Source] {
        self.source.as_slice()
    }

    /// Get the build information.
    pub const fn build(&self) -> &Build {
        &self.build
    }

    /// Get the requirements information.
    pub const fn requirements(&self) -> &Requirements {
        &self.requirements
    }

    /// Get the test information.
    pub const fn test(&self) -> &Test {
        &self.test
    }

    /// Get the about information.
    pub const fn about(&self) -> &About {
        &self.about
    }
}

#[cfg(test)]
mod tests {
    use crate::assert_miette_snapshot;

    use super::*;

    #[test]
    fn it_works() {
        let recipe = include_str!("../../examples/xtensor/recipe.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        assert!(recipe.is_ok());
        insta::assert_debug_snapshot!(recipe.unwrap());
    }

    #[test]
    fn context_not_mapping() {
        let raw_recipe = r#"
        context: "not-mapping"

        package:
          name: test
          version: 0.1.0
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        assert!(recipe.is_err());

        let err = recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn context_value_not_scalar() {
        let raw_recipe = r#"
        context:
          key: ["not-scalar"]

        package:
            name: test
            version: 0.1.0
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        assert!(recipe.is_err());

        let err = recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }
}
