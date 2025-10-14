//! Stage 1 Recipe - evaluated recipe with all templates and conditionals resolved

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{About, Build, Extra, Package, Requirements, Source, TestType};

/// Evaluated recipe with all templates and conditionals resolved
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    /// Package information (required)
    pub package: Package,

    /// Build configuration
    #[serde(default, skip_serializing_if = "Build::is_default")]
    pub build: Build,

    /// About metadata
    #[serde(default, skip_serializing_if = "About::is_empty")]
    pub about: About,

    /// Requirements/dependencies
    #[serde(default, skip_serializing_if = "Requirements::is_empty")]
    pub requirements: Requirements,

    /// Extra metadata
    #[serde(default, skip_serializing_if = "Extra::is_empty")]
    pub extra: Extra,

    /// Source information (can be multiple sources)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,

    /// Tests (can be multiple tests)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestType>,

    /// Resolved context variables (the evaluated context after template rendering)
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub context: IndexMap<String, String>,
}

impl Recipe {
    /// Create a new Recipe
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        package: Package,
        build: Build,
        about: About,
        requirements: Requirements,
        extra: Extra,
        source: Vec<Source>,
        tests: Vec<TestType>,
        context: IndexMap<String, String>,
    ) -> Self {
        Self {
            package,
            build,
            about,
            requirements,
            extra,
            source,
            tests,
            context,
        }
    }

    /// Get the package information
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// Get the build configuration
    pub fn build(&self) -> &Build {
        &self.build
    }

    /// Get the about section
    pub fn about(&self) -> &About {
        &self.about
    }

    /// Get the requirements section
    pub fn requirements(&self) -> &Requirements {
        &self.requirements
    }

    /// Get the extra section
    pub fn extra(&self) -> &Extra {
        &self.extra
    }

    /// Get the source section
    pub fn source(&self) -> &[Source] {
        &self.source
    }

    /// Get the tests section
    pub fn tests(&self) -> &[TestType] {
        &self.tests
    }

    /// Get the resolved context variables
    pub fn context(&self) -> &IndexMap<String, String> {
        &self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage0::License;
    use rattler_conda_types::{PackageName, VersionWithSource};
    use std::str::FromStr;

    #[test]
    fn test_recipe_minimal() {
        let name = PackageName::from_str("foo").unwrap();
        let version = VersionWithSource::from_str("1.0.0").unwrap();
        let pkg = Package::new(name, version);
        let build = Build::default();
        let about = About::default();
        let reqs = Requirements::default();
        let extra = Extra::default();

        let recipe = Recipe::new(
            pkg.clone(),
            build.clone(),
            about.clone(),
            reqs.clone(),
            extra.clone(),
            Vec::new(),
            Vec::new(),
            IndexMap::new(),
        );

        assert_eq!(recipe.package(), &pkg);
        assert_eq!(recipe.build(), &build);
        assert_eq!(recipe.about(), &about);
        assert_eq!(recipe.requirements(), &reqs);
        assert_eq!(recipe.extra(), &extra);
        assert!(recipe.source().is_empty());
        assert!(recipe.tests().is_empty());
    }

    #[test]
    fn test_recipe_with_all_sections() {
        use crate::stage1::Dependency;

        let name = PackageName::from_str("bar").unwrap();
        let version = VersionWithSource::from_str("2.0.0").unwrap();
        let pkg = Package::new(name, version);
        let build = Build::with_number(3);
        let about = About {
            license: Some(License::from_str("MIT").unwrap()),
            summary: Some("A test package".to_string()),
            ..Default::default()
        };
        let reqs = Requirements {
            run: vec![Dependency::Spec(Box::new("python".parse().unwrap()))],
            ..Default::default()
        };
        let extra = Extra {
            recipe_maintainers: vec!["Alice".to_string()],
        };

        let recipe = Recipe::new(
            pkg.clone(),
            build.clone(),
            about.clone(),
            reqs.clone(),
            extra.clone(),
            Vec::new(),
            Vec::new(),
            IndexMap::new(),
        );

        assert_eq!(recipe.package(), &pkg);
        assert_eq!(recipe.build(), &build);
        assert_eq!(recipe.about(), &about);
        assert_eq!(recipe.requirements(), &reqs);
        assert_eq!(recipe.extra(), &extra);
    }
}
