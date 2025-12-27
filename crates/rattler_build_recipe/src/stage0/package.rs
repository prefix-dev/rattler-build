use std::fmt::Display;

use rattler_conda_types::VersionWithSource;
use serde::Serialize;

use crate::stage0::types::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PackageName(pub rattler_conda_types::PackageName);

impl Display for PackageName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_source())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Package {
    pub name: Value<PackageName>,
    pub version: Value<VersionWithSource>,
}

impl Package {
    pub fn used_variables(&self) -> Vec<String> {
        let Package { name, version } = self;

        let mut vars = name.used_variables();
        vars.extend(version.used_variables());
        vars.sort();
        vars.dedup();
        vars
    }
}

/// Package metadata for multi-output recipes
///
/// In multi-output recipes, the version can be omitted from package outputs
/// and will be inherited from the recipe-level version.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageMetadata {
    pub name: Value<PackageName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Value<VersionWithSource>>,
}

impl PackageMetadata {
    pub fn used_variables(&self) -> Vec<String> {
        let PackageMetadata { name, version } = self;

        let mut vars = name.used_variables();
        if let Some(version) = version {
            vars.extend(version.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
