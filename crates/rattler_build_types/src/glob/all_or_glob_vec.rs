//! AllOrGlobVec type for selecting all, none, or specific paths

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::GlobVec;

/// A GlobVec or a boolean to select all, none, or specific paths.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum AllOrGlobVec {
    /// Relocate all binaries.
    All(bool),
    /// Relocate specific paths.
    SpecificPaths(GlobVec),
}

impl Default for AllOrGlobVec {
    fn default() -> Self {
        Self::All(true)
    }
}

impl AllOrGlobVec {
    /// Returns true if everything will be selected
    pub fn is_all(&self) -> bool {
        self == &Self::All(true)
    }

    /// Returns true if no path will be selected
    pub fn is_none(&self) -> bool {
        self == &Self::All(false)
    }

    /// Returns true if the path matches any of the globs or if all is selected
    pub fn is_match(&self, p: &Path) -> bool {
        match self {
            AllOrGlobVec::All(val) => *val,
            AllOrGlobVec::SpecificPaths(globs) => globs.is_match(p),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing_all_or_globvec() {
        let yaml = r#"
        - foo
        - bar
        - baz/**/qux
        "#;

        let all_or_globvec: AllOrGlobVec = serde_yaml::from_str(yaml).unwrap();
        assert!(all_or_globvec.is_match(Path::new("foo")));
        assert!(all_or_globvec.is_match(Path::new("bar")));
        assert!(all_or_globvec.is_match(Path::new("baz/qux")));
        assert!(all_or_globvec.is_match(Path::new("baz/bla/qux")));
        assert!(!all_or_globvec.is_match(Path::new("bla")));
        assert!(!all_or_globvec.is_match(Path::new("bla/qux")));

        let as_yaml = serde_yaml::to_string(&all_or_globvec).unwrap();
        insta::assert_snapshot!("all_or_globvec_specific", &as_yaml);
        let parsed_again: AllOrGlobVec = serde_yaml::from_str(&as_yaml).unwrap();
        assert_eq!(parsed_again, all_or_globvec);

        let all_or_globvec: AllOrGlobVec = serde_yaml::from_str("true").unwrap();
        assert!(all_or_globvec.is_match(Path::new("foo")));
        assert!(all_or_globvec.is_all());

        let globs_all = serde_yaml::to_string(&all_or_globvec).unwrap();
        assert_eq!(globs_all, "true\n");

        let all_or_globvec: AllOrGlobVec = serde_yaml::from_str("false").unwrap();
        assert!(!all_or_globvec.is_match(Path::new("foo")));
        assert!(all_or_globvec.is_none());

        let globs_none = serde_yaml::to_string(&all_or_globvec).unwrap();
        assert_eq!(globs_none, "false\n");
    }
}
