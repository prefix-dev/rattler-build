use rattler_conda_types::VersionWithSource;
use serde::Serialize;

use crate::stage0::types::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PackageName(pub rattler_conda_types::PackageName);

impl ToString for PackageName {
    fn to_string(&self) -> String {
        self.0.as_source().to_string()
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
