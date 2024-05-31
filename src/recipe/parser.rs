//! Second and final stage of the recipe parser pipeline.
//!
//! This phase parses YAML and [`SelectorConfig`] into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.
use std::borrow::Cow;

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
mod glob_vec;
mod helper;
mod output;
mod package;
mod regex;
mod requirements;
mod script;
mod skip;
mod source;
mod test;

pub use self::{
    about::About,
    build::{Build, DynamicLinking, PrefixDetection},
    glob_vec::GlobVec,
    output::find_outputs_from_src,
    package::{OutputPackage, Package},
    regex::SerializableRegex,
    requirements::{
        Dependency, IgnoreRunExports, Language, PinCompatible, PinSubpackage, Requirements,
        RunExports,
    },
    script::{Script, ScriptContent},
    source::{GitRev, GitSource, GitUrl, PathSource, Source, UrlSource},
    test::{
        CommandsTest, CommandsTestFiles, CommandsTestRequirements, DownstreamTest,
        PackageContentsTest, PythonTest, TestType,
    },
};

use super::custom_yaml::Node;

/// A recipe that has been parsed and validated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// The schema version of this recipe YAML file
    pub schema_version: u64,
    /// The package information
    pub package: Package,
    /// The information about where to obtain the sources
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,
    /// The information about how to build the package
    pub build: Build,
    /// The information about the requirements
    pub requirements: Requirements,
    /// The information about how to test the package
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestType>,
    /// The information about the package
    #[serde(default, skip_serializing_if = "About::is_default")]
    pub about: About,
}

pub(crate) trait CollectErrors<K, V>: Iterator<Item = Result<K, V>> + Sized {
    fn collect_errors(self) -> Result<(), Vec<V>> {
        let err = self
            .filter_map(|res| match res {
                Ok(_) => None,
                Err(err) => Some(err),
            })
            .fold(Vec::<V>::new(), |mut acc, x| {
                acc.push(x);
                acc
            });
        if err.is_empty() {
            Ok(())
        } else {
            Err(err)
        }
    }
}

impl<T, K, V> CollectErrors<K, V> for T where T: Iterator<Item = Result<K, V>> + Sized {}

pub(crate) trait FlattenErrors<K, V>: Iterator<Item = Result<K, Vec<V>>> + Sized {
    fn flatten_errors(self) -> Result<(), Vec<V>> {
        let err = self
            .filter_map(|res| match res {
                Ok(_) => None,
                Err(err) => Some(err),
            })
            .fold(Vec::<V>::new(), |mut acc, x| {
                acc.extend(x);
                acc
            });
        if err.is_empty() {
            Ok(())
        } else {
            Err(err)
        }
    }
}

impl<T, K, V> FlattenErrors<K, V> for T where T: Iterator<Item = Result<K, Vec<V>>> + Sized {}

impl Recipe {
    /// Build a recipe from a YAML string.
    pub fn from_yaml(yaml: &str, jinja_opt: SelectorConfig) -> Result<Self, Vec<ParsingError>> {
        let yaml_root = Node::parse_yaml(0, yaml).map_err(|err| vec![err])?;

        Self::from_node(&yaml_root, jinja_opt).map_err(|errs| {
            errs.into_iter()
                .map(|err| ParsingError::from_partial(yaml, err))
                .collect()
        })
    }

    /// Build a recipe from a YAML string and use a given package hash string as default value.
    pub fn from_yaml_with_default_hash_str(
        yaml: &str,
        default_pkg_hash: &str,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, Vec<ParsingError>> {
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
    ) -> Result<Self, Vec<PartialParsingError>> {
        let hash = jinja_opt.hash.clone();
        let mut jinja = Jinja::new(jinja_opt);

        let root_node = root_node.as_mapping().ok_or_else(|| {
            vec![_partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                help = "root node must always be a map with keys like `package`, `source`, `build`, `requirements`, `tests`, `about`, `context` and `extra`"
            )]
        })?;

        // add context values
        if let Some(context) = root_node.get("context") {
            let context = context.as_mapping().ok_or_else(|| {
                vec![_partialerror!(
                    *context.span(),
                    ErrorKind::ExpectedMapping,
                    help = "`context` must always be a mapping"
                )]
            })?;

            context
                .iter()
                .map(|(k, v)| {
                    let val = v.as_scalar().ok_or_else(|| {
                        vec![_partialerror!(
                            *v.span(),
                            ErrorKind::ExpectedScalar,
                            help = "`context` values must always be scalars (strings)"
                        )]
                    })?;
                    let rendered: Option<ScalarNode> =
                        val.render(&jinja, &format!("context.{}", k.as_str()))?;

                    if let Some(rendered) = rendered {
                        jinja.context_mut().insert(
                            k.as_str().to_owned(),
                            Value::from_safe_string(rendered.as_str().to_string()),
                        );
                    }
                    Ok(())
                })
                .flatten_errors()?;
        }

        let rendered_node: RenderedMappingNode = root_node.render(&jinja, "ROOT")?;

        let mut schema_version = 1;
        let mut package = None;
        let mut build = Build::default();
        let mut source = Vec::new();
        let mut requirements = Requirements::default();
        let mut tests = Vec::default();
        let mut about = About::default();

        rendered_node
            .iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "schema_version" => schema_version = value.try_convert(key_str)?,
                    "package" => package = Some(value.try_convert(key_str)?),
                    "recipe" => {
                        return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField("recipe".to_string().into()),
                        help =
                            "The recipe field is only allowed in conjunction with multiple outputs"
                    )])
                    }
                    "source" => source = value.try_convert(key_str)?,
                    "build" => build = value.try_convert(key_str)?,
                    "requirements" => requirements = value.try_convert(key_str)?,
                    "tests" => tests = value.try_convert(key_str)?,
                    "about" => about = value.try_convert(key_str)?,
                    "context" => {}
                    "extra" => {}
                    invalid_key => {
                        return Err(vec![_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(invalid_key.to_string().into()),
                        )])
                    }
                }
                Ok(())
            })
            .flatten_errors()?;

        // Add hash to build.string if it is not set
        if build.string.is_none() {
            if let Some(hash) = hash {
                build.string = Some(format!("{}_{}", hash, build.number));
            }
        }

        // evaluate the skip conditions
        build.skip = build.skip.with_eval(&jinja)?;

        if schema_version != 1 {
            tracing::warn!("Unknown schema version: {}. rattler-build {} is only known to parse schema version 1.",
                schema_version, env!("CARGO_PKG_VERSION"));
        }

        let recipe = Recipe {
            schema_version,
            package: package.ok_or_else(|| {
                vec![_partialerror!(
                    *root_node.span(),
                    ErrorKind::MissingField(Cow::from("package")),
                    label = "missing required field `package`",
                    help = "add the required field `package`"
                )]
            })?,
            build,
            source,
            requirements,
            tests,
            about,
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
    pub const fn tests(&self) -> &Vec<TestType> {
        &self.tests
    }

    /// Get the about information.
    pub const fn about(&self) -> &About {
        &self.about
    }
}

#[cfg(test)]
mod tests {
    use insta::{assert_snapshot, assert_yaml_snapshot};
    use rattler_conda_types::Platform;

    use crate::{assert_miette_snapshot, variant_config::ParseErrors};

    use super::*;

    #[test]
    fn parsing_unix() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/xtensor.yaml");

        let selector_config_unix = SelectorConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            ..SelectorConfig::default()
        };

        let unix_recipe = Recipe::from_yaml(recipe, selector_config_unix);
        assert!(unix_recipe.is_ok());
        insta::assert_debug_snapshot!("unix_recipe", unix_recipe.unwrap());
    }

    #[test]
    fn parsing_win() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/xtensor.yaml");

        let selector_config_win = SelectorConfig {
            target_platform: Platform::Win64,
            host_platform: Platform::Win64,
            ..SelectorConfig::default()
        };

        let win_recipe = Recipe::from_yaml(recipe, selector_config_win);
        assert!(win_recipe.is_ok());
        insta::assert_debug_snapshot!("recipe_windows", win_recipe.unwrap());
    }

    #[test]
    fn bad_skip_single_output() {
        let raw_recipe = include_str!("../../test-data/recipes/test-parsing/recipe_bad_skip.yaml");
        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err: ParseErrors = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn bad_skip_multi_output() {
        let raw_recipe =
            include_str!("../../test-data/recipes/test-parsing/recipe_bad_skip_multi.yaml");
        let recipes = find_outputs_from_src(raw_recipe).unwrap();
        for recipe in recipes {
            let recipe = Recipe::from_node(&recipe, SelectorConfig::default());
            if recipe.is_ok() {
                assert_eq!(recipe.unwrap().package().name().as_normalized(), "zlib-dev");
                continue;
            }
            let err = recipe.unwrap_err();
            let err: ParseErrors = err
                .into_iter()
                .map(|err| ParsingError::from_partial(&raw_recipe, err))
                .collect::<Vec<_>>()
                .into();
            assert_miette_snapshot!(err);
        }
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
        let err: ParseErrors = recipe.unwrap_err().into();
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
        let err: ParseErrors = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn jinja_error() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/recipe_jinja_error.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        let err: ParseErrors = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn duplicate_keys_error() {
        let recipe =
            include_str!("../../test-data/recipes/test-parsing/recipe_duplicate_keys.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        let err: ParseErrors = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn jinja_sequence() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/recipe_inline_jinja.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        assert_yaml_snapshot!(recipe);
    }

    #[test]
    fn binary_relocation() {
        let recipe = include_str!(
            "../../test-data/recipes/test-parsing/recipe_build_binary_relocation.yaml"
        );
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        assert_yaml_snapshot!(recipe);
    }

    #[test]
    fn binary_relocation_paths() {
        let recipe = include_str!(
            "../../test-data/recipes/test-parsing/recipe_build_binary_relocation_paths.yaml"
        );
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        assert_yaml_snapshot!(recipe);
    }

    #[test]
    fn map_null_values() {
        let recipe =
            include_str!("../../test-data/recipes/test-parsing/map_jinja_null_values.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        assert_yaml_snapshot!(recipe);
    }

    #[test]
    fn test_complete_recipe() {
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            ..SelectorConfig::default()
        };
        let recipe = include_str!("../../test-data/recipes/test-parsing/single_output.yaml");
        let recipe = Recipe::from_yaml(recipe, selector_config).unwrap();
        assert_snapshot!(serde_yaml::to_string(&recipe).unwrap());
    }
}
