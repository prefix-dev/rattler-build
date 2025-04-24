use marked_yaml::Span;

use crate::{
    _partialerror,
    recipe::{
        Jinja,
        custom_yaml::{HasSpan, RenderedNode, RenderedSequenceNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};

#[derive(Default, Debug, Clone)]
pub struct Skip(Vec<(String, Span)>, Option<bool>);

impl TryConvertNode<Vec<(String, Span)>> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<Vec<(String, Span)>, Vec<PartialParsingError>> {
        let mut conditions = vec![];

        for node in self.iter() {
            match node {
                RenderedNode::Scalar(scalar) => {
                    let s: String = scalar.try_convert(name)?;
                    conditions.push((s, *node.span()))
                }
                _ => {
                    return Err(vec![_partialerror!(
                        *node.span(),
                        ErrorKind::ExpectedScalar,
                    )]);
                }
            }
        }
        Ok(conditions)
    }
}

impl TryConvertNode<Skip> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Skip, Vec<PartialParsingError>> {
        let conditions = match self {
            RenderedNode::Scalar(scalar) => vec![(scalar.try_convert(name)?, *self.span())],
            RenderedNode::Sequence(sequence) => sequence.try_convert(name)?,
            RenderedNode::Mapping(_) => {
                return Err(vec![_partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedSequence,
                )]);
            }
            RenderedNode::Null(_) => vec![],
        };

        Ok(Skip(conditions, None))
    }
}

impl Skip {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn with_eval(self, jinja: &Jinja) -> Result<Self, Vec<PartialParsingError>> {
        for condition in &self.0 {
            match jinja.eval(&condition.0) {
                Ok(res) => {
                    if res.is_true() {
                        return Ok(Skip(self.0, Some(true)));
                    }
                }
                Err(e) => {
                    return Err(vec![_partialerror!(
                        condition.1,
                        ErrorKind::JinjaRendering(e),
                    )]);
                }
            }
        }
        Ok(Skip(self.0, Some(false)))
    }

    pub fn eval(&self) -> bool {
        self.1.unwrap_or(true)
    }
}
