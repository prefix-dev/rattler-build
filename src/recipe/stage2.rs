//! Second and final stage of the recipe parser pipeline.
//!
//! This stage takes the [`RawRecipe`] from the first stage and parses it into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.
use minijinja::Value;
use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, ScalarNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        jinja::Jinja,
        stage1::RawRecipe,
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

use super::custom_yaml::Node;

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
        let raw = RawRecipe::from_yaml(yaml)?;
        Self::from_raw(raw, jinja_opt).map_err(|err| ParsingError::from_partial(yaml, err))
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

    /// Build a recipe from a [`RawRecipe`].
    pub fn from_raw(
        raw: RawRecipe,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, PartialParsingError> {
        // Init minijinja
        let mut jinja = Jinja::new(jinja_opt);

        for (k, v) in raw.context {
            let rendered = jinja.render_str(v.as_str()).map_err(|err| {
                _partialerror!(
                    *v.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering context"
                )
            })?;

            jinja
                .context_mut()
                .insert(k.as_str().to_owned(), Value::from_safe_string(rendered));
        }

        let package = Package::from_stage1(&raw.package, &jinja)?;
        let source = Source::from_stage1(raw.source, &jinja)?;

        let about = raw
            .about
            .as_ref()
            .map(|about| About::from_stage1(about, &jinja))
            .transpose()?
            .unwrap_or_default();

        let requirements = raw
            .requirements
            .as_ref()
            .map(|req| Requirements::from_stage1(req, &jinja))
            .transpose()?
            .unwrap_or_default();

        let build = Build::from_stage1(&raw.build, &jinja)?;
        let test = Test::from_stage1(&raw.test, &jinja)?;

        Ok(Self {
            package,
            source,
            build,
            requirements,
            test,
            about,
            extra: (),
        })
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
                    label = "`context` mulst always be a mapping"
                )
            })?;

            for (k, v) in context.iter() {
                let val = v.as_scalar().ok_or_else(|| {
                    _partialerror!(
                        *v.span(),
                        ErrorKind::ExpectedScalar,
                        label = "`context` values must always be scalars"
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

        let mut package = None;
        let mut source = Vec::new();

        for (key, value) in rendered_node.iter() {
            match key.as_str() {
                "package" => package = Some(Package::from_rendered_node(value)?),
                "source" => source.extend(Source::from_rendered_node(value)?),
                "build" => {}
                "requirements" => {}
                "test" => {}
                "about" => {}
                "outputs" => {}
                "context" => {}
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::Other,
                        label = format!("invalid key `{invalid_key}`")
                    ))
                }
            }
        }

        let _recipe = Recipe {
            package: package.ok_or_else(|| {
                _partialerror!(
                    *root_node.span(),
                    ErrorKind::Other,
                    label = "missing required key `package`"
                )
            })?,
            source,
            build: todo!(),
            requirements: todo!(),
            test: todo!(),
            about: todo!(),
            extra: todo!(),
        };

        todo!()
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
    use super::*;

    #[test]
    fn it_works() {
        let recipe = include_str!("../../examples/xtensor/recipe.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        dbg!(&recipe);
    }
}
