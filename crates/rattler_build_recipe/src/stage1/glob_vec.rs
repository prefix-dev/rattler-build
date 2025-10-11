///! Validated glob patterns for file matching
use std::path::Path;

use globset::{Glob, GlobBuilder, GlobSet};
use serde::{Deserialize, Serialize};

use crate::{ErrorKind, ParseError, Span};

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

/// A vector of validated glob patterns with a compiled GlobSet for efficient matching
#[derive(Debug, Clone, Default)]
pub struct GlobVec {
    /// The list of globs with their source strings
    globs: Vec<GlobWithSource>,
    /// Compiled globset for efficient matching
    globset: GlobSet,
}

impl Serialize for GlobVec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as a list of source strings
        let sources: Vec<&str> = self.globs.iter().map(|g| g.source()).collect();
        sources.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GlobVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let sources = Vec::<String>::deserialize(deserializer)?;
        GlobVec::from_strings(sources).map_err(serde::de::Error::custom)
    }
}

impl PartialEq for GlobVec {
    fn eq(&self, other: &Self) -> bool {
        // Compare only the globs, not the compiled globset
        self.globs == other.globs
    }
}

impl Eq for GlobVec {}

impl GlobVec {
    /// Create a new GlobVec from a list of glob pattern strings
    /// Returns an error if any pattern is invalid
    pub fn from_strings(patterns: Vec<String>) -> Result<Self, ParseError> {
        if patterns.is_empty() {
            return Ok(Self::default());
        }

        let mut globs = Vec::with_capacity(patterns.len());
        let mut globset_builder = globset::GlobSetBuilder::new();

        for pattern in patterns {
            let glob = parse_glob(&pattern).map_err(|err| ParseError {
                kind: ErrorKind::InvalidValue,
                span: Span::unknown(),
                message: Some(format!("Invalid glob pattern '{}': {}", pattern, err)),
                suggestion: None,
            })?;

            globset_builder.add(glob.glob.clone());
            globs.push(glob);
        }

        let globset = globset_builder.build().map_err(|err| ParseError {
            kind: ErrorKind::InvalidValue,
            span: Span::unknown(),
            message: Some(format!("Failed to build globset: {}", err)),
            suggestion: None,
        })?;

        Ok(Self { globs, globset })
    }

    /// Returns true if the GlobVec is empty
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty()
    }

    /// Returns the number of glob patterns
    pub fn len(&self) -> usize {
        self.globs.len()
    }

    /// Returns an iterator over the globs
    pub fn iter(&self) -> impl Iterator<Item = &GlobWithSource> {
        self.globs.iter()
    }

    /// Returns true if the path matches any of the glob patterns
    pub fn is_match(&self, path: &Path) -> bool {
        if self.is_empty() {
            return false;
        }
        self.globset.is_match(path)
    }

    /// Returns the source strings of all glob patterns
    pub fn sources(&self) -> Vec<&str> {
        self.globs.iter().map(|g| g.source()).collect()
    }

    /// Convert back to a Vec of source strings
    pub fn to_strings(&self) -> Vec<String> {
        self.globs.iter().map(|g| g.source().to_string()).collect()
    }
}

/// Parse a glob pattern string into a GlobWithSource
///
/// This function applies special handling:
/// - Patterns ending with '/' are treated as directories (appends '**')
/// - Other patterns match both files and directories (appends '{,/**}')
fn parse_glob(pattern: &str) -> Result<GlobWithSource, globset::Error> {
    // First validate the pattern is valid
    let _ = Glob::new(pattern)?;

    if pattern.ends_with('/') {
        // Directory pattern: match everything inside
        Ok(GlobWithSource {
            glob: Glob::new(&format!("{}**", pattern))?,
            source: pattern.to_string(),
        })
    } else {
        // Match either file or directory
        Ok(GlobWithSource {
            glob: GlobBuilder::new(&format!("{}{{,/**}}", pattern))
                .empty_alternates(true)
                .build()?,
            source: pattern.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_globvec() {
        let globvec = GlobVec::from_strings(vec![]).unwrap();
        assert!(globvec.is_empty());
        assert_eq!(globvec.len(), 0);
        assert!(!globvec.is_match(Path::new("foo/bar")));
    }

    #[test]
    fn test_simple_globs() {
        let globvec =
            GlobVec::from_strings(vec!["*.txt".to_string(), "src/**/*.rs".to_string()]).unwrap();

        assert!(!globvec.is_empty());
        assert_eq!(globvec.len(), 2);
        assert!(globvec.is_match(Path::new("file.txt")));
        assert!(globvec.is_match(Path::new("src/main.rs")));
        assert!(globvec.is_match(Path::new("src/foo/bar.rs")));
        assert!(!globvec.is_match(Path::new("file.rs")));
    }

    #[test]
    fn test_directory_patterns() {
        let globvec = GlobVec::from_strings(vec!["bin/".to_string()]).unwrap();

        assert!(globvec.is_match(Path::new("bin/tool")));
        assert!(globvec.is_match(Path::new("bin/sub/tool")));
        assert!(!globvec.is_match(Path::new("lib/tool")));
    }

    #[test]
    fn test_file_or_directory_patterns() {
        let globvec = GlobVec::from_strings(vec!["foo".to_string()]).unwrap();

        // Should match both the file 'foo' and everything under 'foo/'
        assert!(globvec.is_match(Path::new("foo")));
        assert!(globvec.is_match(Path::new("foo/bar")));
        assert!(globvec.is_match(Path::new("foo/baz/qux")));
        assert!(!globvec.is_match(Path::new("bar")));
    }

    #[test]
    fn test_invalid_glob() {
        let result = GlobVec::from_strings(vec!["foo/{bar".to_string()]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.unwrap().contains("Invalid glob pattern"));
    }

    #[test]
    fn test_sources() {
        let patterns = vec!["*.txt".to_string(), "src/**/*.rs".to_string()];
        let globvec = GlobVec::from_strings(patterns.clone()).unwrap();

        let sources = globvec.sources();
        assert_eq!(sources, vec!["*.txt", "src/**/*.rs"]);

        let to_strings = globvec.to_strings();
        assert_eq!(to_strings, patterns);
    }
}
