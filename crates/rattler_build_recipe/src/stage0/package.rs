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
        let mut vars = self.name.used_variables();
        vars.extend(self.version.used_variables());
        vars.sort();
        vars.dedup();
        vars
    }
}
