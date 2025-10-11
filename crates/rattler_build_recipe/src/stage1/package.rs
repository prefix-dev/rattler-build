//! Stage 1 Package - evaluated package information with concrete values

use rattler_conda_types::{PackageName, Version};

/// Evaluated package information with all templates and conditionals resolved
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// The package name (validated conda package name)
    pub name: PackageName,

    /// The package version (validated version)
    pub version: Version,
}

impl Package {
    /// Create a new Package
    pub fn new(name: PackageName, version: Version) -> Self {
        Self { name, version }
    }

    /// Get the package name
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Get the package version
    pub fn version(&self) -> &Version {
        &self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_package_creation() {
        let name = PackageName::from_str("foo").unwrap();
        let version = Version::from_str("1.0.0").unwrap();
        let pkg = Package::new(name.clone(), version.clone());
        assert_eq!(pkg.name(), &name);
        assert_eq!(pkg.version(), &version);
    }
}
