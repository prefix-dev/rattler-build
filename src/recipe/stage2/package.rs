use std::str::FromStr;

use rattler_conda_types::PackageName;
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedNode, RenderedScalarNode, ScalarNode, TryConvertNode},
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

    pub(super) fn from_rendered_node(node: &RenderedNode) -> Result<Self, PartialParsingError> {
        match node.as_mapping() {
            Some(map) => {
                let mut name = RenderedScalarNode::new_blank();
                let mut version = "";

                for (key, value) in map.iter() {
                    match key.as_str() {
                        "name" => {
                            name = value
                                .as_scalar()
                                .cloned()
                                .ok_or(_partialerror!(*value.span(), ErrorKind::ExpectedScalar))?
                        }
                        "version" => {
                            version = value
                                .as_scalar()
                                .map(|s| s.as_str())
                                .ok_or(_partialerror!(*value.span(), ErrorKind::ExpectedScalar))?
                        }
                        _ => {
                            return Err(_partialerror!(
                                *key.span(),
                                ErrorKind::Other,
                                label = "invalid field",
                                help = "valid fields for `package` are `name` and `version`"
                            ))
                        }
                    }
                }

                let name = PackageName::from_str(name.as_str()).map_err(|_err| {
                    _partialerror!(
                        *name.span(),
                        ErrorKind::Other,
                        label = "error parsing `package` field `name`"
                    )
                })?;

                Ok(Package {
                    name,
                    version: version.to_string(),
                })
            }
            None => Err(_partialerror!(
                *node.span(),
                ErrorKind::ExpectedMapping,
                help = "package must be a mapping with `name` and `version` keys"
            )),
        }
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

impl TryConvertNode<PackageName> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PackageName, PartialParsingError> {
        self.as_scalar()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedScalar))
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<PackageName> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<PackageName, PartialParsingError> {
        PackageName::from_str(self.as_str())
            .map_err(|err| _partialerror!(*self.span(), ErrorKind::from(err),))
    }
}
