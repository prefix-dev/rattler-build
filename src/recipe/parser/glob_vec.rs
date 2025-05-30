use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::Path;

use globset::{Glob, GlobBuilder, GlobSet};

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize};

use crate::_partialerror;
use crate::recipe::custom_yaml::{
    HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
    TryConvertNode,
};
use crate::recipe::error::{ErrorKind, PartialParsingError};

/// A glob with the source string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobWithSource {
    /// The glob
    glob: Glob,
    /// The source string
    source: String,
}

impl GlobWithSource {
    /// Returns the glob
    pub fn glob(&self) -> &Glob {
        &self.glob
    }

    /// Returns the source string
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Wrapper type to simplify serialization of Vec<Glob>
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

fn to_glob(glob: &str) -> Result<GlobWithSource, globset::Error> {
    // first, try to parse as a normal glob so that we get a descriptive error
    let _ = Glob::new(glob)?;
    if glob.ends_with('/') {
        // we treat folders as globs that match everything in the folder
        Ok(GlobWithSource {
            glob: Glob::new(&format!("{glob}**"))?,
            source: glob.to_string(),
        })
    } else {
        // Match either file, or folder
        Ok(GlobWithSource {
            glob: GlobBuilder::new(&format!("{glob}{{,/**}}"))
                .empty_alternates(true)
                .build()?,
            source: glob.to_string(),
        })
    }
}

/// A vector of globs that is also immediately converted to a globset
/// to enhance parser errors.
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
    /// Create a new GlobVec from a vector of globs
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

    /// Returns true if the globvec is empty
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Returns an iterator over the globs
    pub fn include_globs(&self) -> &Vec<GlobWithSource> {
        &self.include
    }

    /// Returns an iterator over the globs
    pub fn exclude_globs(&self) -> &Vec<GlobWithSource> {
        &self.exclude
    }

    /// Returns true if the path matches any include glob and does not match any exclude glob
    /// If there are no globs at all, we match nothing.
    /// If there is no include glob, we match everything except the exclude globs.
    pub fn is_match(&self, path: &Path) -> bool {
        // if both include & exclude are empty, we match nothing
        if self.is_empty() {
            return false;
        }
        // if include is empty, it matches everything. Otherwise we check!
        let is_match = self.include.is_empty() || self.include_globset.is_match(path);
        // if exclude is empty, it matches everything. Otherwise we check!
        is_match && (self.exclude.is_empty() || !self.exclude_globset.is_match(path))
    }

    /// Only used for testing
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

impl TryConvertNode<GlobVec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<GlobVec, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Sequence(sequence) => sequence.try_convert(name),
            RenderedNode::Mapping(mapping) => mapping.try_convert(name),
            RenderedNode::Scalar(scalar) => scalar.try_convert(name),
            _ => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedSequence,
                label = "expected a list of globs strings"
            )]),
        }
    }
}

fn to_vector_of_globs(
    sequence: &RenderedSequenceNode,
) -> Result<Vec<GlobWithSource>, Vec<PartialParsingError>> {
    let mut vec = Vec::with_capacity(sequence.len());
    for item in sequence.iter() {
        let str: String = item.try_convert("globs")?;
        vec.push(
            to_glob(&str)
                .map_err(|err| vec![_partialerror!(*item.span(), ErrorKind::GlobParsing(err),)])?,
        );
    }
    Ok(vec)
}

impl TryConvertNode<GlobVec> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<GlobVec, Vec<PartialParsingError>> {
        let vec = vec![
            to_glob(self.as_str())
                .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])?,
        ];
        GlobVec::new(vec.into(), InnerGlobVec::default())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])
    }
}

impl TryConvertNode<GlobVec> for RenderedSequenceNode {
    fn try_convert(&self, _name: &str) -> Result<GlobVec, Vec<PartialParsingError>> {
        let vec = to_vector_of_globs(self)?;
        GlobVec::new(vec.into(), InnerGlobVec::default())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])
    }
}

impl TryConvertNode<GlobVec> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<GlobVec, Vec<PartialParsingError>> {
        // find the `include` and `exclude` keys
        let mut include = Vec::new();
        let mut exclude = Vec::new();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match (key_str, value) {
                ("include", RenderedNode::Sequence(seq)) => {
                    include = to_vector_of_globs(seq)?;
                }
                ("exclude", RenderedNode::Sequence(seq)) => {
                    exclude = to_vector_of_globs(seq)?;
                }
                ("include" | "exclude", _) => {
                    return Err(vec![_partialerror!(
                        *value.span(),
                        ErrorKind::ExpectedSequence,
                        label = "expected a list of globs strings for `include` or `exclude`"
                    )]);
                }
                _ => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(key_str.to_string().into()),
                        help = format!("valid options for {} are `include` and `exclude`", name)
                    )]);
                }
            }
        }

        GlobVec::new(include.into(), exclude.into())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])
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

    /// Returns true if the path matches any exists glob and does not match any not_exists glob
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

impl TryConvertNode<GlobCheckerVec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<GlobCheckerVec, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Mapping(mapping) => mapping.try_convert(name),
            RenderedNode::Sequence(seq) => {
                let globs = to_vector_of_globs(seq)?;
                GlobCheckerVec::new(globs.into(), InnerGlobVec::default())
                    .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])
            }
            RenderedNode::Scalar(scalar) => {
                let glob = to_glob(scalar.as_str()).map_err(|err| {
                    vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)]
                })?;
                GlobCheckerVec::new(vec![glob].into(), InnerGlobVec::default())
                    .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])
            }
            RenderedNode::Null(_) => Ok(GlobCheckerVec::default()),
        }
    }
}

impl TryConvertNode<GlobCheckerVec> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<GlobCheckerVec, Vec<PartialParsingError>> {
        let mut exists = Vec::new();
        let mut not_exists = Vec::new();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match (key_str, value) {
                ("exists", RenderedNode::Sequence(seq)) => {
                    exists = to_vector_of_globs(seq)?;
                }
                ("not_exists", RenderedNode::Sequence(seq)) => {
                    not_exists = to_vector_of_globs(seq)?;
                }
                ("exists" | "not_exists", _) => {
                    return Err(vec![_partialerror!(
                        *value.span(),
                        ErrorKind::ExpectedSequence,
                        label = "expected a list of globs strings for `exists` or `not_exists`"
                    )]);
                }
                _ => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(key_str.to_string().into()),
                        help = format!("valid options for {} are `exists` and `not_exists`", name)
                    )]);
                }
            }
        }

        GlobCheckerVec::new(exists.into(), not_exists.into())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])
    }
}

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

impl TryConvertNode<AllOrGlobVec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<AllOrGlobVec, Vec<PartialParsingError>> {
        if let Some(sequence) = self.as_sequence() {
            sequence.try_convert(name)
        } else if let Some(scalar) = self.as_scalar() {
            scalar.try_convert(name)
        } else {
            Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedScalar,
                label = "expected a boolean value or a sequence of glob strings"
            )])
        }
    }
}

impl TryConvertNode<AllOrGlobVec> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<AllOrGlobVec, Vec<PartialParsingError>> {
        let globvec: GlobVec = self.try_convert(name)?;
        Ok(AllOrGlobVec::SpecificPaths(globvec))
    }
}

impl TryConvertNode<AllOrGlobVec> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<AllOrGlobVec, Vec<PartialParsingError>> {
        if let Some(value) = self.as_bool() {
            Ok(AllOrGlobVec::All(value))
        } else {
            Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::InvalidValue((
                    "Expected a boolean value or a sequence of globs".to_string(),
                    self.as_str().to_owned().into()
                ))
            )])
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{assert_miette_snapshot, recipe::ParsingError};

    use super::*;

    #[test]
    fn test_parsing_globvec() {
        let yaml = r#"globs:
        - foo
        - bar
        - baz/**/qux
        "#;

        let yaml_root = RenderedNode::parse_yaml(0, yaml)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let globvec: GlobVec = tests_node.try_convert("globs").unwrap();
        assert_eq!(globvec.include.len(), 3);
        assert_eq!(globvec.include_globset.len(), 3);

        let as_yaml = serde_yaml::to_string(&globvec).unwrap();
        insta::assert_snapshot!(&as_yaml);
        let parsed_again: GlobVec = serde_yaml::from_str(&as_yaml).unwrap();
        assert_eq!(parsed_again.include.len(), 3);
        assert_eq!(parsed_again.include_globset.len(), 3);

        let yaml = r#"globs:
        include: ["foo/", "bar", "baz/**/qux"]
        exclude: ["foo/bar", "bar/*.txt"]
        "#;

        let yaml_root = RenderedNode::parse_yaml(0, yaml)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let globvec: GlobVec = tests_node.try_convert("globs").unwrap();
        assert_eq!(globvec.include.len(), 3);
        assert_eq!(globvec.include_globset.len(), 3);
        assert_eq!(globvec.exclude.len(), 2);
        assert_eq!(globvec.exclude_globset.len(), 2);

        let as_yaml = serde_yaml::to_string(&globvec).unwrap();
        insta::assert_snapshot!(&as_yaml);
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
    fn test_parsing_globvec_fail() {
        let yaml = r#"globs:
        - foo/{bla
        "#;

        let yaml_root = RenderedNode::parse_yaml(0, yaml)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let res: Result<GlobVec, Vec<PartialParsingError>> = tests_node.try_convert("globs");
        assert!(res.is_err());
        let mut err = res.unwrap_err();
        let err = ParsingError::from_partial(yaml, err.remove(0));
        assert_miette_snapshot!(err);
    }

    #[derive(Deserialize, Serialize)]
    struct TestAllOrGlobVec {
        globs: AllOrGlobVec,
    }

    #[test]
    fn test_parsing_all_or_globvec() {
        let yaml = r#"globs:
        - foo
        - bar
        - baz/**/qux
        "#;

        let yaml_root = RenderedNode::parse_yaml(0, yaml)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let all_or_globvec: AllOrGlobVec = tests_node.try_convert("globs").unwrap();
        assert!(all_or_globvec.is_match(Path::new("foo")));
        assert!(all_or_globvec.is_match(Path::new("bar")));
        assert!(all_or_globvec.is_match(Path::new("baz/qux")));
        assert!(all_or_globvec.is_match(Path::new("baz/bla/qux")));
        assert!(!all_or_globvec.is_match(Path::new("bla")));
        assert!(!all_or_globvec.is_match(Path::new("bla/qux")));

        let mut test_struct = TestAllOrGlobVec {
            globs: all_or_globvec.clone(),
        };
        let as_yaml = serde_yaml::to_string(&test_struct).unwrap();
        insta::assert_snapshot!(&as_yaml);
        let parsed_again: TestAllOrGlobVec = serde_yaml::from_str(&as_yaml).unwrap();
        assert_eq!(parsed_again.globs, all_or_globvec);

        let globs_true = r#"globs: true"#;
        let yaml_root = RenderedNode::parse_yaml(0, globs_true)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let all_or_globvec: AllOrGlobVec = tests_node.try_convert("globs").unwrap();
        assert!(all_or_globvec.is_match(Path::new("foo")));
        assert!(all_or_globvec.is_all());

        test_struct.globs = all_or_globvec.clone();
        let globs_all = serde_yaml::to_string(&test_struct).unwrap();
        insta::assert_snapshot!(&globs_all);
        let parsed_again: TestAllOrGlobVec = serde_yaml::from_str(&globs_all).unwrap();
        assert_eq!(parsed_again.globs, AllOrGlobVec::All(true));

        let globs_false = r#"globs: false"#;
        let yaml_root = RenderedNode::parse_yaml(0, globs_false)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let all_or_globvec: AllOrGlobVec = tests_node.try_convert("globs").unwrap();
        assert!(!all_or_globvec.is_match(Path::new("foo")));
        assert!(all_or_globvec.is_none());

        test_struct.globs = all_or_globvec.clone();
        let globs_none = serde_yaml::to_string(&test_struct).unwrap();
        insta::assert_snapshot!(&globs_none);
        let parsed_again: TestAllOrGlobVec = serde_yaml::from_str(&globs_none).unwrap();
        assert_eq!(parsed_again.globs, AllOrGlobVec::All(false));
    }

    #[test]
    fn test_parsing_glob_checker_vec() {
        let yaml = r#"globs:
        exists: ["foo", "bar"]
        not_exists: ["baz/**/qux"]
        "#;
        let yaml_root = RenderedNode::parse_yaml(0, yaml)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("globs").unwrap();

        // Now we should use GlobCheckerVec which has exists/not_exists fields
        let glob_checker: GlobCheckerVec = tests_node.try_convert("globs").unwrap();
        assert_eq!(glob_checker.exists_globs().len(), 2);
        assert_eq!(glob_checker.not_exists_globs().len(), 1);

        // Test the paths match correctly
        assert!(glob_checker.is_match(Path::new("foo")));
        assert!(glob_checker.is_match(Path::new("bar")));
        assert!(!glob_checker.is_match(Path::new("baz/qux")));
        assert!(!glob_checker.is_match(Path::new("baz/some/qux")));

        // Test direct deserialization
        let yaml_str = r#"
        exists: ["foo", "bar"]
        not_exists: ["baz/**/qux"]
        "#;
        let glob_checker: GlobCheckerVec = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(glob_checker.exists_globs().len(), 2);
        assert_eq!(glob_checker.not_exists_globs().len(), 1);

        // Verify paths match as expected after deserialization
        assert!(glob_checker.is_match(Path::new("foo")));
        assert!(glob_checker.is_match(Path::new("bar")));
        assert!(!glob_checker.is_match(Path::new("baz/qux")));
        assert!(!glob_checker.is_match(Path::new("baz/some/qux")));

        // Test serialization
        let yaml = serde_yaml::to_string(&glob_checker).unwrap();
        insta::assert_snapshot!(&yaml);

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

        // Make sure the `try_convert` functionality also handles plain lists
        let plain_yaml = r#"globs:
        - foo
        - bar
        "#;
        let plain_yaml_root = RenderedNode::parse_yaml(0, plain_yaml)
            .map_err(|err| vec![err])
            .unwrap();
        let plain_tests_node = plain_yaml_root.as_mapping().unwrap().get("globs").unwrap();
        let plain_glob_checker: GlobCheckerVec = plain_tests_node.try_convert("globs").unwrap();
        assert_eq!(plain_glob_checker.exists_globs().len(), 2);
        assert_eq!(plain_glob_checker.not_exists_globs().len(), 0);
        assert!(plain_glob_checker.is_match(Path::new("foo")));
        assert!(plain_glob_checker.is_match(Path::new("bar")));
    }

    #[test]
    fn test_serialize_only_exists_globs_as_list() {
        let checker = GlobCheckerVec::from_vec(vec!["foo", "bar"], None);
        let yaml = serde_yaml::to_string(&checker).unwrap();
        assert_eq!(yaml, "- foo\n- bar\n");
    }
}
