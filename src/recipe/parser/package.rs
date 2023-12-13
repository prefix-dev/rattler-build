use std::str::FromStr;

use itertools::Itertools;
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
    fn try_convert(&self, name: &str) -> Result<Package, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping,)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<Package> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Package, Vec<PartialParsingError>> {
        let mut name_val = None;
        let mut version = None;

        let (_, errs): (Vec<()>, Vec<Vec<PartialParsingError>>) = self
            .iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "name" => name_val = value.try_convert(key_str)?,
                    "version" => version = value.try_convert(key_str)?,
                    invalid => {
                        return Err(vec![_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(invalid.to_string().into()),
                            help = format!("valid fields for `{name}` are `name` and `version`")
                        )])
                    }
                }
                Ok(())
            })
            .partition_result();

        if !errs.is_empty() {
            return Err(errs.into_iter().flatten().collect_vec());
        }

        let Some(version) = version else {
            return Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("version".into()),
                label = "add the field `version` in between here",
                help = format!("the field `version` is required for `{name}`")
            )]);
        };

        let Some(name) = name_val else {
            return Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("name".into()),
                label = "add the field `name` in between here",
                help = format!("the field `name` is required for `{name}`")
            )]);
        };

        Ok(Package { name, version })
    }
}

/// A package information used for [`Output`]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputPackage {
    name: PackageName,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

impl OutputPackage {
    /// Get the package name.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Get the package version.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl TryConvertNode<OutputPackage> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<OutputPackage, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping,)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<OutputPackage> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<OutputPackage, Vec<PartialParsingError>> {
        let mut name_val = None;
        let mut version = None;
        let span = *self.span();

        let (_, errs): (Vec<()>, Vec<Vec<PartialParsingError>>) = self
            .iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "name" => {
                        name_val = value.try_convert(key_str)?;
                    }
                    "version" => {
                        version = value.try_convert(key_str)?;
                    }
                    invalid => {
                        return Err(vec![_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(invalid.to_string().into()),
                            help = format!("valid fields for `{name}` are `name` and `version`")
                        )])
                    }
                }
                Ok(())
            })
            .partition_result();

        if !errs.is_empty() {
            return Err(errs.into_iter().flatten().collect_vec());
        }

        let Some(name) = name_val else {
            return Err(vec![_partialerror!(
                span,
                ErrorKind::MissingField("name".into()),
                help = format!("the field `name` is required for `{name}`")
            )]);
        };

        Ok(OutputPackage { name, version })
    }
}

impl TryConvertNode<PackageName> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PackageName, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedScalar)])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<PackageName> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<PackageName, Vec<PartialParsingError>> {
        PackageName::from_str(self.as_str())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::from(err),)])
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assert_miette_snapshot,
        recipe::{jinja::SelectorConfig, Recipe},
        variant_config::ParseErrors,
    };

    #[test]
    fn missing_fields() {
        let raw_recipe = r#"
        package:
            name: test
        "#;

        let recipe = Recipe::from_yaml(raw_recipe, SelectorConfig::default());
        let err: ParseErrors = recipe.unwrap_err().into();
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
        let err: ParseErrors = recipe.unwrap_err().into();
        assert_miette_snapshot!(err);
    }
}
