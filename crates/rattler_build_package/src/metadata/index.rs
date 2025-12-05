//! IndexJson builder

use chrono::{DateTime, Utc};
use rattler_conda_types::package::IndexJson;
use rattler_conda_types::{NoArchType, PackageName, Platform, VersionWithSource};

use crate::Result;

/// Builder for creating IndexJson metadata
///
/// # Example
/// ```rust
/// use rattler_build_package::metadata::IndexJsonBuilder;
/// use rattler_conda_types::{PackageName, VersionWithSource};
/// use std::str::FromStr;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let index = IndexJsonBuilder::new(
///         PackageName::new_unchecked("mypackage"),
///         "1.0.0".parse()?,
///         "h12345_0".to_string()
///     )
///     .with_build_number(0)
///     .with_dependency("python >=3.8".to_string())
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct IndexJsonBuilder {
    name: PackageName,
    version: VersionWithSource,
    build: String,
    build_number: u64,
    arch: Option<String>,
    platform: Option<String>,
    subdir: Option<String>,
    license: Option<String>,
    license_family: Option<String>,
    timestamp: Option<DateTime<Utc>>,
    depends: Vec<String>,
    constrains: Vec<String>,
    noarch: NoArchType,
    track_features: Vec<String>,
}

impl IndexJsonBuilder {
    /// Create a new IndexJsonBuilder
    ///
    /// # Arguments
    /// * `name` - Package name
    /// * `version` - Package version
    /// * `build` - Build string (e.g., "h12345_0")
    pub fn new(name: PackageName, version: VersionWithSource, build: String) -> Self {
        Self {
            name,
            version,
            build,
            build_number: 0,
            arch: None,
            platform: None,
            subdir: None,
            license: None,
            license_family: None,
            timestamp: None,
            depends: Vec::new(),
            constrains: Vec::new(),
            noarch: NoArchType::none(),
            track_features: Vec::new(),
        }
    }

    /// Set the build number
    pub fn with_build_number(mut self, build_number: u64) -> Self {
        self.build_number = build_number;
        self
    }

    /// Set the architecture
    pub fn with_arch(mut self, arch: String) -> Self {
        self.arch = Some(arch);
        self
    }

    /// Set the platform
    pub fn with_platform(mut self, platform: String) -> Self {
        self.platform = Some(platform);
        self
    }

    /// Set arch and platform from a Platform
    pub fn with_target_platform(mut self, target: &Platform) -> Self {
        self.arch = target.arch().map(|a| a.to_string());
        self.platform = target.only_platform().map(|p| p.to_string());
        self.subdir = Some(target.to_string());
        self
    }

    /// Set the license
    pub fn with_license(mut self, license: String) -> Self {
        self.license = Some(license);
        self
    }

    /// Set the license family
    pub fn with_license_family(mut self, family: String) -> Self {
        self.license_family = Some(family);
        self
    }

    /// Set the timestamp
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Add a dependency
    pub fn with_dependency(mut self, dep: String) -> Self {
        self.depends.push(dep);
        self
    }

    /// Set all dependencies
    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.depends = deps;
        self
    }

    /// Add a constraint
    pub fn with_constraint(mut self, constraint: String) -> Self {
        self.constrains.push(constraint);
        self
    }

    /// Set all constraints
    pub fn with_constraints(mut self, constraints: Vec<String>) -> Self {
        self.constrains = constraints;
        self
    }

    /// Set the noarch type
    pub fn with_noarch(mut self, noarch: NoArchType) -> Self {
        self.noarch = noarch;
        self
    }

    /// Add a track feature
    pub fn with_track_feature(mut self, feature: String) -> Self {
        self.track_features.push(feature);
        self
    }

    /// Build the IndexJson
    pub fn build(self) -> Result<IndexJson> {
        Ok(IndexJson {
            name: self.name,
            version: self.version,
            build: self.build,
            build_number: self.build_number,
            arch: self.arch,
            platform: self.platform,
            subdir: self.subdir,
            license: self.license,
            license_family: self.license_family,
            timestamp: self.timestamp,
            depends: self.depends,
            constrains: self.constrains,
            noarch: self.noarch,
            track_features: self.track_features,
            features: None,
            python_site_packages_path: None,
            purls: None,
            experimental_extra_depends: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_builder() -> Result<()> {
        let name = PackageName::new_unchecked("test");
        let version = "1.0.0".parse().unwrap();

        let index = IndexJsonBuilder::new(name.clone(), version, "h12345_0".to_string())
            .with_build_number(0)
            .with_dependency("python >=3.8".to_string())
            .build()?;

        assert_eq!(index.name, name);
        assert_eq!(index.build_number, 0);
        assert_eq!(index.depends.len(), 1);

        Ok(())
    }
}
