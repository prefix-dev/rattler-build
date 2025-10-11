//! Stage 1 Recipe - evaluated recipe with all templates and conditionals resolved

use super::{About, Build, Extra, Package, Requirements, Source, TestType};

/// Evaluated recipe with all templates and conditionals resolved
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// Package information (required)
    pub package: Package,

    /// Build configuration
    pub build: Build,

    /// About metadata
    pub about: About,

    /// Requirements/dependencies
    pub requirements: Requirements,

    /// Extra metadata
    pub extra: Extra,

    /// Source information (can be multiple sources)
    pub source: Vec<Source>,

    /// Tests (can be multiple tests)
    pub tests: Vec<TestType>,
}

impl Recipe {
    /// Create a new Recipe
    pub fn new(
        package: Package,
        build: Build,
        about: About,
        requirements: Requirements,
        extra: Extra,
        source: Vec<Source>,
        tests: Vec<TestType>,
    ) -> Self {
        Self {
            package,
            build,
            about,
            requirements,
            extra,
            source,
            tests,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::{PackageName, Version};
    use std::str::FromStr;

    #[test]
    fn test_recipe_minimal() {
        let name = PackageName::from_str("foo").unwrap();
        let version = Version::from_str("1.0.0").unwrap();
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
        use spdx::Expression;

        let name = PackageName::from_str("bar").unwrap();
        let version = Version::from_str("2.0.0").unwrap();
        let pkg = Package::new(name, version);
        let build = Build::with_number(3);
        let about = About {
            license: Some(Expression::parse("MIT").unwrap()),
            summary: Some("A test package".to_string()),
            ..Default::default()
        };
        let reqs = Requirements {
            run: vec!["python".to_string()],
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
        );

        assert_eq!(recipe.package(), &pkg);
        assert_eq!(recipe.build(), &build);
        assert_eq!(recipe.about(), &about);
        assert_eq!(recipe.requirements(), &reqs);
        assert_eq!(recipe.extra(), &extra);
    }
}
