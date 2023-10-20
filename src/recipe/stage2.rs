//! Second and final stage of the recipe parser pipeline.
//!
//! This stage takes the [`RawRecipe`] from the first stage and parses it into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.
use std::str::FromStr;

use minijinja::Value;
use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, ScalarNode, TryConvertNode},
        error::{ErrorKind, ParsingError, PartialParsingError},
        jinja::Jinja,
        stage1::RawRecipe,
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

/// A trait to render a certain stage1 node into its final type.
trait Render<T> {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<T, PartialParsingError>;
}

/// A jinja rendered string
struct Rendered(String);

impl Rendered {
    // Parses this rendered value into another type.
    pub fn parse<F: FromStr>(&self) -> Result<F, F::Err> {
        FromStr::from_str(&self.0)
    }
}

impl<N: TryConvertNode<ScalarNode> + HasSpan> Render<Rendered> for N {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Rendered, PartialParsingError> {
        jinja
            .render_str(self.try_convert(name)?.as_str())
            .map_err(|err| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::JinjaRendering(err),
                    label = format!("error rendering {name}")
                )
            })
            .map(Rendered)
    }
}

impl<N: TryConvertNode<ScalarNode> + HasSpan, T: FromStr> Render<T> for N
where
    ErrorKind: From<T::Err>,
{
    fn render(&self, jinja: &Jinja, name: &str) -> Result<T, PartialParsingError> {
        match Rendered::parse(&self.render(jinja, name)?) {
            Ok(result) => Ok(result),
            Err(e) => Err(_partialerror!(*self.span(), ErrorKind::from(e),)),
        }
    }
}

// impl<N, T> Render<T> for N
// where
//     N: TryConvertNode<ScalarNode> + HasSpan,
//     T: FromStr,
//     T::Err: Display,
// {
//     fn render(&self, jinja: &Jinja, name: &str) -> Result<T, PartialParsingError> {
//         match Rendered::parse(&self.render(jinja, name)?) {
//             Ok(result) => Ok(result),
//             Err(e) => Err(_partialerror!(
//                 *self.span(),
//                 ErrorKind::Other,
//                 label = e.to_string()
//             )),
//         }
//     }
// }

impl<N: Render<T>, T: FromStr> Render<Option<T>> for Option<N> {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Option<T>, PartialParsingError> {
        match self {
            None => Ok(None),
            Some(node) => Ok(Some(node.render(jinja, name)?)),
        }
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
