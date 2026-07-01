//! A path that may reference a restricted set of build-time directory
//! variables that are only known once the build directories have been created.
//!
//! Recipe fields like `source.patches` and `about.license_file` are rendered
//! during the recipe evaluation stage, long before the build directories
//! (`SRC_DIR`, `PREFIX`, ...) exist. To allow these fields to point at files
//! inside those directories, a small allow-list of variables is kept as literal
//! `${{ VAR }}` tokens after rendering and substituted at build time via
//! [`LateBoundPath::resolve`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Names of the build-time directory variables that may be referenced from a
/// [`LateBoundPath`].
pub mod vars {
    /// The source working directory (`$SRC_DIR`).
    pub const SRC_DIR: &str = "SRC_DIR";
    /// The recipe directory (`$RECIPE_DIR`).
    pub const RECIPE_DIR: &str = "RECIPE_DIR";
    /// The top-level build directory (`$BUILD_DIR`).
    pub const BUILD_DIR: &str = "BUILD_DIR";
    /// The host prefix (`$PREFIX`).
    pub const PREFIX: &str = "PREFIX";
    /// The build prefix (`$BUILD_PREFIX`).
    pub const BUILD_PREFIX: &str = "BUILD_PREFIX";
}

/// Variables that may be referenced from `source.patches`.
///
/// Patches are applied while sources are being fetched, before the build script
/// runs, so only directories that exist at that point are allowed (the host and
/// build prefixes are not yet populated).
pub const PATCH_VARS: &[&str] = &[vars::SRC_DIR, vars::RECIPE_DIR, vars::BUILD_DIR];

/// Variables that may be referenced from `about.license_file`.
///
/// License files are collected during packaging, after the build script has
/// run, so the host and build prefixes are available as well.
pub const LICENSE_VARS: &[&str] = &[
    vars::PREFIX,
    vars::BUILD_PREFIX,
    vars::SRC_DIR,
    vars::RECIPE_DIR,
    vars::BUILD_DIR,
];

/// The union of all variables that can appear in a [`LateBoundPath`].
pub const ALL_VARS: &[&str] = LICENSE_VARS;

/// A path that may reference a restricted set of build-time directory variables
/// (e.g. `${{ SRC_DIR }}`, `${{ PREFIX }}`).
///
/// The variables are kept as literal `${{ VAR }}` tokens after the recipe has
/// been rendered and are substituted with concrete paths at build time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LateBoundPath(String);

impl LateBoundPath {
    /// Create a new late-bound path from a (already rendered) template string.
    pub fn new(template: impl Into<String>) -> Self {
        Self(template.into())
    }

    /// The raw template string, with any late-bound `${{ VAR }}` tokens preserved.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The canonical token string for a variable name, e.g. `${{ SRC_DIR }}`.
    pub fn token(name: &str) -> String {
        format!("${{{{ {name} }}}}")
    }

    /// Returns `true` if the path still contains a late-bound variable token.
    pub fn is_late_bound(&self) -> bool {
        ALL_VARS.iter().any(|v| self.0.contains(&Self::token(v)))
    }

    /// Resolve the late-bound tokens using the provided lookup of variable name
    /// to directory, returning the concrete path.
    ///
    /// Tokens whose variable is not returned by `lookup` are left untouched.
    pub fn resolve(&self, lookup: impl Fn(&str) -> Option<PathBuf>) -> PathBuf {
        let mut rendered = self.0.clone();
        for var in ALL_VARS {
            let token = Self::token(var);
            if rendered.contains(&token)
                && let Some(path) = lookup(var)
            {
                rendered = rendered.replace(&token, &path.to_string_lossy());
            }
        }
        PathBuf::from(rendered)
    }
}

impl std::fmt::Display for LateBoundPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for LateBoundPath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for LateBoundPath {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token() {
        assert_eq!(LateBoundPath::token("SRC_DIR"), "${{ SRC_DIR }}");
    }

    #[test]
    fn test_is_late_bound() {
        assert!(LateBoundPath::new("${{ SRC_DIR }}/foo.patch").is_late_bound());
        assert!(LateBoundPath::new("${{ PREFIX }}/share/licenses/LICENSE").is_late_bound());
        assert!(!LateBoundPath::new("patches/foo.patch").is_late_bound());
        // Unknown variables are not considered late-bound tokens.
        assert!(!LateBoundPath::new("${{ UNKNOWN }}/foo").is_late_bound());
    }

    #[test]
    fn test_resolve() {
        let path = LateBoundPath::new("${{ SRC_DIR }}/src/patches/0001.patch");
        let resolved = path.resolve(|var| match var {
            "SRC_DIR" => Some(PathBuf::from("/tmp/work")),
            _ => None,
        });
        assert_eq!(resolved, PathBuf::from("/tmp/work/src/patches/0001.patch"));
    }

    #[test]
    fn test_resolve_unknown_left_untouched() {
        let path = LateBoundPath::new("relative/path.patch");
        let resolved = path.resolve(|_| Some(PathBuf::from("/tmp/work")));
        assert_eq!(resolved, PathBuf::from("relative/path.patch"));
    }

    #[test]
    fn test_resolve_prefix() {
        let path = LateBoundPath::new("${{ PREFIX }}/lib/R/library/foo/LICENSE");
        let resolved = path.resolve(|var| match var {
            "PREFIX" => Some(PathBuf::from("/opt/conda/h_env")),
            _ => None,
        });
        assert_eq!(
            resolved,
            PathBuf::from("/opt/conda/h_env/lib/R/library/foo/LICENSE")
        );
    }
}
