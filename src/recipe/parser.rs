//! Second and final stage of the recipe parser pipeline.
//!
//! This phase parses YAML and [`SelectorConfig`] into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.
use minijinja::Value;
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
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
mod output;
mod package;
mod requirements;
mod source;
mod test;

pub use self::{
    about::About,
    build::{Build, RunExports, ScriptEnv},
    output::find_outputs_from_src,
    package::{OutputPackage, Package},
    requirements::{Compiler, Dependency, PinSubpackage, Requirements},
    source::{Checksum, GitSource, GitUrl, PathSource, Source, UrlSource},
    test::Test,
};

use super::custom_yaml::Node;

/// A recipe that has been parsed and validated.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let yaml_root = Node::parse_yaml(0, yaml)?;

        Self::from_node(&yaml_root, jinja_opt).map_err(|err| ParsingError::from_partial(yaml, err))
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

    /// Create recipes from a YAML [`Node`] structure.
    pub fn from_node(
        root_node: &Node,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, PartialParsingError> {
        let hash = jinja_opt.hash.clone();
        let mut jinja = Jinja::new(jinja_opt);

        let root_node = root_node
            .as_mapping()
            .ok_or_else(|| _partialerror!(*root_node.span(), ErrorKind::ExpectedMapping,))?;

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

        let rendered_node: RenderedMappingNode = root_node.render(&jinja, "ROOT")?;

        let mut package = None;
        let mut build = Build::default();
        let mut source = Vec::new();
        let mut requirements = Requirements::default();
        let mut test = Test::default();
        let mut about = About::default();

        for (key, value) in rendered_node.iter() {
            let key_str = key.as_str();
            match key_str {
                "package" => package = Some(value.try_convert(key_str)?),
                "recipe" => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField("recipe".to_string().into()),
                        help =
                            "The recipe field is only allowed in conjunction with multiple outputs"
                    ))
                }
                "source" => source = value.try_convert(key_str)?,
                "build" => build = value.try_convert(key_str)?,
                "requirements" => requirements = value.try_convert(key_str)?,
                "test" => test = value.try_convert(key_str)?,
                "about" => about = value.try_convert(key_str)?,
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

        // Add hash to build.string if it is not set
        if build.string.is_none() {
            if let Some(hash) = hash {
                build.string = Some(format!("{}_{}", hash, build.number));
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

        Ok(recipe)
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
    use insta::assert_yaml_snapshot;

    use crate::assert_miette_snapshot;

    use super::*;

    #[test]
    fn it_works() {
        let recipe = include_str!("../../examples/xtensor/recipe.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        assert!(recipe.is_ok());
        #[cfg(target_family = "unix")]
        insta::assert_debug_snapshot!(recipe.unwrap());
        #[cfg(target_family = "windows")]
        insta::assert_debug_snapshot!("recipe_windows", recipe.unwrap());
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
        let err = recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn jinja_error() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/recipe_jinja_error.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        let err = recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn jinja_sequence() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/recipe_inline_jinja.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        assert_yaml_snapshot!(recipe);
    }
}
