//! GlobVec and related types for glob pattern matching with serialization support.

use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::Path;

use globset::{Glob, GlobBuilder, GlobSet};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize};

/// A glob with the source string preserved
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobWithSource {
    /// The compiled glob pattern
    glob: Glob,
    /// The original source string
    source: String,
}

impl GlobWithSource {
    /// Returns the compiled glob
    pub fn glob(&self) -> &Glob {
        &self.glob
    }

    /// Returns the source string
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Wrapper type to simplify serialization of Vec<GlobWithSource>
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct InnerGlobVec(Vec<GlobWithSource>);

impl Deref for InnerGlobVec {
    type Target = Vec<GlobWithSource>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl InnerGlobVec {
    fn globset(&self) -> Result<GlobSet, globset::Error> {
        let mut globset_builder = globset::GlobSetBuilder::new();
        for glob in self.iter() {
            globset_builder.add(glob.glob.clone());
        }
        globset_builder.build()
    }
}

impl From<Vec<String>> for InnerGlobVec {
    fn from(vec: Vec<String>) -> Self {
        let vec = vec
            .into_iter()
            .map(|glob| to_glob(&glob).expect("glob parsing failed"))
            .collect();
        Self(vec)
    }
}

impl From<Vec<GlobWithSource>> for InnerGlobVec {
    fn from(vec: Vec<GlobWithSource>) -> Self {
        Self(vec)
    }
}

/// Validate a glob pattern without creating a GlobWithSource
/// This is useful for early validation during parsing
pub fn validate_glob_pattern(pattern: &str) -> Result<(), globset::Error> {
    // Just try to parse it - we don't need the result
    Glob::new(pattern)?;
    Ok(())
}

/// Convert a string to a GlobWithSource, applying special folder handling
fn to_glob(glob: &str) -> Result<GlobWithSource, globset::Error> {
    // First, try to parse as a normal glob so that we get a descriptive error
    let _ = Glob::new(glob)?;

    // Strip leading "./" since paths are matched as relative (e.g. "data.txt" not "./data.txt")
    let glob_stripped = glob.strip_prefix("./").unwrap_or(glob);

    // "./" or "." means "everything" — treat as "**"
    if glob_stripped.is_empty() || glob_stripped == "." {
        return Ok(GlobWithSource {
            glob: Glob::new("**")?,
            source: glob.to_string(),
        });
    }

    if glob_stripped.ends_with('/') {
        // We treat folders as globs that match everything in the folder
        Ok(GlobWithSource {
            glob: Glob::new(&format!("{glob_stripped}**"))?,
            source: glob.to_string(),
        })
    } else {
        // Match either file, or folder
        Ok(GlobWithSource {
            glob: GlobBuilder::new(&format!("{glob_stripped}{{,/**}}"))
                .empty_alternates(true)
                .build()?,
            source: glob.to_string(),
        })
    }
}

/// A vector of globs that is also immediately converted to a globset
/// for efficient matching. Supports both include and exclude patterns.
#[derive(Default, Clone)]
pub struct GlobVec {
    include: InnerGlobVec,
    exclude: InnerGlobVec,
    include_globset: GlobSet,
    exclude_globset: GlobSet,
}

impl PartialEq for GlobVec {
    fn eq(&self, other: &Self) -> bool {
        self.include == other.include && self.exclude == other.exclude
    }
}

impl Eq for GlobVec {}

impl Serialize for InnerGlobVec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for glob in self.iter() {
            seq.serialize_element(&glob.source)?;
        }
        seq.end()
    }
}

impl Serialize for GlobVec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.exclude.is_empty() {
            self.include.serialize(serializer)
        } else {
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("include", &self.include)?;
            map.serialize_entry("exclude", &self.exclude)?;
            map.end()
        }
    }
}

impl Debug for GlobVec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.include.iter().map(|glob| glob.glob.glob()))
            .finish()
    }
}

impl<'de> Deserialize<'de> for GlobVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum GlobVecInput {
            List(Vec<String>),
            Map {
                include: Vec<String>,
                exclude: Vec<String>,
            },
        }

        let input = GlobVecInput::deserialize(deserializer)?;
        let (include, exclude) = match input {
            GlobVecInput::List(list) => (list, Vec::new()),
            GlobVecInput::Map { include, exclude } => (include, exclude),
        };

        GlobVec::new(include.into(), exclude.into())
            .map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

impl GlobVec {
    /// Create a new GlobVec from vectors of include and exclude globs
    fn new(include: InnerGlobVec, exclude: InnerGlobVec) -> Result<Self, globset::Error> {
        let include_globset = include.globset()?;
        let exclude_globset = exclude.globset()?;
        Ok(Self {
            include,
            exclude,
            include_globset,
            exclude_globset,
        })
    }

    /// Returns true if the globvec is empty (no include or exclude patterns)
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Returns the include globs
    pub fn include_globs(&self) -> &Vec<GlobWithSource> {
        &self.include
    }

    /// Returns the exclude globs
    pub fn exclude_globs(&self) -> &Vec<GlobWithSource> {
        &self.exclude
    }

    /// Returns true if the path matches any include glob and does not match any exclude glob.
    /// If there are no globs at all, we match nothing.
    /// If there is no include glob, we match everything except the exclude globs.
    pub fn is_match(&self, path: &Path) -> bool {
        // If both include & exclude are empty, we match nothing
        if self.is_empty() {
            return false;
        }
        // If include is empty, it matches everything. Otherwise we check!
        let is_match = self.include.is_empty() || self.include_globset.is_match(path);
        // If exclude is empty, it matches everything. Otherwise we check!
        is_match && (self.exclude.is_empty() || !self.exclude_globset.is_match(path))
    }

    /// Create a GlobVec from string vectors (used during evaluation/parsing)
    pub fn from_strings(
        include: Vec<String>,
        exclude: Vec<String>,
    ) -> Result<Self, globset::Error> {
        let include_vec: Vec<GlobWithSource> = include
            .into_iter()
            .map(|glob| to_glob(&glob))
            .collect::<Result<Vec<_>, _>>()?;

        let exclude_vec: Vec<GlobWithSource> = exclude
            .into_iter()
            .map(|glob| to_glob(&glob))
            .collect::<Result<Vec<_>, _>>()?;

        Self::new(InnerGlobVec(include_vec), InnerGlobVec(exclude_vec))
    }

    /// Only used for testing - create from string slices
    pub fn from_vec(include: Vec<&str>, exclude: Option<Vec<&str>>) -> Self {
        let include_vec: Vec<GlobWithSource> = include
            .into_iter()
            .map(|glob| to_glob(glob).unwrap())
            .collect();

        let exclude_vec: Vec<GlobWithSource> = exclude
            .unwrap_or_default()
            .into_iter()
            .map(|glob| to_glob(glob).unwrap())
            .collect();

        let include = InnerGlobVec(include_vec);
        let globset = include.globset().unwrap();
        let exclude = InnerGlobVec(exclude_vec);
        let exclude_globset = exclude.globset().unwrap();

        Self {
            include,
            exclude,
            include_globset: globset,
            exclude_globset,
        }
    }
}

/// A special version of GlobVec dedicated to existence/non-existence checks
/// with appropriately named fields 'exists' and 'not_exists'.
///
/// This type is used for file existence checks, particularly in package content tests.
/// - 'exists': Glob patterns that should match at least one file
/// - 'not_exists': Glob patterns that should not match any files
#[derive(Clone)]
pub struct GlobCheckerVec {
    exists: InnerGlobVec,
    not_exists: InnerGlobVec,
    exists_globset: GlobSet,
    not_exists_globset: GlobSet,
}

impl Default for GlobCheckerVec {
    fn default() -> Self {
        let empty_globset = globset::GlobSetBuilder::new().build().unwrap();
        Self {
            exists: InnerGlobVec::default(),
            not_exists: InnerGlobVec::default(),
            exists_globset: empty_globset.clone(),
            not_exists_globset: empty_globset,
        }
    }
}

impl Debug for GlobCheckerVec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.exists.is_empty() && self.not_exists.is_empty() {
            f.write_str("[]")
        } else if self.not_exists.is_empty() && !self.exists.is_empty() {
            f.debug_list()
                .entries(self.exists.iter().map(|glob| glob.glob.glob()))
                .finish()
        } else {
            let mut debug_struct = f.debug_struct("GlobCheckerVec");
            if !self.exists.is_empty() {
                debug_struct.field(
                    "exists",
                    &self
                        .exists
                        .iter()
                        .map(|g| g.glob.glob())
                        .collect::<Vec<_>>(),
                );
            }
            if !self.not_exists.is_empty() {
                debug_struct.field(
                    "not_exists",
                    &self
                        .not_exists
                        .iter()
                        .map(|g| g.glob.glob())
                        .collect::<Vec<_>>(),
                );
            }
            debug_struct.finish()
        }
    }
}

impl PartialEq for GlobCheckerVec {
    fn eq(&self, other: &Self) -> bool {
        self.exists == other.exists && self.not_exists == other.not_exists
    }
}

impl Eq for GlobCheckerVec {}

impl GlobCheckerVec {
    /// Create a new GlobCheckerVec from vectors of globs
    fn new(exists: InnerGlobVec, not_exists: InnerGlobVec) -> Result<Self, globset::Error> {
        let exists_globset = exists.globset()?;
        let not_exists_globset = not_exists.globset()?;
        Ok(Self {
            exists,
            not_exists,
            exists_globset,
            not_exists_globset,
        })
    }

    /// Static method to convert a list of globs to a GlobCheckerVec
    pub fn from_vec(exists: Vec<&str>, not_exists: Option<Vec<&str>>) -> Self {
        let exists_vec: Vec<GlobWithSource> = exists
            .into_iter()
            .map(|glob| to_glob(glob).unwrap())
            .collect();

        let not_exists_vec: Vec<GlobWithSource> = not_exists
            .unwrap_or_default()
            .into_iter()
            .map(|glob| to_glob(glob).unwrap())
            .collect();

        let exists = InnerGlobVec(exists_vec);
        let exists_globset = exists.globset().unwrap();
        let not_exists = InnerGlobVec(not_exists_vec);
        let not_exists_globset = not_exists.globset().unwrap();

        Self {
            exists,
            not_exists,
            exists_globset,
            not_exists_globset,
        }
    }

    /// Returns true if the path matches any exists glob and does not match any not_exists glob.
    /// If there are no globs at all, we match nothing.
    /// If there is no exists glob, we match everything except the not_exists globs.
    pub fn is_match(&self, path: &Path) -> bool {
        if self.exists.is_empty() && self.not_exists.is_empty() {
            return false;
        }
        let is_match = self.exists.is_empty() || self.exists_globset.is_match(path);
        is_match && (self.not_exists.is_empty() || !self.not_exists_globset.is_match(path))
    }

    /// Returns true if the checker is empty
    pub fn is_empty(&self) -> bool {
        self.exists.is_empty() && self.not_exists.is_empty()
    }

    /// Returns an iterator over the exists globs
    pub fn exists_globs(&self) -> &Vec<GlobWithSource> {
        &self.exists
    }

    /// Returns an iterator over the not_exists globs
    pub fn not_exists_globs(&self) -> &Vec<GlobWithSource> {
        &self.not_exists
    }
}

impl Serialize for GlobCheckerVec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // If only exists globs are present (no not_exists), render as a simple list
        if self.not_exists.is_empty() && !self.exists.is_empty() {
            self.exists.serialize(serializer)
        } else {
            let mut map = serializer.serialize_map(Some(2))?;
            if !self.exists.is_empty() {
                map.serialize_entry("exists", &self.exists)?;
            }
            if !self.not_exists.is_empty() {
                map.serialize_entry("not_exists", &self.not_exists)?;
            }
            map.end()
        }
    }
}

impl<'de> Deserialize<'de> for GlobCheckerVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum GlobCheckerInput {
            Map {
                #[serde(default)]
                exists: Vec<String>,
                #[serde(default)]
                not_exists: Vec<String>,
            },
            // If not specified, we will treat the list as 'exists' only
            List(Vec<String>),
        }

        let input = GlobCheckerInput::deserialize(deserializer)?;

        match input {
            GlobCheckerInput::Map { exists, not_exists } => {
                GlobCheckerVec::new(exists.into(), not_exists.into())
                    .map_err(|e| serde::de::Error::custom(e.to_string()))
            }
            GlobCheckerInput::List(list) => {
                GlobCheckerVec::new(list.into(), InnerGlobVec::default())
                    .map_err(|e| serde::de::Error::custom(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing_globvec() {
        let yaml = r#"
        - foo
        - bar
        - baz/**/qux
        "#;

        let globvec: GlobVec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(globvec.include.len(), 3);
        assert_eq!(globvec.include_globset.len(), 3);

        let as_yaml = serde_yaml::to_string(&globvec).unwrap();
        insta::assert_snapshot!("globvec_simple", &as_yaml);
        let parsed_again: GlobVec = serde_yaml::from_str(&as_yaml).unwrap();
        assert_eq!(parsed_again.include.len(), 3);
        assert_eq!(parsed_again.include_globset.len(), 3);

        let yaml = r#"
        include: ["foo/", "bar", "baz/**/qux"]
        exclude: ["foo/bar", "bar/*.txt"]
        "#;

        let globvec: GlobVec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(globvec.include.len(), 3);
        assert_eq!(globvec.include_globset.len(), 3);
        assert_eq!(globvec.exclude.len(), 2);
        assert_eq!(globvec.exclude_globset.len(), 2);

        let as_yaml = serde_yaml::to_string(&globvec).unwrap();
        insta::assert_snapshot!("globvec_with_exclude", &as_yaml);
        let parsed_again: GlobVec = serde_yaml::from_str(&as_yaml).unwrap();
        assert_eq!(parsed_again.include.len(), 3);
        assert_eq!(parsed_again.include_globset.len(), 3);
        assert_eq!(parsed_again.exclude.len(), 2);
        assert_eq!(parsed_again.exclude_globset.len(), 2);
    }

    #[test]
    fn test_glob_match_folder() {
        let globvec = GlobVec::from_vec(vec!["foo/"], None);
        assert!(globvec.is_match(Path::new("foo/bar")));
        assert!(globvec.is_match(Path::new("foo/bla")));
        assert!(globvec.is_match(Path::new("foo/bla/bar")));
        assert!(!globvec.is_match(Path::new("bar")));
        assert!(!globvec.is_match(Path::new("bla")));

        let globvec = GlobVec::from_vec(vec!["foo"], None);
        assert!(globvec.is_match(Path::new("foo/bar")));
        assert!(globvec.is_match(Path::new("foo/bla")));
        assert!(globvec.is_match(Path::new("foo/bla/bar")));
        assert!(!globvec.is_match(Path::new("bar")));
        assert!(!globvec.is_match(Path::new("bla")));
    }

    #[test]
    fn test_glob_dot_slash_matches_everything() {
        // "./" and "." should match all files, just like "**" (issue #2085)
        for pattern in &["./", "."] {
            let globvec = GlobVec::from_vec(vec![pattern], None);
            assert!(globvec.is_match(Path::new("data.txt")), "{pattern} should match data.txt");
            assert!(globvec.is_match(Path::new("sub/file.txt")), "{pattern} should match sub/file.txt");
            assert!(globvec.is_match(Path::new("a/b/c")), "{pattern} should match a/b/c");
        }

        // "./subdir/" should match files inside subdir
        let globvec = GlobVec::from_vec(vec!["./subdir/"], None);
        assert!(globvec.is_match(Path::new("subdir/file.txt")));
        assert!(!globvec.is_match(Path::new("other/file.txt")));
    }

    #[test]
    fn test_glob_match_all_except() {
        let globvec = GlobVec::from_vec(vec!["**"], Some(vec!["*.txt"]));
        assert!(!globvec.is_match(Path::new("foo/bar.txt")));
        assert!(globvec.is_match(Path::new("foo/bla")));
        assert!(globvec.is_match(Path::new("foo/bla/bar")));
        assert!(!globvec.is_match(Path::new("bar.txt")));
        assert!(globvec.is_match(Path::new("bla")));

        // empty include should be the same
        let globvec = GlobVec::from_vec(vec![], Some(vec!["*.txt"]));
        assert!(!globvec.is_match(Path::new("foo/bar.txt")));
        assert!(globvec.is_match(Path::new("foo/bla")));
        assert!(globvec.is_match(Path::new("foo/bla/bar")));
        assert!(!globvec.is_match(Path::new("bar.txt")));
        assert!(globvec.is_match(Path::new("bla")));

        // empty everything should match nothing
        let globvec = GlobVec::from_vec(vec![], None);
        assert!(!globvec.is_match(Path::new("foo/bar.txt")));
    }

    #[test]
    fn test_parsing_glob_checker_vec() {
        let yaml = r#"
        exists: ["foo", "bar"]
        not_exists: ["baz/**/qux"]
        "#;

        let glob_checker: GlobCheckerVec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(glob_checker.exists_globs().len(), 2);
        assert_eq!(glob_checker.not_exists_globs().len(), 1);

        // Test the paths match correctly
        assert!(glob_checker.is_match(Path::new("foo")));
        assert!(glob_checker.is_match(Path::new("bar")));
        assert!(!glob_checker.is_match(Path::new("baz/qux")));
        assert!(!glob_checker.is_match(Path::new("baz/some/qux")));

        // Test serialization
        let yaml = serde_yaml::to_string(&glob_checker).unwrap();
        insta::assert_snapshot!("glob_checker_vec", &yaml);

        // Test default values (empty arrays)
        let empty_yaml = "{}";
        let empty_checker: GlobCheckerVec = serde_yaml::from_str(empty_yaml).unwrap();
        assert!(empty_checker.is_empty());
        assert_eq!(empty_checker.exists_globs().len(), 0);
        assert_eq!(empty_checker.not_exists_globs().len(), 0);

        // Test backward compatibility - plain list of globs should be treated as `exists`
        let plain_list = r#"["foo", "bar"]"#;
        let legacy_checker: GlobCheckerVec = serde_yaml::from_str(plain_list).unwrap();
        assert_eq!(legacy_checker.exists_globs().len(), 2);
        assert_eq!(legacy_checker.not_exists_globs().len(), 0);
        assert!(legacy_checker.is_match(Path::new("foo")));
        assert!(legacy_checker.is_match(Path::new("bar")));
        assert!(!legacy_checker.is_match(Path::new("baz")));
    }

    #[test]
    fn test_serialize_only_exists_globs_as_list() {
        let checker = GlobCheckerVec::from_vec(vec!["foo", "bar"], None);
        let yaml = serde_yaml::to_string(&checker).unwrap();
        assert_eq!(yaml, "- foo\n- bar\n");
    }
}
