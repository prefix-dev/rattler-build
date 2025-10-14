//! Multi-output recipe support
//!
//! This module defines the types for multi-output recipes, which allow building
//! multiple packages from a single recipe with staging/caching support.

use serde::Serialize;

use crate::stage0::{
    about::About,
    build::Build,
    package::{Package, PackageMetadata},
    requirements::Requirements,
    source::Source,
    tests::TestType,
    types::Value,
};

/// A recipe can be either a single-output or multi-output recipe
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Recipe {
    /// Traditional single-output recipe
    SingleOutput(Box<SingleOutputRecipe>),
    /// Multi-output recipe with staging support
    MultiOutput(Box<MultiOutputRecipe>),
}

/// Traditional single-output recipe (what we had before)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SingleOutputRecipe {
    /// Schema version (optional, defaults to None). Only version 1 is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,

    /// Context variables for Jinja template rendering (order-preserving)
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub context: indexmap::IndexMap<String, Value<String>>,

    pub package: Package,
    pub build: Build,
    pub requirements: Requirements,
    pub about: About,
    pub extra: crate::stage0::extra::Extra,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestType>,
}

/// Multi-output recipe with staging support
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MultiOutputRecipe {
    /// Schema version (optional, defaults to None). Only version 1 is supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,

    /// Context variables for Jinja template rendering (order-preserving)
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub context: indexmap::IndexMap<String, Value<String>>,

    /// Recipe metadata (name is optional, version is required)
    pub recipe: RecipeMetadata,

    /// Top-level source (inheritable by outputs)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,

    /// Top-level build configuration (inheritable by outputs)
    #[serde(default)]
    pub build: Build,

    /// Top-level about information (inheritable by outputs)
    #[serde(default)]
    pub about: About,

    /// Extra metadata
    #[serde(default)]
    pub extra: crate::stage0::extra::Extra,

    /// List of outputs (staging and package outputs)
    pub outputs: Vec<Output>,
}

/// Recipe-level metadata (replaces top-level package in multi-output recipes)
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct RecipeMetadata {
    /// Package name (optional - can be omitted if only used for grouping outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Value<crate::stage0::package::PackageName>>,

    /// Version (optional - can be inherited by each output from their package metadata)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Value<rattler_conda_types::VersionWithSource>>,
}

/// An output can be either a staging output or a package output
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Output {
    /// Staging output - builds once and caches results
    Staging(Box<StagingOutput>),
    /// Package output - produces a package artifact
    Package(Box<PackageOutput>),
}

/// Staging output configuration
///
/// A staging output builds code once and caches the results.
/// Other outputs can inherit from this staging cache.
/// Staging outputs do not produce package artifacts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StagingOutput {
    /// Staging metadata (name is required)
    pub staging: StagingMetadata,

    /// Source for this staging build
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,

    /// Requirements for staging build (only build/host/ignore_run_exports allowed)
    #[serde(default)]
    pub requirements: Requirements,

    /// Build configuration (only script is allowed)
    #[serde(default)]
    pub build: StagingBuild,
}

/// Staging metadata
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StagingMetadata {
    /// Name of the staging cache (required, must follow PackageName rules)
    pub name: Value<String>,
}

/// Build configuration for staging outputs
///
/// Only the script field is allowed for staging outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct StagingBuild {
    /// Build script - either inline commands or a file path
    #[serde(
        default,
        skip_serializing_if = "crate::stage0::types::ConditionalList::is_empty"
    )]
    pub script: crate::stage0::types::ConditionalList<crate::stage0::types::ScriptContent>,
}

/// Package output configuration
///
/// A package output produces a package artifact and can inherit from
/// a staging cache or from the top-level recipe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PackageOutput {
    /// Package metadata (version is optional, can be inherited from recipe)
    pub package: PackageMetadata,

    /// What to inherit from (staging cache or top-level)
    #[serde(default)]
    pub inherit: Inherit,

    /// Source for this output (in addition to or instead of inherited source)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<Source>,

    /// Requirements for this output
    #[serde(default)]
    pub requirements: Requirements,

    /// Build configuration for this output
    #[serde(default)]
    pub build: Build,

    /// About information for this output
    #[serde(default)]
    pub about: About,

    /// Tests for this output
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestType>,
}

/// Serialize TopLevel as null
fn serialize_top_level<S>(serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_none()
}

/// Inheritance configuration
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(untagged)]
pub enum Inherit {
    /// Inherit from a named staging cache (short form: just the name)
    CacheName(Value<String>),

    /// Inherit from a staging cache with options (long form: mapping)
    CacheWithOptions(CacheInherit),

    /// Inherit from top-level (null or omitted)
    #[default]
    #[serde(serialize_with = "serialize_top_level")]
    TopLevel,
}

/// Cache inheritance with options
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CacheInherit {
    /// Name of the staging cache to inherit from
    pub from: Value<String>,

    /// Whether to inherit run_exports (default: true)
    pub run_exports: bool,
}

impl Recipe {
    /// Get all used variables across the entire recipe
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Recipe::SingleOutput(single) => single.as_ref().used_variables(),
            Recipe::MultiOutput(multi) => multi.as_ref().used_variables(),
        }
    }

    /// Get all free specs (specs without version or build constraints) across the entire recipe
    pub fn free_specs(&self) -> Vec<rattler_conda_types::PackageName> {
        match self {
            Recipe::SingleOutput(single) => single.as_ref().free_specs(),
            Recipe::MultiOutput(multi) => multi.as_ref().free_specs(),
        }
    }
}

impl SingleOutputRecipe {
    /// Get all used variables in this single-output recipe
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = self.package.used_variables();
        vars.extend(self.build.used_variables());
        vars.extend(self.requirements.used_variables());
        vars.extend(self.about.used_variables());
        vars.extend(self.extra.used_variables());
        for src in &self.source {
            vars.extend(src.used_variables());
        }
        for test in &self.tests {
            vars.extend(test.used_variables());
        }
        for value in self.context.values() {
            vars.extend(value.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }

    /// Get all free specs (specs without version or build constraints) in this single-output recipe
    pub fn free_specs(&self) -> Vec<rattler_conda_types::PackageName> {
        self.requirements.free_specs()
    }
}

impl MultiOutputRecipe {
    /// Get all used variables across recipe and all outputs
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        // Top-level variables
        if let Some(name) = &self.recipe.name {
            vars.extend(name.used_variables());
        }
        if let Some(version) = &self.recipe.version {
            vars.extend(version.used_variables());
        }
        vars.extend(self.build.used_variables());
        vars.extend(self.about.used_variables());
        vars.extend(self.extra.used_variables());
        for src in &self.source {
            vars.extend(src.used_variables());
        }

        // Context variables
        for value in self.context.values() {
            vars.extend(value.used_variables());
        }

        // Output variables
        for output in &self.outputs {
            vars.extend(output.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }

    /// Get all free specs (specs without version or build constraints) across all outputs
    pub fn free_specs(&self) -> Vec<rattler_conda_types::PackageName> {
        let mut specs = Vec::new();

        // Collect free specs from all outputs
        for output in &self.outputs {
            specs.extend(output.free_specs());
        }

        specs.sort();
        specs.dedup();
        specs
    }
}

impl Output {
    /// Get all used variables in this output
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Output::Staging(staging) => staging.as_ref().used_variables(),
            Output::Package(package) => package.as_ref().used_variables(),
        }
    }

    /// Get all free specs (specs without version or build constraints) in this output
    pub fn free_specs(&self) -> Vec<rattler_conda_types::PackageName> {
        match self {
            Output::Staging(staging) => staging.as_ref().free_specs(),
            Output::Package(package) => package.as_ref().free_specs(),
        }
    }
}

impl StagingOutput {
    /// Get all used variables in this staging output
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = self.staging.name.used_variables();
        for src in &self.source {
            vars.extend(src.used_variables());
        }
        vars.extend(self.requirements.used_variables());
        vars.extend(self.build.used_variables());
        vars.sort();
        vars.dedup();
        vars
    }

    /// Get all free specs (specs without version or build constraints) in this staging output
    pub fn free_specs(&self) -> Vec<rattler_conda_types::PackageName> {
        self.requirements.free_specs()
    }
}

impl PackageOutput {
    /// Get all used variables in this package output
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = self.package.used_variables();
        vars.extend(self.inherit.used_variables());
        for src in &self.source {
            vars.extend(src.used_variables());
        }
        vars.extend(self.requirements.used_variables());
        vars.extend(self.build.used_variables());
        vars.extend(self.about.used_variables());
        for test in &self.tests {
            vars.extend(test.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }

    /// Get all free specs (specs without version or build constraints) in this package output
    pub fn free_specs(&self) -> Vec<rattler_conda_types::PackageName> {
        self.requirements.free_specs()
    }
}

impl Inherit {
    /// Get all used variables in this inheritance configuration
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Inherit::TopLevel => Vec::new(),
            Inherit::CacheName(name) => name.used_variables(),
            Inherit::CacheWithOptions(options) => options.from.used_variables(),
        }
    }
}

impl StagingBuild {
    /// Get all used variables in staging build
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for item in &self.script {
            vars.extend(item.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
