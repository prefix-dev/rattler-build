///! AllOrGlobVec - A type that can be either "all" (boolean) or specific glob patterns
use std::path::Path;

use super::glob_vec::GlobVec;
use crate::ParseError;

/// A GlobVec or a boolean to select all, none, or specific paths
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllOrGlobVec {
    /// Select all paths (true) or no paths (false)
    All(bool),
    /// Select specific paths matching these glob patterns
    SpecificPaths(GlobVec),
}

impl Default for AllOrGlobVec {
    fn default() -> Self {
        Self::All(true)
    }
}

impl AllOrGlobVec {
    /// Create an AllOrGlobVec from a boolean value
    pub fn from_bool(value: bool) -> Self {
        Self::All(value)
    }

    /// Create an AllOrGlobVec from a list of glob pattern strings
    pub fn from_strings(patterns: Vec<String>) -> Result<Self, ParseError> {
        Ok(Self::SpecificPaths(GlobVec::from_strings(patterns)?))
    }

    /// Returns true if everything will be selected (All(true))
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All(true))
    }

    /// Returns true if nothing will be selected (All(false))
    pub fn is_none(&self) -> bool {
        matches!(self, Self::All(false))
    }

    /// Returns true if specific paths are configured
    pub fn is_specific_paths(&self) -> bool {
        matches!(self, Self::SpecificPaths(_))
    }

    /// Returns true if the path matches the configuration
    /// - All(true) matches everything
    /// - All(false) matches nothing
    /// - SpecificPaths(globs) matches if any glob matches
    pub fn is_match(&self, path: &Path) -> bool {
        match self {
            AllOrGlobVec::All(value) => *value,
            AllOrGlobVec::SpecificPaths(globs) => globs.is_match(path),
        }
    }

    /// Get the glob patterns if this is SpecificPaths, None otherwise
    pub fn as_glob_vec(&self) -> Option<&GlobVec> {
        match self {
            AllOrGlobVec::SpecificPaths(globs) => Some(globs),
            AllOrGlobVec::All(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_true() {
        let all = AllOrGlobVec::All(true);
        assert!(all.is_all());
        assert!(!all.is_none());
        assert!(!all.is_specific_paths());
        assert!(all.is_match(Path::new("any/path")));
    }

    #[test]
    fn test_all_false() {
        let none = AllOrGlobVec::All(false);
        assert!(!none.is_all());
        assert!(none.is_none());
        assert!(!none.is_specific_paths());
        assert!(!none.is_match(Path::new("any/path")));
    }

    #[test]
    fn test_specific_paths() {
        let specific =
            AllOrGlobVec::from_strings(vec!["bin/*".to_string(), "lib/*.so".to_string()]).unwrap();

        assert!(!specific.is_all());
        assert!(!specific.is_none());
        assert!(specific.is_specific_paths());
        assert!(specific.is_match(Path::new("bin/tool")));
        assert!(specific.is_match(Path::new("lib/foo.so")));
        assert!(!specific.is_match(Path::new("etc/config")));
    }

    #[test]
    fn test_default() {
        let default = AllOrGlobVec::default();
        assert!(default.is_all());
        assert!(default.is_match(Path::new("anything")));
    }

    #[test]
    fn test_from_bool() {
        let all_true = AllOrGlobVec::from_bool(true);
        assert!(all_true.is_all());

        let all_false = AllOrGlobVec::from_bool(false);
        assert!(all_false.is_none());
    }

    #[test]
    fn test_as_glob_vec() {
        let specific = AllOrGlobVec::from_strings(vec!["*.txt".to_string()]).unwrap();
        assert!(specific.as_glob_vec().is_some());

        let all = AllOrGlobVec::All(true);
        assert!(all.as_glob_vec().is_none());
    }
}
