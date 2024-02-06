use std::fmt::{self, Debug, Formatter};
use std::path::Path;

use globset::{Glob, GlobSet};

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

use crate::_partialerror;
use crate::recipe::custom_yaml::{
    HasSpan, RenderedNode, RenderedScalarNode, RenderedSequenceNode, TryConvertNode,
};
use crate::recipe::error::{ErrorKind, PartialParsingError};

/// A vector of globs that is also immediately converted to a globset
/// to enhance parser errors.
#[derive(Default, Clone)]
pub struct GlobVec(Vec<Glob>, Option<GlobSet>);

impl PartialEq for GlobVec {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for GlobVec {}

impl Serialize for GlobVec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for glob in self.0.iter() {
            seq.serialize_element(glob.glob())?;
        }
        seq.end()
    }
}

impl Debug for GlobVec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|glob| glob.glob()))
            .finish()
    }
}

impl<'de> Deserialize<'de> for GlobVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut raw_globs: Vec<String> = Vec::deserialize(deserializer)?;
        let mut globs = Vec::with_capacity(raw_globs.len());
        for raw in raw_globs.drain(..) {
            let glob = Glob::new(&raw).map_err(serde::de::Error::custom)?;
            globs.push(glob);
        }

        if globs.is_empty() {
            Ok(Self(globs, None))
        } else {
            let mut globset_builder = globset::GlobSetBuilder::new();
            for glob in globs.iter() {
                globset_builder.add(glob.clone());
            }
            let globset = globset_builder.build().map_err(serde::de::Error::custom)?;

            Ok(Self(globs, Some(globset)))
        }
    }
}

impl GlobVec {
    /// Returns true if the globvec is empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator over the globs
    pub fn globs(&self) -> impl Iterator<Item = &Glob> {
        self.0.iter()
    }

    /// Returns the globset if it exists
    pub fn globset(&self) -> Option<&GlobSet> {
        self.1.as_ref()
    }

    /// Returns true if the path matches any of the globs
    pub fn is_match(&self, path: &Path) -> bool {
        if let Some(globset) = self.1.as_ref() {
            globset.is_match(path)
        } else {
            false
        }
    }

    /// Only used for testing
    #[cfg(test)]
    pub fn from_vec(vec: Vec<&str>) -> Self {
        let mut glob_vec = Vec::with_capacity(vec.len());
        for glob in vec.into_iter() {
            glob_vec.push(Glob::new(glob).unwrap());
        }

        if glob_vec.is_empty() {
            Self(glob_vec, None)
        } else {
            let mut globset_builder = globset::GlobSetBuilder::new();
            for glob in glob_vec.iter() {
                globset_builder.add(glob.clone());
            }
            let globset = globset_builder.build().unwrap();

            Self(glob_vec, Some(globset))
        }
    }
}

impl TryConvertNode<GlobVec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<GlobVec, Vec<PartialParsingError>> {
        self.as_sequence()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedSequence)])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<GlobVec> for RenderedSequenceNode {
    fn try_convert(&self, _name: &str) -> Result<GlobVec, Vec<PartialParsingError>> {
        let mut vec = Vec::with_capacity(self.len());
        for item in self.iter() {
            let str: String = item.try_convert(_name)?;
            vec.push(
                Glob::new(&str).map_err(|err| {
                    vec![_partialerror!(*item.span(), ErrorKind::GlobParsing(err),)]
                })?,
            );
        }

        if vec.is_empty() {
            Ok(GlobVec(vec, None))
        } else {
            let mut globset_builder = globset::GlobSetBuilder::new();
            for glob in vec.iter() {
                globset_builder.add(glob.clone());
            }
            let globset = globset_builder
                .build()
                .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::GlobParsing(err),)])?;

            Ok(GlobVec(vec, Some(globset)))
        }
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
            Err(vec![
                _partialerror!(*self.span(), ErrorKind::ExpectedScalar),
                _partialerror!(*self.span(), ErrorKind::ExpectedSequence),
            ])
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
        assert_eq!(globvec.0.len(), 3);
        assert_eq!(globvec.1.as_ref().unwrap().len(), 3);

        let as_yaml = serde_yaml::to_string(&globvec).unwrap();
        insta::assert_snapshot!(&as_yaml);
        let parsed_again: GlobVec = serde_yaml::from_str(&as_yaml).unwrap();
        assert_eq!(parsed_again.0.len(), 3);
        assert_eq!(parsed_again.1.as_ref().unwrap().len(), 3);
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
