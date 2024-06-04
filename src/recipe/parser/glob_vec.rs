use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::Path;

use globset::{Glob, GlobSet};

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize};

use crate::_partialerror;
use crate::recipe::custom_yaml::{
    HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
    TryConvertNode,
};
use crate::recipe::error::{ErrorKind, PartialParsingError};

/// Wrapper type to simplify serialization of Vec<Glob>
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct InnerGlobVec(Vec<Glob>);

impl Deref for InnerGlobVec {
    type Target = Vec<Glob>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl InnerGlobVec {
    fn globset(&self) -> Result<GlobSet, globset::Error> {
        let mut globset_builder = globset::GlobSetBuilder::new();
        for glob in self.iter() {
            globset_builder.add(glob.clone());
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

impl From<Vec<Glob>> for InnerGlobVec {
    fn from(vec: Vec<Glob>) -> Self {
        Self(vec)
    }
}

fn to_glob(glob: &str) -> Result<Glob, globset::Error> {
    if glob.ends_with('/') && !glob.contains('*') {
        // we treat folders as globs that match everything in the folder
        Glob::new(&format!("{}**", glob))
    } else {
        Glob::new(glob)
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
            seq.serialize_element(glob.glob())?;
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
            .entries(self.include.iter().map(|glob| glob.glob()))
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
    pub fn include_globs(&self) -> &Vec<Glob> {
        &self.include
    }

    /// Returns an iterator over the globs
    pub fn exclude_globs(&self) -> &Vec<Glob> {
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
    #[cfg(test)]
    pub fn from_vec(include: Vec<&str>, exclude: Option<Vec<&str>>) -> Self {
        let include_vec: Vec<Glob> = include
            .into_iter()
            .map(|glob| to_glob(glob).unwrap())
            .collect();
        let exclude_vec: Vec<Glob> = exclude
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
            exclude_globset: exclude_globset,
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
) -> Result<Vec<Glob>, Vec<PartialParsingError>> {
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
        let vec = vec![to_glob(self.as_str())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])?];
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
}
