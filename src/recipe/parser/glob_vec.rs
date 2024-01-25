use std::fmt::{self, Formatter, Debug};

use globset::{Glob, GlobSet};

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

use crate::_partialerror;
use crate::recipe::custom_yaml::{HasSpan, RenderedNode, RenderedSequenceNode, TryConvertNode};
use crate::recipe::error::{ErrorKind, PartialParsingError};

#[derive(Default, Clone)]
pub struct GlobVec(Vec<Glob>, Option<GlobSet>);

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
        f.debug_list().entries(self.0.iter().map(|glob| glob.glob())).finish()
    }
}

impl<'de> Deserialize<'de> for GlobVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut vec = Vec::deserialize(deserializer)?;
        let mut glob_vec = Vec::with_capacity(vec.len());
        for glob in vec.drain(..) {
            let glob = Glob::new(glob).map_err(serde::de::Error::custom)?;
            glob_vec.push(glob);
        }

        if glob_vec.is_empty() {
            Ok(Self(glob_vec, None))
        } else {
            let mut globset_builder = globset::GlobSetBuilder::new();
            for glob in glob_vec.iter() {
                globset_builder.add(glob.clone());
            }
            let globset = globset_builder.build().map_err(serde::de::Error::custom)?;

            Ok(Self(glob_vec, Some(globset)))
        }
    }
}

impl GlobVec {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn globset(&self) -> Option<&GlobSet> {
        self.1.as_ref()
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
}
