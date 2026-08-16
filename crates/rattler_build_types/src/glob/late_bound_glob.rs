//! A glob pattern that may reference late-bound build-directory variables.
//!
//! Recipe fields like `about.license_file` accept a mix of ordinary glob
//! patterns (matched against the work and recipe directories during packaging)
//! and patterns that reference build directory variables that only exist once
//! the build has started (e.g. `${{ PREFIX }}/share/licenses/*`). Rather than
//! splitting those into two separate fields, both kinds are represented by a
//! single [`LateBoundGlob`] element and collected in a [`LateBoundGlobVec`], so
//! callers and the rendered recipe see one unified list.
//!
//! An ordinary glob is simply a [`LateBoundGlob`] with no late-bound tokens: it
//! is validated and compiled eagerly. A pattern containing `${{ VAR }}` tokens
//! is kept as a template and only resolved to a concrete path — and then
//! matched as a glob — at packaging time (see [`LateBoundPath::resolve`]).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::LateBoundPath;
use crate::glob::GlobVec;

/// A single glob pattern that may reference late-bound build-directory
/// variables (e.g. `${{ PREFIX }}/share/licenses/*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateBoundGlob {
    /// The original pattern string, with any `${{ VAR }}` tokens preserved.
    source: String,
    /// Whether the pattern still contains late-bound variable tokens and thus
    /// has to be resolved before it can be matched as a glob.
    late_bound: bool,
}

impl LateBoundGlob {
    /// Create a new entry from an (already rendered) pattern string.
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let late_bound = LateBoundPath::new(source.as_str()).is_late_bound();
        Self { source, late_bound }
    }

    /// The raw pattern string, with any late-bound `${{ VAR }}` tokens preserved.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns `true` if the pattern references late-bound build directory
    /// variables and must be resolved before matching.
    pub fn is_late_bound(&self) -> bool {
        self.late_bound
    }

    /// Resolve the late-bound tokens using the provided variable lookup,
    /// returning the concrete path. Tokens whose variable is not returned by
    /// `lookup` are left untouched; token-free patterns are returned as-is.
    pub fn resolve(&self, lookup: impl Fn(&str) -> Option<PathBuf>) -> PathBuf {
        LateBoundPath::new(self.source.as_str()).resolve(lookup)
    }
}

/// A collection of [`LateBoundGlob`] patterns, mixing ordinary globs and
/// late-bound patterns in a single, order-preserving list (with optional
/// excludes).
///
/// Internally the token-free entries are also compiled into a [`GlobVec`] so
/// they can be matched efficiently, while the late-bound entries are kept for
/// resolution at packaging time. This split is a private implementation detail:
/// the public API and the serialized form expose a single list of patterns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LateBoundGlobVec {
    /// All include entries, in the order they were declared.
    include: Vec<LateBoundGlob>,
    /// Exclude patterns (ordinary globs).
    exclude: Vec<String>,
    /// The token-free include entries compiled into a glob set (with the
    /// excludes applied), for efficient matching and reuse of the existing
    /// glob-copy machinery.
    ordinary: GlobVec,
}

impl LateBoundGlobVec {
    /// Build a [`LateBoundGlobVec`] from include and exclude pattern strings.
    ///
    /// Token-free include patterns (and all excludes) are validated and
    /// compiled up front; entries containing `${{ VAR }}` tokens are kept as
    /// late-bound templates.
    pub fn from_sources(
        include: Vec<String>,
        exclude: Vec<String>,
    ) -> Result<Self, globset::Error> {
        let include: Vec<LateBoundGlob> = include.into_iter().map(LateBoundGlob::new).collect();

        // Only the token-free entries can be compiled into a glob set now.
        let ordinary_sources: Vec<String> = include
            .iter()
            .filter(|g| !g.is_late_bound())
            .map(|g| g.source().to_string())
            .collect();
        let ordinary = GlobVec::from_strings(ordinary_sources, exclude.clone())?;

        Ok(Self {
            include,
            exclude,
            ordinary,
        })
    }

    /// Returns `true` if there are no include or exclude patterns.
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// All include entries, in declaration order.
    pub fn entries(&self) -> &[LateBoundGlob] {
        &self.include
    }

    /// The exclude patterns.
    pub fn exclude(&self) -> &[String] {
        &self.exclude
    }

    /// The token-free include entries compiled into a [`GlobVec`] (with
    /// excludes applied), for ordinary glob matching / copying.
    pub fn ordinary_globs(&self) -> &GlobVec {
        &self.ordinary
    }

    /// The late-bound entries (those referencing `${{ VAR }}` tokens), which
    /// must be resolved to concrete paths before matching.
    pub fn late_bound(&self) -> impl Iterator<Item = &LateBoundGlob> {
        self.include.iter().filter(|g| g.is_late_bound())
    }
}

/// The serialized shape of a [`LateBoundGlobVec`]: either a plain list of
/// patterns or an include/exclude map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum LateBoundGlobVecRepr {
    List(Vec<String>),
    Map {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        include: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exclude: Vec<String>,
    },
}

impl Serialize for LateBoundGlobVec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let include: Vec<String> = self
            .include
            .iter()
            .map(|g| g.source().to_string())
            .collect();
        let repr = if self.exclude.is_empty() {
            LateBoundGlobVecRepr::List(include)
        } else {
            LateBoundGlobVecRepr::Map {
                include,
                exclude: self.exclude.clone(),
            }
        };
        repr.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LateBoundGlobVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (include, exclude) = match LateBoundGlobVecRepr::deserialize(deserializer)? {
            LateBoundGlobVecRepr::List(list) => (list, Vec::new()),
            LateBoundGlobVecRepr::Map { include, exclude } => (include, exclude),
        };
        LateBoundGlobVec::from_sources(include, exclude)
            .map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_late_bound_glob_classification() {
        assert!(!LateBoundGlob::new("LICENSE").is_late_bound());
        assert!(!LateBoundGlob::new("licenses/*.txt").is_late_bound());
        assert!(LateBoundGlob::new("${{ PREFIX }}/share/licenses/LICENSE").is_late_bound());
    }

    #[test]
    fn test_partition_and_order() {
        let vec = LateBoundGlobVec::from_sources(
            vec![
                "LICENSE".to_string(),
                "${{ PREFIX }}/share/licenses/foo".to_string(),
                "licenses/*.txt".to_string(),
            ],
            Vec::new(),
        )
        .unwrap();

        // Order is preserved across both kinds.
        let sources: Vec<_> = vec.entries().iter().map(|g| g.source()).collect();
        assert_eq!(
            sources,
            vec![
                "LICENSE",
                "${{ PREFIX }}/share/licenses/foo",
                "licenses/*.txt"
            ]
        );

        // Only the token-free entries are compiled into the glob set.
        assert_eq!(vec.ordinary_globs().include_globs().len(), 2);
        assert_eq!(vec.late_bound().count(), 1);
    }

    #[test]
    fn test_serde_round_trip_single_key() {
        let vec = LateBoundGlobVec::from_sources(
            vec![
                "LICENSE".to_string(),
                "${{ PREFIX }}/share/licenses/foo".to_string(),
            ],
            Vec::new(),
        )
        .unwrap();

        let yaml = serde_yaml::to_string(&vec).unwrap();
        assert!(
            !yaml.contains("late_bound"),
            "leaked late-bound key:\n{yaml}"
        );

        let parsed: LateBoundGlobVec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, vec);
    }

    #[test]
    fn test_serde_with_exclude() {
        let vec = LateBoundGlobVec::from_sources(
            vec![
                "licenses/*".to_string(),
                "${{ SRC_DIR }}/LICENSE".to_string(),
            ],
            vec!["licenses/skip".to_string()],
        )
        .unwrap();

        let yaml = serde_yaml::to_string(&vec).unwrap();
        let parsed: LateBoundGlobVec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, vec);
        assert_eq!(parsed.exclude(), &["licenses/skip".to_string()]);
    }
}
