#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skip(bool);

impl TryConvertNode<Skip> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Skip, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Scalar(scalar) => scalar.try_convert(name),
            RenderedNode::Sequence(sequence) => sequence.try_convert(name),
            RenderedNode::Mapping(mapping) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedScalarOrSequenceOrMapping,
            )]),
            RenderedNode::Null => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedScalarOrSequenceOrMapping,
            )]),
        }?
    }
}
