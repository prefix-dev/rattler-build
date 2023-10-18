//! First stage of the recipe parser pipeline.
//!
//! This stage parses the recipe file into a raw representation, without any minijinja processing
//! done and parsing only the required keys and their raw values (that can be jinja syntax).

use linked_hash_map::LinkedHashMap;

use super::{
    custom_yaml::{MappingNode, Node, ScalarNode},
    error::{marker_span_to_span, ErrorKind, ParsingError, PartialParsingError},
};

use crate::{_error, _partialerror};

/// This is the raw reprentation of a recipe, without any minijinja processing done.
///
/// This is the type that is used to parse the recipe file in the first stage, and only validates
/// the existance of the root keys and required keys but not their values (that can be jinja syntax).
#[derive(Debug, Clone, PartialEq)]
pub struct RawRecipe {
    pub(crate) context: LinkedHashMap<ScalarNode, ScalarNode>,
    pub(crate) package: Package,
    pub(crate) source: Source,
    pub(crate) build: Build,
    pub(crate) requirements: Option<Requirements>,
    pub(crate) test: Test,
    pub(crate) about: Option<About>,
    pub(crate) extra: Extra,
}

impl RawRecipe {
    /// Parse a recipe from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, ParsingError> {
        let yaml_root = marked_yaml::parse_yaml(0, yaml)
            .map_err(|err| super::error::load_error_handler(yaml, err))?;

        let yaml_root = Node::try_from(yaml_root)
            .map_err(|err| _error!(yaml, marker_span_to_span(yaml, err.span), err.kind))?;

        // Panic here should never happen, as the marked_yaml requires that the root node is a mapping
        let root_map = yaml_root.as_mapping().expect("top level must be a mapping");

        let mut context = LinkedHashMap::new();
        let mut package = Package {
            name: ScalarNode::from(""),
            version: ScalarNode::from(""),
        };
        let mut source = Source::default();
        let mut build = Build::default();
        let mut requirements = None;
        let mut test = Test::default();
        let mut about = None;
        let mut extra = Extra::default();

        for (key, value) in root_map.iter() {
            let key = key.as_str();

            match key {
                "context" => {
                    let context_node = value;
                    let context_span = marker_span_to_span(yaml, *context_node.span());

                    let context_mapping = context_node.as_mapping().ok_or({
                        _error!(
                            yaml,
                            context_span,
                            ErrorKind::ExpectedMapping,
                            label = "expected a mapping here",
                        )
                    })?;

                    for (key, value) in context_mapping.iter() {
                        let value = value.as_scalar().ok_or_else(|| {
                            _error!(
                                yaml,
                                marker_span_to_span(yaml, *value.span()),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            )
                        })?;

                        context.insert(key.clone(), value.clone());
                    }
                }
                "package" => {
                    package = Package::from_node(value)
                        .map_err(|err| ParsingError::from_partial(yaml, err))?;
                }
                "source" => source.node = Some(value.clone()),
                "build" => build.node = value.as_mapping().cloned(),
                "requirements" => {
                    requirements = Some(
                        Requirements::from_node(value)
                            .map_err(|err| ParsingError::from_partial(yaml, err))?,
                    );
                }
                "test" => test.node = Some(value.clone()),
                "about" => {
                    about = Some(
                        About::from_node(value)
                            .map_err(|err| ParsingError::from_partial(yaml, err))?,
                    );
                }
                "extra" => extra.node = Some(value.clone()),
                _ => {
                    return Err(_error!(
                        yaml,
                        marker_span_to_span(yaml, *value.span()),
                        ErrorKind::Other,
                        label = "unexpected key",
                        help = "expected one of `context`, `package`, `source`, `build`, `requirements`, `test`, `about` or `extra`"
                    ));
                }
            }
        }

        Ok(Self {
            context,
            package,
            source,
            build,
            requirements,
            test,
            about,
            extra,
        })
    }
}

/// A package with name and version
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// The package name
    pub(crate) name: ScalarNode,
    /// The package version
    pub(crate) version: ScalarNode,
}

impl Package {
    pub fn from_node(node: &Node) -> Result<Self, PartialParsingError> {
        if let Some(package_node) = node.as_mapping() {
            let package_span = *package_node.span();

            let mut name = None;
            let mut version = None;

            for (key, value) in package_node.iter() {
                let key = key.as_str();

                match key {
                    "name" => {
                        if let Some(name_node) = value.as_scalar() {
                            name = Some(name_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "version" => {
                        if let Some(version_node) = value.as_scalar() {
                            version = Some(version_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    _ => {
                        return Err(_partialerror!(
                            *value.span(),
                            ErrorKind::Other,
                            label = "unexpected key",
                            help = "expected one of `name` or `version`"
                        ));
                    }
                }
            }

            let name = name.ok_or_else(|| {
                _partialerror!(package_span, ErrorKind::Other, label = "missing key `name`",)
            })?;

            let version = version.ok_or_else(|| {
                _partialerror!(
                    package_span,
                    ErrorKind::Other,
                    label = "missing key `version`",
                )
            })?;

            Ok(Package { name, version })
        } else {
            Err(_partialerror!(
                *node.span(),
                ErrorKind::ExpectedMapping,
                label = "expected a mapping here",
            ))
        }
    }
}

/// A source of a package
///
/// There are many possibilities for this field that cannot be semantically checked
/// in the first stage. It is optional, there is no required fields, allows for if-selector,
/// certain fields only occours with another field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    pub(crate) node: Option<Node>,
}

/// A build of a package
///
/// There are many possibilities for this field that cannot be semantically checked
/// in the first stage. It is optional, there is no required fields, allows for if-selector,
/// certain fields only occours with another field.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Build {
    pub(crate) node: Option<MappingNode>,
}

/// A requirements of a package (dependencies)
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Requirements {
    pub(crate) build: Option<Node>,
    pub(crate) host: Option<Node>,
    pub(crate) run: Option<Node>,
    pub(crate) run_constrained: Option<Node>,
}

impl Requirements {
    fn from_node(value: &Node) -> Result<Self, PartialParsingError> {
        if let Some(requirements_node) = value.as_mapping() {
            let requirements_span = *requirements_node.span();

            let mut req = Requirements::default();

            for (key, value) in requirements_node.iter() {
                let key = key.as_str();

                match key {
                    "build" => req.build = Some(value.clone()),
                    "host" => req.host = Some(value.clone()),
                    "run" => req.run = Some(value.clone()),
                    "run_constrained" => req.run_constrained = Some(value.clone()),
                    _ => {
                        return Err(_partialerror!(
                            requirements_span,
                            ErrorKind::Other,
                            label = "unexpected key",
                            help = "expected one of `build`, `host`, `run` or `run_constrained`"
                        ));
                    }
                }
            }

            Ok(req)
        } else {
            Err(_partialerror!(
                *value.span(),
                ErrorKind::ExpectedMapping,
                label = "expected a mapping here",
            ))
        }
    }
}

/// A tests of a package
///
/// There are many possibilities for this field that cannot be semantically checked
/// in the first stage. It is optional, there is no required fields, allows for if-selector,
/// certain fields only occours with another field.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Test {
    pub(crate) node: Option<Node>,
}

/// A package about information
#[derive(Debug, Default, Clone, PartialEq)]
pub struct About {
    pub(crate) homepage: Option<Node>,
    pub(crate) repository: Option<ScalarNode>,
    pub(crate) documentation: Option<ScalarNode>,
    pub(crate) license: Option<ScalarNode>,
    pub(crate) license_family: Option<ScalarNode>,
    pub(crate) license_file: Option<Node>,
    pub(crate) license_url: Option<ScalarNode>,
    pub(crate) summary: Option<ScalarNode>,
    pub(crate) description: Option<ScalarNode>,
    pub(crate) prelink_message: Option<ScalarNode>,
}

impl About {
    fn from_node(node: &Node) -> Result<Self, PartialParsingError> {
        if let Some(about_node) = node.as_mapping() {
            let mut ab = About::default();

            for (key, value) in about_node.iter() {
                let key_str = key.as_str();

                match key_str {
                    "homepage" | "home" => ab.homepage = Some(value.clone()),
                    "repository" | "dev_url" => {
                        if let Some(repository_node) = value.as_scalar() {
                            ab.repository = Some(repository_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "documentation" | "doc_url" => {
                        if let Some(documentation_node) = value.as_scalar() {
                            ab.documentation = Some(documentation_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "license" => {
                        if let Some(license_node) = value.as_scalar() {
                            ab.license = Some(license_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "license_family" => {
                        if let Some(license_family_node) = value.as_scalar() {
                            ab.license_family = Some(license_family_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "license_file" => ab.license_file = Some(value.clone()),
                    "license_url" => {
                        if let Some(license_url_node) = value.as_scalar() {
                            ab.license_url = Some(license_url_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "summary" => {
                        if let Some(summary_node) = value.as_scalar() {
                            ab.summary = Some(summary_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "description" => {
                        if let Some(description_node) = value.as_scalar() {
                            ab.description = Some(description_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    "prelink_message" => {
                        if let Some(prelink_message_node) = value.as_scalar() {
                            ab.prelink_message = Some(prelink_message_node.clone());
                        } else {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                label = "expected a scalar value here",
                            ));
                        }
                    }
                    _ => {
                        return Err(_partialerror!(
                            *key.span(),
                            ErrorKind::Other,
                            label = "unexpected key",
                            help = "expected one of `homepage` (or `home`), `repository` (or `dev_url`), `documentation` (or `doc_url`), `license`, `license_family`, `license_file`, `license_url`, `summary`, `description` or `prelink_message`"
                        ));
                    }
                }
            }

            Ok(ab)
        } else {
            Err(_partialerror!(
                *node.span(),
                ErrorKind::ExpectedMapping,
                label = "expected a mapping here",
            ))
        }
    }
}

/// A tests of a package
///
/// There are many possibilities for this field that cannot be semantically checked
/// in the first stage. It is optional, there is no required fields, allows for if-selector,
/// certain fields only occours with another field.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Extra {
    pub(crate) node: Option<Node>,
}

#[cfg(test)]
mod tests {

    use insta::assert_debug_snapshot;

    use crate::assert_miette_snapshot;

    use super::*;

    #[test]
    fn test_parse() {
        let raw_recipe = include_str!("../../examples/xtensor/recipe.yaml");
        let raw_recipe = RawRecipe::from_yaml(raw_recipe);
        assert!(raw_recipe.is_ok());

        let raw_recipe = raw_recipe.unwrap();

        assert_debug_snapshot!(raw_recipe);
    }

    #[test]
    fn context_not_mapping() {
        let raw_recipe = r#"
        context: "not-mapping"

        package:
          name: test
          version: 0.1.0
        "#;

        let raw_recipe = RawRecipe::from_yaml(raw_recipe);
        assert!(raw_recipe.is_err());

        let err = raw_recipe.unwrap_err();
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

        let raw_recipe = RawRecipe::from_yaml(raw_recipe);
        assert!(raw_recipe.is_err());

        let err = raw_recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }
}
