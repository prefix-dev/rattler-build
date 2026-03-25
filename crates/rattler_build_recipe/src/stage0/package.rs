use rattler_conda_types::{SourcePackageName, VersionWithSource};
use serde::Serialize;

use crate::stage0::types::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Package {
    pub name: Value<SourcePackageName>,
    pub version: Value<VersionWithSource>,
}

impl Package {
    /// Create a new package with the given name and version
    pub fn new(name: Value<SourcePackageName>, version: Value<VersionWithSource>) -> Self {
        Self { name, version }
    }

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
    pub name: Value<SourcePackageName>,
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
