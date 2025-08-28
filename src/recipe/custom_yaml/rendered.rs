#![allow(missing_docs)]
//! Module to define an `Node` type that is specific to the first stage of the
//! new Conda recipe format parser.

use std::{fmt, hash::Hash, ops};

use indexmap::IndexMap;
use marked_yaml::{Span, types::MarkedScalarNode};
use serde::{Serialize, Serializer};

use crate::{
    _partialerror,
    recipe::{
        Render,
        error::{ErrorKind, ParsingError, PartialParsingError, jinja_error_to_label},
        jinja::Jinja,
    },
    source_code::SourceCode,
};

use super::{
    HasSpan, MappingNode, Node, ScalarNode, SequenceNode, SequenceNodeInternal, parse_yaml,
};

/// A span-marked new Conda Recipe YAML node
///
/// This is a reinterpretation of the [`marked_yaml::Node`] type that is specific
/// for the first stage of the new Conda recipe format parser. This type handles
/// the `if / then / else` selector (or if-selector for simplicity) as a special
/// case of the sequence node, i.e., the occurrences of if-selector in the recipe
/// are syntactically parsed in the conversion of [`marked_yaml::Node`] to this type.
///
/// **CAUTION:** The user of this type that is responsible to handle the if the
/// if-selector has semantic validity or not.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the
/// same place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RenderedNode {
    /// A YAML scalar
    ///
    /// You can test if a node is a scalar, and retrieve it as one if you
    /// so wish.
    Scalar(RenderedScalarNode),
    /// A YAML mapping
    ///
    /// You can test if a node is a mapping, and retrieve it as one if you
    /// so wish.
    Mapping(RenderedMappingNode),
    /// A YAML sequence
    ///
    /// You can test if a node is a sequence, and retrieve it as one if you
    /// so wish.
    Sequence(RenderedSequenceNode),
    /// A YAML null
    ///
    /// This is a special case of a scalar node, but is treated as its own
    /// type here for convenience.
    Null(RenderedScalarNode),
}

impl Serialize for RenderedNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            RenderedNode::Scalar(node) => node.serialize(serializer),
            RenderedNode::Mapping(node) => node.serialize(serializer),
            RenderedNode::Sequence(node) => node.serialize(serializer),
            RenderedNode::Null(node) => node.serialize(serializer),
        }
    }
}

impl RenderedNode {
    /// Parse YAML from a string and return a Node representing
    /// the content.
    ///
    /// When parsing YAML, the source is stored into all markers which are
    /// in the node spans.  This means that later if you only have a node,
    /// you can determine which source it came from without needing complex
    /// lifetimes to bind strings or other non-copy data to nodes.
    ///
    /// This requires that the top level be a mapping, but the returned
    /// type here is the generic Node enumeration to make it potentially easier
    /// for callers to use.  Regardless, it's always possible to treat the
    /// returned node as a mapping node without risk of panic.
    pub fn parse_yaml<S: SourceCode>(
        init_span_index: usize,
        src: S,
    ) -> Result<Self, ParsingError<S>> {
        let yaml = parse_yaml(init_span_index, src.clone())?;
        Self::try_from(yaml).map_err(|err| ParsingError::from_partial(src, err))
    }

    pub fn is_mapping(&self) -> bool {
        matches!(self, Self::Mapping(_))
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar(_))
    }

    pub fn is_sequence(&self) -> bool {
        matches!(self, Self::Sequence(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null(_))
    }

    /// Retrieve the scalar from this node if there is one
    pub fn as_scalar(&self) -> Option<&RenderedScalarNode> {
        match self {
            RenderedNode::Scalar(msn) => Some(msn),
            _ => None,
        }
    }

    /// Retrieve the sequence from this node if there is one
    pub fn as_sequence(&self) -> Option<&RenderedSequenceNode> {
        match self {
            RenderedNode::Sequence(msn) => Some(msn),
            _ => None,
        }
    }

    /// Retrieve the mapping from this node if there is one
    pub fn as_mapping(&self) -> Option<&RenderedMappingNode> {
        match self {
            RenderedNode::Mapping(mmn) => Some(mmn),
            _ => None,
        }
    }
}

impl HasSpan for RenderedNode {
    fn span(&self) -> &Span {
        match self {
            Self::Mapping(map) => map.span(),
            Self::Scalar(scalar) => scalar.span(),
            Self::Sequence(seq) => seq.span(),
            Self::Null(null) => null.span(),
        }
    }
}

impl<'i> TryFrom<&'i RenderedNode> for &'i RenderedScalarNode {
    type Error = ();

    fn try_from(value: &'i RenderedNode) -> Result<Self, Self::Error> {
        value.as_scalar().ok_or(())
    }
}

impl From<RenderedScalarNode> for RenderedNode {
    fn from(value: RenderedScalarNode) -> Self {
        Self::Scalar(value)
    }
}

impl From<RenderedMappingNode> for RenderedNode {
    fn from(value: RenderedMappingNode) -> Self {
        Self::Mapping(value)
    }
}

impl From<RenderedSequenceNode> for RenderedNode {
    fn from(value: RenderedSequenceNode) -> Self {
        Self::Sequence(value)
    }
}

impl From<Vec<RenderedNode>> for RenderedNode {
    fn from(value: Vec<RenderedNode>) -> Self {
        Self::Sequence(RenderedSequenceNode::from(value))
    }
}

impl From<IndexMap<RenderedScalarNode, RenderedNode>> for RenderedNode {
    fn from(value: IndexMap<RenderedScalarNode, RenderedNode>) -> Self {
        Self::Mapping(RenderedMappingNode::from(value))
    }
}

impl From<String> for RenderedNode {
    fn from(value: String) -> Self {
        Self::Scalar(RenderedScalarNode::from(value))
    }
}

impl From<&str> for RenderedNode {
    fn from(value: &str) -> Self {
        Self::Scalar(RenderedScalarNode::from(value.to_owned()))
    }
}

impl TryFrom<marked_yaml::Node> for RenderedNode {
    type Error = PartialParsingError;

    fn try_from(value: marked_yaml::Node) -> Result<Self, Self::Error> {
        RenderedNode::try_from(&value)
    }
}

impl TryFrom<&marked_yaml::Node> for RenderedNode {
    type Error = PartialParsingError;

    fn try_from(value: &marked_yaml::Node) -> Result<Self, Self::Error> {
        match value {
            marked_yaml::Node::Scalar(scalar) => Ok(Self::Scalar(scalar.into())),
            marked_yaml::Node::Mapping(map) => {
                Ok(Self::Mapping(RenderedMappingNode::try_from(map.clone())?))
            }
            marked_yaml::Node::Sequence(seq) => {
                Ok(Self::Sequence(RenderedSequenceNode::try_from(seq.clone())?))
            }
        }
    }
}

/// A marked scalar YAML node
///
/// Scalar nodes are treated by this crate as strings, though a few special
/// values are processed into the types which YAML would ascribe.  In particular
/// strings of the value `null`, `true`, `false`, etc. are able to present as
/// their special values to make it a bit easier for users of the crate.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the
/// same place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Clone)]
pub struct RenderedScalarNode {
    span: marked_yaml::Span,
    source: String,
    value: String,
    may_coerce: bool,
}

impl Serialize for RenderedScalarNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl RenderedScalarNode {
    pub fn new(span: marked_yaml::Span, source: String, value: String, may_coerce: bool) -> Self {
        Self {
            span,
            source,
            value,
            may_coerce,
        }
    }

    pub fn new_blank() -> Self {
        Self::new(
            marked_yaml::Span::new_blank(),
            String::new(),
            String::new(),
            false,
        )
    }

    /// Treat the scalar node as a string
    ///
    /// Since scalars are always stringish, this is always safe.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Return the source with the original Jinja template
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Treat the scalar node as a boolean
    ///
    /// If the scalar contains any of the following then it is true:
    ///
    /// * `true`
    /// * `True`
    /// * `TRUE`
    ///
    /// The following are considered false:
    ///
    /// * `false`
    /// * `False`
    /// * `FALSE`
    ///
    /// Everything else is not a boolean and so will return None
    pub fn as_bool(&self) -> Option<bool> {
        if !self.may_coerce {
            return None;
        }
        match self.value.as_str() {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        if !self.may_coerce {
            return None;
        }
        self.value.parse().ok()
    }
}

impl HasSpan for RenderedScalarNode {
    fn span(&self) -> &Span {
        &self.span
    }
}

impl PartialEq for RenderedScalarNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }

    #[allow(clippy::partialeq_ne_impl)]
    fn ne(&self, other: &Self) -> bool {
        self.value != other.value
    }
}

impl Eq for RenderedScalarNode {}

impl Hash for RenderedScalarNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl fmt::Debug for RenderedScalarNode {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("RenderedScalarNode");
        if include_span {
            debug.field("span", &self.span);
        }
        debug.field("value", &self.value).finish()
    }
}

impl<'a> From<&'a str> for RenderedScalarNode {
    /// Convert from any borrowed string into a node
    fn from(value: &'a str) -> Self {
        Self::new(
            marked_yaml::Span::new_blank(),
            value.to_owned(),
            value.to_owned(),
            false,
        )
    }
}

impl From<String> for RenderedScalarNode {
    /// Convert from any owned string into a node
    fn from(value: String) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value.clone(), value, false)
    }
}

impl ops::Deref for RenderedScalarNode {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl std::borrow::Borrow<str> for RenderedScalarNode {
    fn borrow(&self) -> &str {
        &self.value
    }
}

impl From<MarkedScalarNode> for RenderedScalarNode {
    fn from(value: MarkedScalarNode) -> Self {
        Self::from(&value)
    }
}

impl From<&MarkedScalarNode> for RenderedScalarNode {
    fn from(value: &MarkedScalarNode) -> Self {
        Self::new(
            *value.span(),
            value.as_str().to_owned(),
            value.as_str().to_owned(),
            value.may_coerce(),
        )
    }
}

impl From<bool> for RenderedScalarNode {
    /// Convert from a boolean into a node
    fn from(value: bool) -> Self {
        if value { "true".into() } else { "false".into() }
    }
}

macro_rules! scalar_from_to_number {
    ($t:ident, $as:ident) => {
        impl From<$t> for RenderedScalarNode {
            #[doc = "Convert from "]
            #[doc = stringify!($t)]
            #[doc = r#" into a node"#]
            fn from(value: $t) -> Self {
                format!("{}", value).into()
            }
        }

        impl RenderedScalarNode {
            #[doc = "Treat the scalar node as "]
            #[doc = stringify!($t)]
            #[doc = r#".

If this scalar node's value can be represented properly as
a number of the right kind then return it.  This is essentially
a shortcut for using the `FromStr` trait on the return value of
`.as_str()`."#]
            pub fn $as(&self) -> Option<$t> {
                use std::str::FromStr;
                $t::from_str(&self.value).ok()
            }
        }
    };
}

scalar_from_to_number!(i8, as_i8);
scalar_from_to_number!(i16, as_i16);
scalar_from_to_number!(i32, as_i32);
scalar_from_to_number!(i64, as_i64);
scalar_from_to_number!(i128, as_i128);
scalar_from_to_number!(isize, as_isize);
scalar_from_to_number!(u8, as_u8);
scalar_from_to_number!(u16, as_u16);
scalar_from_to_number!(u32, as_u32);
scalar_from_to_number!(u64, as_u64);
scalar_from_to_number!(u128, as_u128);
scalar_from_to_number!(usize, as_usize);

/// A marked YAML sequence node
///
/// Sequence nodes in YAML are simply ordered lists of YAML nodes.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the
/// same place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Clone)]
pub struct RenderedSequenceNode {
    span: marked_yaml::Span,
    value: Vec<RenderedNode>,
}

impl Serialize for RenderedSequenceNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl RenderedSequenceNode {
    pub fn new(span: marked_yaml::Span, value: Vec<RenderedNode>) -> Self {
        Self { span, value }
    }
}

impl HasSpan for RenderedSequenceNode {
    fn span(&self) -> &marked_yaml::Span {
        &self.span
    }
}

impl PartialEq for RenderedSequenceNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl Eq for RenderedSequenceNode {}

impl Hash for RenderedSequenceNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl From<Vec<RenderedNode>> for RenderedSequenceNode {
    fn from(value: Vec<RenderedNode>) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value)
    }
}

impl TryFrom<marked_yaml::types::MarkedSequenceNode> for RenderedSequenceNode {
    type Error = PartialParsingError;

    fn try_from(node: marked_yaml::types::MarkedSequenceNode) -> Result<Self, Self::Error> {
        let mut value = Vec::with_capacity(node.len());

        for item in node.iter() {
            value.push(RenderedNode::try_from(item.clone())?);
        }

        Ok(Self::new(*node.span(), value))
    }
}

impl ops::Deref for RenderedSequenceNode {
    type Target = Vec<RenderedNode>;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl ops::DerefMut for RenderedSequenceNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl fmt::Debug for RenderedSequenceNode {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("RenderedSequenceNode");
        if include_span {
            debug.field("span", &self.span);
        }
        debug.field("value", &self.value).finish()
    }
}

/// A marked YAML mapping node
///
/// Mapping nodes in YAML are defined as a key/value mapping where the keys are
/// unique and always scalars, whereas values may be YAML nodes of any kind.
///
/// Because there is an example that on the `context` key-value definition, a later
/// key was defined as a jinja string using previous values, we need to care about
/// insertion order we use [`IndexMap`] for this.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the same
/// place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Clone)]
pub struct RenderedMappingNode {
    span: marked_yaml::Span,
    value: IndexMap<RenderedScalarNode, RenderedNode>,
}

impl Serialize for RenderedMappingNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl RenderedMappingNode {
    pub fn new(span: marked_yaml::Span, value: IndexMap<RenderedScalarNode, RenderedNode>) -> Self {
        Self { span, value }
    }
}

impl HasSpan for RenderedMappingNode {
    fn span(&self) -> &marked_yaml::Span {
        &self.span
    }
}

impl PartialEq for RenderedMappingNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for RenderedMappingNode {}

impl Hash for RenderedMappingNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.keys().for_each(|k| k.hash(state));
    }
}

impl fmt::Debug for RenderedMappingNode {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("RenderedMappingNode");
        if include_span {
            debug.field("span", &self.span);
        }
        debug.field("value", &self.value).finish()
    }
}

impl From<IndexMap<RenderedScalarNode, RenderedNode>> for RenderedMappingNode {
    fn from(value: IndexMap<RenderedScalarNode, RenderedNode>) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value)
    }
}

impl TryFrom<marked_yaml::types::MarkedMappingNode> for RenderedMappingNode {
    type Error = PartialParsingError;

    fn try_from(value: marked_yaml::types::MarkedMappingNode) -> Result<Self, Self::Error> {
        let val: Result<IndexMap<_, _>, _> = value
            .iter()
            .map(|(key, value)| match RenderedNode::try_from(value) {
                Ok(v) => Ok((key.into(), v)),
                Err(e) => Err(e),
            })
            .collect();

        Ok(Self::new(*value.span(), val?))
    }
}

impl ops::Deref for RenderedMappingNode {
    type Target = IndexMap<RenderedScalarNode, RenderedNode>;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl ops::DerefMut for RenderedMappingNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl Render<RenderedNode> for Node {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<RenderedNode, Vec<PartialParsingError>> {
        match self {
            Node::Scalar(s) => s.render(jinja, name),
            Node::Mapping(m) => m.render(jinja, name),
            Node::Sequence(s) => s.render(jinja, name),
            Node::Null(n) => Ok(RenderedNode::Null(RenderedScalarNode::new(
                *n.span(),
                n.as_str().to_owned(),
                n.as_str().to_owned(),
                false,
            ))),
        }
    }
}

impl Render<RenderedNode> for ScalarNode {
    fn render(&self, jinja: &Jinja, _name: &str) -> Result<RenderedNode, Vec<PartialParsingError>> {
        let rendered = jinja.render_str(self.as_str()).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::JinjaRendering(err),
                label = jinja_error_to_label(&err),
            )]
        })?;
        // unsure whether this should be allowed to coerce // check if it's quoted?
        let rendered = RenderedScalarNode::new(
            *self.span(),
            self.as_str().to_string(),
            rendered,
            self.may_coerce,
        );

        if rendered.is_empty() {
            Ok(RenderedNode::Null(rendered))
        } else {
            Ok(RenderedNode::Scalar(rendered))
        }
    }
}

impl Render<Option<RenderedNode>> for ScalarNode {
    fn render(
        &self,
        jinja: &Jinja,
        _name: &str,
    ) -> Result<Option<RenderedNode>, Vec<PartialParsingError>> {
        let rendered = jinja.render_str(self.as_str()).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::JinjaRendering(err),
                label = format!("Rendering error: {}", err.kind())
            )]
        })?;

        let rendered = RenderedScalarNode::new(
            *self.span(),
            self.as_str().to_string(),
            rendered,
            self.may_coerce,
        );

        if rendered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(RenderedNode::Scalar(rendered)))
        }
    }
}

impl Render<RenderedNode> for MappingNode {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<RenderedNode, Vec<PartialParsingError>> {
        let rendered = self.render(jinja, name)?;

        Ok(RenderedNode::Mapping(rendered))
    }
}

impl Render<RenderedMappingNode> for MappingNode {
    fn render(
        &self,
        jinja: &Jinja,
        name: &str,
    ) -> Result<RenderedMappingNode, Vec<PartialParsingError>> {
        let mut rendered = IndexMap::new();

        for (key, value) in self.iter() {
            let key = RenderedScalarNode::new(
                *key.span(),
                key.as_str().to_owned(),
                key.as_str().to_owned(),
                false,
            );
            let value: RenderedNode = value.render(jinja, &format!("{name}.{}", key.as_str()))?;
            if value.is_null() {
                continue;
            }
            rendered.insert(key, value);
        }

        let rendered = RenderedMappingNode::new(*self.span(), rendered);
        Ok(rendered)
    }
}

impl Render<RenderedNode> for SequenceNode {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<RenderedNode, Vec<PartialParsingError>> {
        let rendered: RenderedSequenceNode = self.render(jinja, name)?;

        if rendered.is_empty() {
            return Ok(RenderedNode::Null(RenderedScalarNode::new(
                *self.span(),
                String::new(),
                String::new(),
                false,
            )));
        }

        Ok(RenderedNode::Sequence(rendered))
    }
}

impl Render<RenderedSequenceNode> for SequenceNode {
    fn render(
        &self,
        jinja: &Jinja,
        name: &str,
    ) -> Result<RenderedSequenceNode, Vec<PartialParsingError>> {
        let mut rendered = Vec::with_capacity(self.len());

        for item in self.iter() {
            let item: RenderedSequenceNode = item.render(jinja, name)?;
            rendered.extend(item.iter().cloned());
        }

        let rendered = RenderedSequenceNode::new(*self.span(), rendered);

        Ok(rendered)
    }
}

impl Render<RenderedSequenceNode> for SequenceNodeInternal {
    fn render(
        &self,
        jinja: &Jinja,
        name: &str,
    ) -> Result<RenderedSequenceNode, Vec<crate::recipe::error::PartialParsingError>> {
        let mut rendered = Vec::new();
        match self {
            SequenceNodeInternal::Simple(node) => rendered.push(node.render(jinja, name)?),
            SequenceNodeInternal::Conditional(if_sel) => {
                let if_res = if_sel.process(jinja)?;
                if let Some(if_res) = if_res {
                    let rend: RenderedNode = if_res.render(jinja, name)?;

                    if let Some(rend) = rend.as_sequence() {
                        rendered.extend(rend.iter().cloned());
                    } else {
                        rendered.push(rend);
                    }
                }
            }
        }

        // filter out all null values
        rendered.retain(|item| !matches!(item, RenderedNode::Null(_)));

        Ok(RenderedSequenceNode::from(rendered))
    }
}
