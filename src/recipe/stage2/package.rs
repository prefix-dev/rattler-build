use std::str::FromStr;

use rattler_conda_types::PackageName;
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, ScalarNode,
            TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1, Render,
    },
};

/// A recipe package information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    name: PackageName,
    version: String,
}

impl Package {
    pub(super) fn from_stage1(
        package: &stage1::Package,
        jinja: &Jinja,
    ) -> Result<Self, PartialParsingError> {
        let name: Option<ScalarNode> = package.name.render(jinja, "package name")?;

        let Some(name) = name else {
            return Err(_partialerror!(
                *package.name.span(),
                ErrorKind::Other,
                label = "package name is required"
            ));
        };

        let name = PackageName::from_str(name.as_str()).map_err(|_err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::Other,
                label = "error parsing package name"
            )
        })?;

        let version: Option<ScalarNode> = package.version.render(jinja, "package version")?;

        let Some(version) = version else {
            return Err(_partialerror!(
                *package.version.span(),
                ErrorKind::Other,
                label = "package version is required"
            ));
        };

        Ok(Package {
            name,
            version: version.to_string(),
        })
    }

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
        let mut version = String::new();

        for (key, value) in self.iter() {
            match key.as_str() {
                "name" => {
                    name_val = Some(value.try_convert("name")?);
                }
                "version" => {
                    version = value.try_convert("version")?;
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

        let name = name_val.ok_or_else(|| {
            _partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = format!("error parsing `{name}` field `name`")
            )
        })?;

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
