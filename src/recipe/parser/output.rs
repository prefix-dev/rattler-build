use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
};

use super::{About, Build, OutputPackage, Requirements, Source, Test};

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub(crate) package: OutputPackage,
    pub(crate) build: Build,
    pub(crate) source: Vec<Source>,
    pub(crate) requirements: Requirements,
    pub(crate) test: Test,
    pub(crate) about: About,
}

impl Output {
    /// Get the output package information.
    pub fn package(&self) -> &OutputPackage {
        &self.package
    }

    /// Get the output build information.
    pub fn build(&self) -> &Build {
        &self.build
    }

    pub fn source(&self) -> &[Source] {
        &self.source
    }

    pub fn requirements(&self) -> &Requirements {
        &self.requirements
    }

    pub fn test(&self) -> &Test {
        &self.test
    }

    pub fn about(&self) -> &About {
        &self.about
    }
}

impl TryConvertNode<Output> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Output, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping))
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Output> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Output, PartialParsingError> {
        let mut package = None;
        let mut build = Build::default();
        let mut source = Vec::new();
        let mut requirements = Requirements::default();
        let mut test = Test::default();
        let mut about = About::default();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "package" => {
                    package = value.try_convert(key_str)?;
                }
                "build" => {
                    build = value.try_convert(key_str)?;
                }
                "source" => {
                    source = value.try_convert(key_str)?;
                }
                "requirements" => {
                    requirements = value.try_convert(key_str)?;
                }
                "test" => {
                    test = value.try_convert(key_str)?;
                }
                "about" => {
                    about = value.try_convert(key_str)?;
                }
                invalid => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!(
                            "valid fields for `{name}` are `package`, `build`, `source`, \
                            `requirements`, `test`, and `about`"
                        )
                    ));
                }
            }
        }

        let package = package.ok_or_else(|| {
            _partialerror!(
                *self.span(),
                ErrorKind::MissingField("package".to_string().into())
            )
        })?;

        Ok(Output {
            package,
            build,
            source,
            requirements,
            test,
            about,
        })
    }
}

impl TryConvertNode<Output> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<Output, PartialParsingError> {
        Err(_partialerror!(
            *self.span(),
            ErrorKind::ExpectedMapping,
            help = format!("`{name}` must be a mapping")
        ))
    }
}
