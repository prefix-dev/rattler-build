//! Second and final stage of the recipe parser pipeline.
//!
//! This phase parses YAML and [`SelectorConfig`] into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Debug;

use crate::{
    _partialerror,
    recipe::{
        Render,
        custom_yaml::{HasSpan, RenderedMappingNode, ScalarNode, TryConvertNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        jinja::Jinja,
    },
    selectors::SelectorConfig,
    source_code::SourceCode,
};

mod about;
mod build;
mod cache;
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
    build::{Build, BuildString, DynamicLinking, PrefixDetection, Python},
    cache::Cache,
    glob_vec::{GlobCheckerVec, GlobVec, GlobWithSource},
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
        PackageContentsTest, PerlTest, PythonTest, PythonVersion, RTest, TestType,
    },
};

use crate::recipe::{custom_yaml::Node, variable::Variable};

/// A recipe that has been parsed and validated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// The schema version of this recipe YAML file
    pub schema_version: u64,
    /// The context values of this recipe
    pub context: IndexMap<String, Variable>,
    /// The package information
    pub package: Package,
    /// The cache build that should be used for this package
    /// This is the same for all outputs of a recipe
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<Cache>,
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
    /// Extra information as a map with string keys and any value
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra: IndexMap<String, serde_yaml::Value>,
}

pub(crate) trait CollectErrors<K, V>: Iterator<Item = Result<K, V>> + Sized {
    fn collect_errors(self) -> Result<(), Vec<V>> {
        let err = self
            .filter_map(|res| res.err())
            .fold(Vec::<V>::new(), |mut acc, x| {
                acc.push(x);
                acc
            });
        if err.is_empty() { Ok(()) } else { Err(err) }
    }
}

impl<T, K, V> CollectErrors<K, V> for T where T: Iterator<Item = Result<K, V>> + Sized {}

pub(crate) trait FlattenErrors<K, V>: Iterator<Item = Result<K, Vec<V>>> + Sized {
    fn flatten_errors(self) -> Result<(), Vec<V>> {
        let err = self
            .filter_map(|res| res.err())
            .fold(Vec::<V>::new(), |mut acc, x| {
                acc.extend(x);
                acc
            });
        if err.is_empty() { Ok(()) } else { Err(err) }
    }
}

impl<T, K, V> FlattenErrors<K, V> for T where T: Iterator<Item = Result<K, Vec<V>>> + Sized {}

impl Recipe {
    /// Build a recipe from a YAML string.
    pub fn from_yaml<S: SourceCode>(
        yaml: S,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, Vec<ParsingError<S>>> {
        let yaml_root = Node::parse_yaml(0, yaml.clone()).map_err(|err| vec![err])?;

        Self::from_node(&yaml_root, jinja_opt).map_err(|errs| {
            errs.into_iter()
                .map(|err| ParsingError::from_partial(yaml.clone(), err))
                .collect()
        })
    }

    fn context_scalar_to_var(
        k: &ScalarNode,
        v: &Node,
        jinja: &Jinja,
    ) -> Result<Option<Variable>, Vec<PartialParsingError>> {
        if k.as_str().contains('-') {
            return Err(vec![_partialerror!(
                *k.span(),
                ErrorKind::InvalidContextVariableName,
                help = "`context` variable names cannot contain hyphens (-) as they are not valid in jinja expressions"
            )]);
        }

        let val = v.as_scalar().ok_or_else(|| {
            vec![_partialerror!(
                *v.span(),
                ErrorKind::ExpectedScalar,
                help = "`context` values must always be scalars (booleans, integers or strings) or uniform lists of scalars"
            )]
        })?;
        let rendered: Option<ScalarNode> = val.render(jinja, &format!("context.{}", k.as_str()))?;
        if let Some(rendered) = rendered {
            let variable = if let Some(value) = rendered.as_bool() {
                Variable::from(value)
            } else if let Some(value) = rendered.as_integer() {
                Variable::from(value)
            } else {
                Variable::from_string(&rendered)
            };
            Ok(Some(variable))
        } else {
            Ok(None)
        }
    }

    /// Create recipes from a YAML [`Node`] structure.
    pub fn from_node(
        root_node: &Node,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, Vec<PartialParsingError>> {
        let experimental = jinja_opt.experimental;
        let mut jinja = Jinja::new(jinja_opt);

        let root_node = root_node.as_mapping().ok_or_else(|| {
            vec![_partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                help = "root node must always be a map with keys like `package`, `source`, `build`, `requirements`, `tests`, `about`, `context` and `extra`"
            )]
        })?;

        // add context values
        let mut context: IndexMap<String, Variable> = IndexMap::new();

        if let Some(context_map) = root_node.get("context") {
            let context_map = context_map.as_mapping().ok_or_else(|| {
                vec![_partialerror!(
                    *context_map.span(),
                    ErrorKind::ExpectedMapping,
                    help = "`context` must always be a mapping"
                )]
            })?;

            for (k, v) in context_map.iter() {
                let variable = if let Some(sequence) = v.as_sequence() {
                    if experimental {
                        let mut rendered_sequence: Vec<Variable> =
                            Vec::with_capacity(sequence.len());

                        for (index, item) in sequence.iter().enumerate() {
                            let rendered_item: Node =
                                item.render(&jinja, &format!("context.{}[{}]", k.as_str(), index))?;
                            if let Some(variable) =
                                Self::context_scalar_to_var(k, &rendered_item, &jinja)?
                            {
                                if index != 0
                                    && variable.as_ref().kind()
                                        != rendered_sequence[0].as_ref().kind()
                                {
                                    return Err(vec![_partialerror!(
                                        *item.span(),
                                        ErrorKind::SequenceMixedTypes((
                                            variable.as_ref().kind(),
                                            rendered_sequence[0].as_ref().kind()
                                        )),
                                        help = "sequence `context` must have all members of the same scalar type"
                                    )]);
                                }
                                rendered_sequence.push(variable);
                            }
                        }
                        Variable::from(rendered_sequence)
                    } else {
                        return Err(vec![_partialerror!(
                            *k.span(),
                            ErrorKind::ExperimentalOnly("context-list".to_string()),
                            help = "Sequence values in `context` are only allowed in experimental mode (`--experimental`)"
                        )]);
                    }
                } else if let Some(variable) = Self::context_scalar_to_var(k, v, &jinja)? {
                    variable
                } else {
                    continue;
                };
                context.insert(k.as_str().to_string(), variable.clone());
                // also immediately insert into jinja context so that the value can be used
                // in later jinja expressions
                jinja
                    .context_mut()
                    .insert(k.as_str().to_string(), variable.into());
            }
        }

        let rendered_node: RenderedMappingNode = root_node.render(&jinja, "ROOT")?;

        let mut schema_version = 1;
        let mut package = None;
        let mut build = Build::default();
        let mut source = Vec::new();
        let mut requirements = Requirements::default();
        let mut tests = Vec::default();
        let mut about = About::default();
        let mut cache = None;
        let mut extra = IndexMap::default();

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
                    "cache" => {
                        if experimental {
                            cache = Some(value.try_convert(key_str)?)
                        } else {
                            return Err(vec![_partialerror!(
                                *key.span(),
                                ErrorKind::ExperimentalOnly("cache".to_string()),
                                help = "The `cache` key is only allowed in experimental mode (`--experimental`)"
                            )])
                        }
                    }
                    "source" => source = value.try_convert(key_str)?,
                    "build" => build = value.try_convert(key_str)?,
                    "requirements" => requirements = value.try_convert(key_str)?,
                    "tests" => tests = value.try_convert(key_str)?,
                    "about" => about = value.try_convert(key_str)?,
                    "context" => {}
                    "extra" => extra = value.as_mapping().ok_or_else(|| {
                                            vec![_partialerror!(
                                                *value.span(),
                                                ErrorKind::ExpectedMapping,
                                                label = format!("expected a mapping for `{key_str}`")
                                            )]
                                        })
                                        .and_then(|m| m.try_convert(key_str))?,
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

        // evaluate the skip conditions
        build.skip = build.skip.with_eval(&jinja)?;

        if schema_version != 1 {
            tracing::warn!(
                "Unknown schema version: {}. rattler-build {} is only known to parse schema version 1.",
                schema_version,
                env!("CARGO_PKG_VERSION")
            );
        }

        let recipe = Recipe {
            schema_version,
            context,
            package: package.ok_or_else(|| {
                vec![_partialerror!(
                    *root_node.span(),
                    ErrorKind::MissingField(Cow::from("package")),
                    label = "missing required field `package`",
                    help = "add the required field `package`"
                )]
            })?,
            cache,
            build,
            source,
            requirements,
            tests,
            about,
            extra,
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
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"character: \d+", "character: [FILTERED]");
        settings.bind(|| {
            insta::assert_debug_snapshot!("unix_recipe", unix_recipe.unwrap());
        });
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
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"character: \d+", "character: [FILTERED]");
        settings.bind(|| {
            insta::assert_debug_snapshot!("recipe_windows", win_recipe.unwrap());
        });
    }

    #[test]
    fn bad_skip_single_output() {
        let raw_recipe = include_str!("../../test-data/recipes/test-parsing/recipe_bad_skip.yaml");
        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err: ParseErrors<_> = recipe.unwrap_err().into();
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
            let err: ParseErrors<_> = err
                .into_iter()
                .map(|err| ParsingError::from_partial(raw_recipe, err))
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
        let err: ParseErrors<_> = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn context_value_not_scalar() {
        let raw_recipe = r#"
        context:
          key:
            foo:
              - [not, scalar]

        package:
            name: test
            version: 0.1.0
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err: ParseErrors<_> = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn context_value_not_uniform_list() {
        let raw_recipe = r#"
        context:
          foo:
            - foo
            - bar
            - 3
            - 4
            - baz

        package:
            name: test
            version: 0.1.0
        "#;

        let recipe = Recipe::from_yaml(
            raw_recipe,
            SelectorConfig {
                experimental: true,
                ..SelectorConfig::default()
            },
        );
        let err: ParseErrors<_> = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn context_variable_with_hyphen() {
        let raw_recipe = r#"
        context:
          foo-bar: baz

        package:
            name: test
            version: 0.1.0
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err: ParseErrors<_> = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn jinja_error() {
        let recipe = include_str!("../../test-data/recipes/test-parsing/recipe_jinja_error.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        let err: ParseErrors<_> = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn duplicate_keys_error() {
        let recipe =
            include_str!("../../test-data/recipes/test-parsing/recipe_duplicate_keys.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default());
        let err: ParseErrors<_> = recipe.unwrap_err().into();
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
