//! Subpackage support (melange-style file splitting)
//!
//! A subpackage splits a set of files off of its owning output. The owning
//! output keeps the remainder of the built files. Subpackages share the
//! parent's single build, but can declare their own runtime requirements,
//! about metadata and tests, and can be referenced via `pin_subpackage`.
//!
//! See `design/subpackages.md` for the full design.

use serde::Serialize;

use crate::stage0::{
    about::About,
    output::{Output, Recipe},
    package::PackageMetadata,
    requirements::Requirements,
    tests::TestType,
    types::{ConditionalList, IncludeExclude},
};

/// Returns true if any output in the recipe declares `subpackages`.
pub fn recipe_has_subpackages(recipe: &Recipe) -> bool {
    match recipe {
        Recipe::SingleOutput(single) => !single.subpackages.is_empty(),
        Recipe::MultiOutput(multi) => multi.outputs.iter().any(
            |output| matches!(output, Output::Package(p) if !p.subpackages.is_empty()),
        ),
    }
}

/// A subpackage of an output.
///
/// Files matching `files` are split off from the owning output into this
/// subpackage. The version (if omitted) and about metadata (for unset fields)
/// are inherited from the parent output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Subpackage {
    /// Package metadata (name is required, version is optional and inherits
    /// from the parent output when omitted).
    pub package: PackageMetadata,

    /// Globs selecting which of the built files this subpackage claims.
    ///
    /// Files are claimed in subpackage declaration order (first-match-wins);
    /// anything left unclaimed stays with the parent output.
    #[serde(default)]
    pub files: IncludeExclude,

    /// Requirements for this subpackage.
    ///
    /// Only `run`, `run_constraints`, `run_exports` and `ignore_run_exports`
    /// are meaningful here — a subpackage does not run its own build, so
    /// `build`/`host` requirements are rejected at parse time.
    #[serde(default)]
    pub requirements: Requirements,

    /// About metadata for this subpackage. Unset fields inherit from the
    /// parent output's about section.
    #[serde(default)]
    pub about: About,

    /// Tests for this subpackage.
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub tests: ConditionalList<TestType>,
}

impl Subpackage {
    /// Get all variables used in this subpackage (for variant computation).
    pub fn used_variables(&self) -> Vec<String> {
        let Subpackage {
            package,
            files,
            requirements,
            about,
            tests,
        } = self;

        let mut vars = package.used_variables();
        vars.extend(files.used_variables());
        vars.extend(requirements.used_variables());
        vars.extend(about.used_variables());
        for test_item in tests {
            vars.extend(super::output::collect_test_item_variables(test_item));
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
