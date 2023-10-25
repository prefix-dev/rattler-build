use std::str::FromStr;

use rattler_conda_types::PackageName;
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
};

/// A recipe package information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    name: PackageName,
    version: String,
}

impl Package {
    /// Get the package name.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Get the package version.
    pub fn version(&self) -> &str {
        &self.version
    }
}

impl TryConvertNode<Package> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Package, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping,))
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Package> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Package, PartialParsingError> {
        let mut name_val = None;
        let mut version = None;
        let span = *self.span();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "name" => {
                    name_val = value.try_convert(key_str)?;
                }
                "version" => {
                    version = value.try_convert(key_str)?;
                }
                invalid => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!("valid fields for `{name}` are `name` and `version`")
                    ))
                }
            }
        }

        let Some(version) = version else {
            return Err(_partialerror!(
                span,
                ErrorKind::MissingField("version".into()),
                help = format!("the field `version` is required for `{name}`")
            ));
        };

        let Some(name) = name_val else {
            return Err(_partialerror!(
                span,
                ErrorKind::MissingField("name".into()),
                help = format!("the field `name` is required for `{name}`")
            ));
        };

        Ok(Package { name, version })
    }
}

impl TryConvertNode<PackageName> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PackageName, PartialParsingError> {
        self.as_scalar()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedScalar))
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<PackageName> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<PackageName, PartialParsingError> {
        PackageName::from_str(self.as_str())
            .map_err(|err| _partialerror!(*self.span(), ErrorKind::from(err),))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assert_miette_snapshot,
        recipe::{jinja::SelectorConfig, Recipe},
    };

    #[test]
    fn missing_fields() {
        let raw_recipe = r#"
        package:
            name: test
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err = recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }

    #[test]
    fn invalid_fields() {
        let raw_recipe = r#"
        package:
            name: test
            version: 0.1.0
            invalid: "field"
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err = recipe.unwrap_err();
        assert_miette_snapshot!(err);
    }
}
