use std::str::FromStr;

use rattler_conda_types::PackageName;
use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1,
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
        let name = jinja.render_str(package.name.as_str()).map_err(|err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::JinjaRendering(err),
                label = "error rendering package name"
            )
        })?;
        let name = PackageName::from_str(name.as_str()).map_err(|_err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::Other,
                label = "error parsing package name"
            )
        })?;
        let version = jinja.render_str(package.version.as_str()).map_err(|err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::JinjaRendering(err),
                label = "error rendering package version"
            )
        })?;
        Ok(Package { name, version })
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
