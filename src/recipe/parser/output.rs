//! Output parsing is a bit more complicated than the other sections.
//!
//! The reason for this is that the `outputs` field is a list of mappings, and
//! each mapping can have its own `package`, `source`, `build`, `requirements`,
//! `test`, and `about` fields.
//!
//! (GrayJack): I think that the best way to do the merges are in the original Node

use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, Node, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
        ParsingError,
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

static DEEP_MERGE_KEYS: [&str; 4] = ["package", "about", "extra", "build"];

pub fn find_outputs(src: &str) -> Result<Vec<Node>, ParsingError> {
    let root_node = marked_yaml::parse_yaml(0, src)
        .map_err(|err| crate::recipe::error::load_error_handler(src, err))?;

    let root_map = root_node.as_mapping().ok_or_else(|| {
        ParsingError::from_partial(
            src,
            _partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                help = "root node must always be a mapping"
            ),
        )
    })?;

    let Some(outputs) = root_map.get("outputs") else {
        let recipe =
            Node::try_from(root_node).map_err(|err| ParsingError::from_partial(src, err))?;
        return Ok(vec![recipe]);
    };

    // TODO: Schema
    let outputs = outputs.as_sequence().ok_or_else(|| {
        ParsingError::from_partial(
            src,
            _partialerror!(
                *outputs.span(),
                ErrorKind::ExpectedSequence,
                help = "`outputs` must always be a sequence"
            ),
        )
    })?;

    let mut res = Vec::with_capacity(outputs.len());

    // the schema says that `outputs` can be either an output, a if-selector or a sequence
    // of outputs and if-selectors. We need to handle all of these cases but for now, lets
    // handle only sequence of outputs
    for output in outputs.iter() {
        // 1. clone the root node
        // 2. remove the `outputs` key
        // 3. substitute repeated value (make sure to preserve the spans)
        // 4. merge skip values (make sure to preserve the spans)
        // Note: Make sure to preserve the spans of the original root span so the error
        // messages remain accurate and point the correct part of the original recipe src

        let mut root = root_map.clone();
        root.remove("outputs");

        // recipe.version, if exists in root, and package.version doesn't exist in output, we will
        // use that instead
        // ignore recipe.name
        let version = root
            .get("recipe")
            .and_then(|recipe| recipe.as_mapping())
            .and_then(|recipe| recipe.get("version"));

        let mut output_node = output.clone();

        let output_map = output_node.as_mapping_mut().ok_or_else(|| {
            ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::ExpectedMapping,
                    help = "`outputs` must always be a mapping"
                ),
            )
        })?;

        for (key, value) in root.iter() {
            if !output_map.contains_key(key) {
                output_map.insert(key.clone(), value.clone());
            } else {
                // deep merge
                if DEEP_MERGE_KEYS.contains(&key.as_str()) {
                    let output_value = output_map.get_mut(key).unwrap();
                    let output_value_map = output_value.as_mapping_mut().unwrap();

                    let mut root_value = value.clone();
                    let root_value_map = root_value.as_mapping_mut().unwrap();

                    for (key, value) in root_value_map.iter() {
                        if !root_value_map.contains_key(key) {
                            output_value_map.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
        }

        if let Some(version) = version {
            let Some(package_map) = output_map
                .get_mut("package")
                .and_then(|node| node.as_mapping_mut())
            else {
                return Err(ParsingError::from_partial(
                    src,
                    _partialerror!(
                        *output_node.span(),
                        ErrorKind::MissingField("package".to_string().into())
                    ),
                ));
            };

            if !package_map.contains_key("version") {
                package_map.insert("version".into(), version.clone());
            }
        }

        output_map.remove("recipe");

        let recipe =
            Node::try_from(output_node).map_err(|err| ParsingError::from_partial(src, err))?;
        res.push(recipe);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_outputs() {
        let src = include_str!("../../../../zlib.yaml");
        let outputs = find_outputs(src).unwrap();

        insta::assert_debug_snapshot!("1", outputs[0]);
        insta::assert_debug_snapshot!("2", outputs[1]);
        insta::assert_debug_snapshot!("3", outputs[2]);
    }
}
