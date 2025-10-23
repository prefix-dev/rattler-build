//! Stage 1 Recipe - evaluated recipe with all templates and conditionals resolved

use indexmap::IndexMap;
use rattler_build_jinja::Variable;
use serde::{Deserialize, Serialize};

use super::{About, Build, Extra, Package, Requirements, Source, TestType};

/// Staging cache - a build artifact that doesn't produce a package
///
/// In multi-output recipes, staging outputs are built first and their results
/// are cached. Package outputs can then inherit from these caches, avoiding
/// redundant rebuilds of common dependencies.
///
/// A staging cache is similar to a Recipe but:
/// - Does not produce a package artifact
/// - Only has build/host requirements (no run requirements)
/// - Only has a build script (no tests, no about section)
/// - Results are cached and can be inherited by package outputs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagingCache {
    /// Name of the staging cache (unique identifier)
    pub name: String,

    /// Build configuration (only script field is used)
    #[serde(default, skip_serializing_if = "Build::is_default")]
    pub build: Build,

    /// Requirements (only build/host/ignore_run_exports are allowed)
    #[serde(default, skip_serializing_if = "Requirements::is_empty")]
    pub requirements: Requirements,

    /// Source information (can be multiple sources)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,

    /// Used variant - the subset of variant variables that were actually accessed
    #[serde(skip)]
    pub used_variant: std::collections::BTreeMap<rattler_build_types::NormalizedKey, Variable>,
}

impl StagingCache {
    /// Create a new staging cache
    pub fn new(
        name: String,
        build: Build,
        requirements: Requirements,
        source: Vec<Source>,
        used_variant: std::collections::BTreeMap<rattler_build_types::NormalizedKey, Variable>,
    ) -> Self {
        Self {
            name,
            build,
            requirements,
            source,
            used_variant,
        }
    }
}

/// Inheritance configuration for package outputs
///
/// Specifies which staging cache (if any) this recipe inherits from
/// and how run_exports should be handled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InheritsFrom {
    /// Name of the staging cache to inherit from
    pub cache_name: String,

    /// Whether to inherit run_exports from the staging cache (default: true)
    #[serde(default = "default_run_exports")]
    pub inherit_run_exports: bool,
}

fn default_run_exports() -> bool {
    true
}

impl InheritsFrom {
    /// Create a new inheritance configuration with the given cache name
    /// By default, run_exports are inherited
    pub fn new(cache_name: String) -> Self {
        Self {
            cache_name,
            inherit_run_exports: true,
        }
    }

    /// Create a new inheritance configuration with explicit run_exports setting
    pub fn with_run_exports(cache_name: String, inherit_run_exports: bool) -> Self {
        Self {
            cache_name,
            inherit_run_exports,
        }
    }
}

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
    pub context: IndexMap<String, Variable>,

    /// Staging caches that need to be built before this recipe.
    /// These are staging outputs from multi-output recipes that must be built and cached first.
    /// The results are then available when building this recipe.
    /// For single-output recipes, this will always be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staging_caches: Vec<StagingCache>,

    /// Inheritance information for multi-output recipes.
    /// Specifies which staging cache (if any) this recipe inherits from.
    /// For single-output recipes or outputs that inherit from top-level, this is None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<InheritsFrom>,

    /// Used variant - the subset of variant variables that were actually accessed
    /// during recipe evaluation (plus always-included variables like target_platform)
    #[serde(skip)]
    pub used_variant: std::collections::BTreeMap<rattler_build_types::NormalizedKey, Variable>,
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
        context: IndexMap<String, Variable>,
        used_variant: std::collections::BTreeMap<rattler_build_types::NormalizedKey, Variable>,
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
            staging_caches: Vec::new(),
            inherits_from: None,
            used_variant,
        }
    }

    /// Create a new Recipe with staging cache dependencies
    #[allow(clippy::too_many_arguments)]
    pub fn with_staging_caches(
        package: Package,
        build: Build,
        about: About,
        requirements: Requirements,
        extra: Extra,
        source: Vec<Source>,
        tests: Vec<TestType>,
        context: IndexMap<String, Variable>,
        staging_caches: Vec<StagingCache>,
        inherits_from: Option<InheritsFrom>,
        used_variant: std::collections::BTreeMap<rattler_build_types::NormalizedKey, Variable>,
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
            staging_caches,
            inherits_from,
            used_variant,
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
    pub fn context(&self) -> &IndexMap<String, Variable> {
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
            std::collections::BTreeMap::new(),
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
            std::collections::BTreeMap::new(),
        );

        assert_eq!(recipe.package(), &pkg);
        assert_eq!(recipe.build(), &build);
        assert_eq!(recipe.about(), &about);
        assert_eq!(recipe.requirements(), &reqs);
        assert_eq!(recipe.extra(), &extra);
    }
}
