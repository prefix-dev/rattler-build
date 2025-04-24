//! Module to define an `Node` type that is specific to the first stage of the
//! new Conda recipe format parser.

use core::fmt::Display;
use std::{collections::BTreeMap, fmt, hash::Hash, ops, path::PathBuf, str::FromStr};

use indexmap::{IndexMap, IndexSet};
use marked_yaml::{
    Span,
    loader::{LoaderOptions, parse_yaml_with_options},
    types::MarkedScalarNode,
};
use rattler_conda_types::VersionWithSource;
use url::Url;

use crate::{
    _partialerror,
    normalized_key::NormalizedKey,
    recipe::{
        error::{ErrorKind, ParsingError, PartialParsingError, jinja_error_to_label},
        jinja::Jinja,
    },
    source_code::SourceCode,
};

mod rendered;
pub use rendered::{RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode};

use super::Render;

/// A marked new Conda Recipe YAML node
///
/// This is a reinterpretation of the [`marked_yaml::Node`] type that is
/// specific for the first stage of the new Conda recipe format parser. This
/// type handles the `if / then / else` selector (or if-selector for simplicity)
/// as a special case of the sequence node, i.e., the occurrences of if-selector
/// in the recipe are syntactically parsed in the conversion of
/// [`marked_yaml::Node`] to this type.
///
/// **CAUTION:** The user of this type that is responsible to handle the if the
/// if-selector has semantic validity or not.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the
/// same place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    /// A YAML scalar
    ///
    /// You can test if a node is a scalar, and retrieve it as one if you
    /// so wish.
    Scalar(ScalarNode),
    /// A YAML mapping
    ///
    /// You can test if a node is a mapping, and retrieve it as one if you
    /// so wish.
    Mapping(MappingNode),
    /// A YAML sequence
    ///
    /// You can test if a node is a sequence, and retrieve it as one if you
    /// so wish.
    Sequence(SequenceNode),
    /// A YAML null
    ///
    /// This is a special case of a scalar node, but is treated as its own
    /// type here for convenience.
    Null(ScalarNode),
}

/// Parse YAML from a string and return a Node representing the content.
pub fn parse_yaml<S: SourceCode>(
    init_span_index: usize,
    src: S,
) -> Result<marked_yaml::Node, ParsingError<S>> {
    let options = LoaderOptions::default()
        .error_on_duplicate_keys(true)
        .prevent_coercion(true);
    let yaml = parse_yaml_with_options(init_span_index, src.clone(), options)
        .map_err(|err| crate::recipe::error::load_error_handler(src, err))?;

    Ok(yaml)
}

impl Node {
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

    /// Check if this node is a mapping
    pub fn is_mapping(&self) -> bool {
        matches!(self, Self::Mapping(_))
    }

    /// Check if this node is a scalar
    pub fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar(_))
    }

    /// Check if this node is a sequence
    pub fn is_sequence(&self) -> bool {
        matches!(self, Self::Sequence(_))
    }

    /// Retrieve the scalar from this node if there is one
    pub fn as_scalar(&self) -> Option<&ScalarNode> {
        match self {
            Node::Scalar(msn) => Some(msn),
            _ => None,
        }
    }

    /// Retrieve the sequence from this node if there is one
    pub fn as_sequence(&self) -> Option<&SequenceNode> {
        match self {
            Node::Sequence(msn) => Some(msn),
            _ => None,
        }
    }

    /// Retrieve the mapping from this node if there is one
    pub fn as_mapping(&self) -> Option<&MappingNode> {
        match self {
            Node::Mapping(mmn) => Some(mmn),
            _ => None,
        }
    }
}

impl Render<Node> for Node {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Node, Vec<PartialParsingError>> {
        match self {
            Node::Scalar(s) => s.render(jinja, name),
            Node::Mapping(m) => m.render(jinja, name),
            Node::Sequence(s) => s.render(jinja, name),
            Node::Null(n) => Ok(Node::Null(n.clone())),
        }
    }
}

impl Render<Node> for ScalarNode {
    fn render(&self, jinja: &Jinja, _name: &str) -> Result<Node, Vec<PartialParsingError>> {
        let rendered = jinja.render_str(self.as_str()).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::JinjaRendering(err),
                label = jinja_error_to_label(&err)
            )]
        })?;

        Ok(Node::from(ScalarNode::new(
            *self.span(),
            rendered,
            self.may_coerce,
        )))
    }
}

impl Render<Option<ScalarNode>> for ScalarNode {
    fn render(
        &self,
        jinja: &Jinja,
        _name: &str,
    ) -> Result<std::option::Option<ScalarNode>, Vec<PartialParsingError>> {
        let rendered = jinja.render_str(self.as_str()).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::JinjaRendering(err),
                label = jinja_error_to_label(&err)
            )]
        })?;

        if rendered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ScalarNode::new(
                *self.span(),
                rendered,
                self.may_coerce,
            )))
        }
    }
}

impl Render<Node> for MappingNode {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Node, Vec<PartialParsingError>> {
        let mut rendered = IndexMap::new();

        for (key, value) in self.iter() {
            rendered.insert(
                key.clone(),
                value.render(jinja, &format!("{name} {}", key.as_str()))?,
            );
        }

        let map = MappingNode::new(*self.span(), rendered);

        Ok(Node::Mapping(map))
    }
}

impl Render<Node> for SequenceNode {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Node, Vec<PartialParsingError>> {
        let mut rendered = Vec::new();

        for item in self.iter() {
            rendered.push(item.render(jinja, name)?);
        }

        let seq = SequenceNode::new(*self.span(), rendered);
        Ok(Node::Sequence(seq))
    }
}

impl Render<Node> for SequenceNodeInternal {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Node, Vec<PartialParsingError>> {
        match self {
            Self::Simple(n) => n.render(jinja, name),
            Self::Conditional(if_sel) => {
                let if_res = if_sel.process(jinja)?;
                if let Some(if_res) = if_res {
                    Ok(if_res.render(jinja, name)?)
                } else {
                    Ok(Node::Null(ScalarNode::new(
                        *self.span(),
                        "".to_owned(),
                        false,
                    )))
                }
            }
        }
    }
}

impl Render<SequenceNodeInternal> for SequenceNodeInternal {
    fn render(
        &self,
        jinja: &Jinja,
        name: &str,
    ) -> Result<SequenceNodeInternal, Vec<PartialParsingError>> {
        match self {
            Self::Simple(n) => Ok(Self::Simple(n.render(jinja, name)?)),
            Self::Conditional(if_sel) => {
                let if_res = if_sel.process(jinja)?;
                if let Some(if_res) = if_res {
                    Ok(Self::Simple(if_res.render(jinja, name)?))
                } else {
                    Ok(Self::Simple(Node::Null(ScalarNode::new(
                        *self.span(),
                        "".to_owned(),
                        false,
                    ))))
                }
            }
        }
    }
}

/// A trait that defines that the implementer has an associated span.
pub trait HasSpan {
    /// Get the span of the implementer
    fn span(&self) -> &marked_yaml::Span;
}

impl HasSpan for Node {
    fn span(&self) -> &Span {
        match self {
            Self::Mapping(map) => map.span(),
            Self::Scalar(scalar) => scalar.span(),
            Self::Sequence(seq) => seq.span(),
            Self::Null(s) => s.span(),
        }
    }
}

impl<'i> TryFrom<&'i Node> for &'i ScalarNode {
    type Error = ();

    fn try_from(value: &'i Node) -> Result<Self, Self::Error> {
        value.as_scalar().ok_or(())
    }
}

impl From<ScalarNode> for Node {
    fn from(value: ScalarNode) -> Self {
        match value.as_str() {
            "null" | "~" | "" => Self::Null(value),
            _ => Self::Scalar(value),
        }
    }
}

impl From<MappingNode> for Node {
    fn from(value: MappingNode) -> Self {
        Self::Mapping(value)
    }
}

impl From<SequenceNode> for Node {
    fn from(value: SequenceNode) -> Self {
        Self::Sequence(value)
    }
}

impl From<Vec<SequenceNodeInternal>> for Node {
    fn from(value: Vec<SequenceNodeInternal>) -> Self {
        Self::Sequence(SequenceNode::from(value))
    }
}

impl From<IndexMap<ScalarNode, Node>> for Node {
    fn from(value: IndexMap<ScalarNode, Node>) -> Self {
        Self::Mapping(MappingNode::from(value))
    }
}

impl From<String> for Node {
    fn from(value: String) -> Self {
        Self::from(&*value)
    }
}

impl From<&str> for Node {
    fn from(value: &str) -> Self {
        match value {
            "null" | "~" | "" => Self::Null(ScalarNode::from(value)),
            _ => Self::Scalar(ScalarNode::from(value)),
        }
    }
}

impl From<&MarkedScalarNode> for Node {
    fn from(value: &MarkedScalarNode) -> Self {
        let scalar: ScalarNode = value.into();
        scalar.into()
    }
}

impl TryFrom<marked_yaml::Node> for Node {
    type Error = PartialParsingError;

    fn try_from(value: marked_yaml::Node) -> Result<Self, Self::Error> {
        Node::try_from(&value)
    }
}

impl TryFrom<&marked_yaml::Node> for Node {
    type Error = PartialParsingError;

    fn try_from(value: &marked_yaml::Node) -> Result<Self, Self::Error> {
        match value {
            marked_yaml::Node::Scalar(scalar) => Ok(Self::from(scalar)),
            marked_yaml::Node::Mapping(map) => {
                Ok(Self::Mapping(MappingNode::try_from(map.clone())?))
            }
            marked_yaml::Node::Sequence(seq) => {
                Ok(Self::Sequence(SequenceNode::try_from(seq.clone())?))
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
pub struct ScalarNode {
    span: marked_yaml::Span,
    value: String,
    may_coerce: bool,
}

impl ScalarNode {
    /// Create a new scalar node with a span
    pub fn new(span: marked_yaml::Span, value: String, may_coerce: bool) -> Self {
        Self {
            span,
            value,
            may_coerce,
        }
    }

    /// Treat the scalar node as a string
    ///
    /// Since scalars are always stringish, this is always safe.
    pub fn as_str(&self) -> &str {
        &self.value
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

    /// Convert the scalar node to an integer and follow coercion rules
    pub fn as_integer(&self) -> Option<i64> {
        if !self.may_coerce {
            return None;
        }
        self.value.parse().ok()
    }
}

impl HasSpan for ScalarNode {
    fn span(&self) -> &Span {
        &self.span
    }
}

impl PartialEq for ScalarNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
    #[allow(clippy::partialeq_ne_impl)]
    fn ne(&self, other: &Self) -> bool {
        self.value != other.value
    }
}
impl Eq for ScalarNode {}

impl Hash for ScalarNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl fmt::Debug for ScalarNode {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("ScalarNode");
        if include_span {
            debug.field("span", &self.span);
        }
        debug.field("value", &self.value).finish()
    }
}

impl<'a> From<&'a str> for ScalarNode {
    /// Convert from any borrowed string into a node
    fn from(value: &'a str) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value.to_owned(), false)
    }
}

impl From<String> for ScalarNode {
    /// Convert from any owned string into a node
    fn from(value: String) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value, false)
    }
}

impl ops::Deref for ScalarNode {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl std::borrow::Borrow<str> for ScalarNode {
    fn borrow(&self) -> &str {
        &self.value
    }
}

impl From<MarkedScalarNode> for ScalarNode {
    fn from(value: MarkedScalarNode) -> Self {
        Self::from(&value)
    }
}

impl From<&MarkedScalarNode> for ScalarNode {
    fn from(value: &MarkedScalarNode) -> Self {
        Self::new(*value.span(), value.as_str().to_owned(), value.may_coerce())
    }
}

impl From<bool> for ScalarNode {
    /// Convert from a boolean into a node
    fn from(value: bool) -> Self {
        if value { "true".into() } else { "false".into() }
    }
}

macro_rules! scalar_from_to_number {
    ($t:ident, $as:ident) => {
        impl From<$t> for ScalarNode {
            #[doc = "Convert from "]
            #[doc = stringify!($t)]
            #[doc = r#" into a node"#]
            fn from(value: $t) -> Self {
                format!("{}", value).into()
            }
        }

        impl ScalarNode {
            #[doc = "Treat the scalar node as "]
            #[doc = stringify!($t)]
            #[doc = r#".

If this scalar node's value can be represented properly as
a number of the right kind then return it.  This is essentially
a shortcut for using the `FromStr` trait on the return value of
`.as_str()`."#]
            pub fn $as(&self) -> Option<$t> {
                use std::str::FromStr;
                if !self.may_coerce {
                    return None;
                }
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
pub struct SequenceNode {
    span: marked_yaml::Span,
    value: Vec<SequenceNodeInternal>,
}

impl SequenceNode {
    /// Create a new sequence node with a span
    pub fn new(span: marked_yaml::Span, value: Vec<SequenceNodeInternal>) -> Self {
        Self { span, value }
    }

    /// Check if this sequence node is only conditional.
    ///
    /// This is convenient for places that accept if-selectors but don't accept
    /// simple sequence.
    pub fn is_only_conditional(&self) -> bool {
        self.value
            .iter()
            .all(|v| matches!(v, SequenceNodeInternal::Conditional(_)))
    }
}

impl HasSpan for SequenceNode {
    fn span(&self) -> &marked_yaml::Span {
        &self.span
    }
}

impl PartialEq for SequenceNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl Eq for SequenceNode {}

impl Hash for SequenceNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl From<Vec<SequenceNodeInternal>> for SequenceNode {
    fn from(value: Vec<SequenceNodeInternal>) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value)
    }
}

impl TryFrom<marked_yaml::types::MarkedSequenceNode> for SequenceNode {
    type Error = PartialParsingError;

    fn try_from(node: marked_yaml::types::MarkedSequenceNode) -> Result<Self, Self::Error> {
        let mut value = Vec::with_capacity(node.len());

        for item in node.iter() {
            value.push(SequenceNodeInternal::try_from(item.clone())?);
        }

        Ok(Self::new(*node.span(), value))
    }
}

impl ops::Deref for SequenceNode {
    type Target = Vec<SequenceNodeInternal>;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl ops::DerefMut for SequenceNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl fmt::Debug for SequenceNode {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("SequenceNode");
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
/// Because there is an example that on the `context` key-value definition, a
/// later key was defined as a jinja string using previous values, we need to
/// care about insertion order we use [`IndexMap`] for this.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the same
/// place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Clone)]
pub struct MappingNode {
    span: marked_yaml::Span,
    value: IndexMap<ScalarNode, Node>,
}

impl MappingNode {
    /// Create a new mapping node with a span
    pub fn new(span: marked_yaml::Span, value: IndexMap<ScalarNode, Node>) -> Self {
        Self { span, value }
    }
}

impl HasSpan for MappingNode {
    fn span(&self) -> &marked_yaml::Span {
        &self.span
    }
}

impl PartialEq for MappingNode {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for MappingNode {}

impl Hash for MappingNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.keys().for_each(|k| k.hash(state));
    }
}

impl fmt::Debug for MappingNode {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("MappingNode");
        if include_span {
            debug.field("span", &self.span);
        }
        debug.field("value", &self.value).finish()
    }
}

impl From<IndexMap<ScalarNode, Node>> for MappingNode {
    fn from(value: IndexMap<ScalarNode, Node>) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value)
    }
}

impl TryFrom<marked_yaml::types::MarkedMappingNode> for MappingNode {
    type Error = PartialParsingError;

    fn try_from(value: marked_yaml::types::MarkedMappingNode) -> Result<Self, Self::Error> {
        let val: Result<IndexMap<_, _>, _> = value
            .iter()
            .map(|(key, value)| match Node::try_from(value) {
                Ok(v) => Ok((key.into(), v)),
                Err(e) => Err(e),
            })
            .collect();

        Ok(Self::new(*value.span(), val?))
    }
}

impl ops::Deref for MappingNode {
    type Target = IndexMap<ScalarNode, Node>;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl ops::DerefMut for MappingNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

/// Special internal representation of the sequence node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(clippy::large_enum_variant)]
pub enum SequenceNodeInternal {
    /// A simple node
    Simple(Node),
    /// A conditional node
    Conditional(IfSelector),
}

impl SequenceNodeInternal {
    /// Get the span of the entire sequence node
    pub fn span(&self) -> &marked_yaml::Span {
        match self {
            Self::Simple(node) => node.span(),
            Self::Conditional(selector) => selector.span(),
        }
    }

    /// Process the sequence node using the given jinja environment, returning
    /// the chosen node.
    pub fn process(&self, jinja: &Jinja) -> Result<Option<Node>, Vec<PartialParsingError>> {
        match self {
            Self::Simple(node) => Ok(Some(node.clone())),
            Self::Conditional(selector) => selector.process(jinja),
        }
    }
}

impl TryFrom<marked_yaml::Node> for SequenceNodeInternal {
    type Error = PartialParsingError;

    fn try_from(value: marked_yaml::Node) -> Result<Self, Self::Error> {
        match value {
            marked_yaml::Node::Scalar(s) => Ok(Self::Simple(Node::Scalar(ScalarNode::from(s)))),
            marked_yaml::Node::Mapping(map) => {
                if let Some((key, val)) = map.front() {
                    if key.as_str() == "if" {
                        let span = *map.span();
                        let cond = if let marked_yaml::Node::Scalar(s) = val {
                            s.into()
                        } else {
                            return Err(_partialerror!(
                                *val.span(),
                                ErrorKind::IfSelectorConditionNotScalar,
                                label = "if-selector condition must be a scalar"
                            ));
                        };

                        let then = if let Some(t) = map.get("then") {
                            Node::try_from(t)?
                        } else {
                            return Err(_partialerror!(
                                span,
                                ErrorKind::IfSelectorMissingThen,
                                label = "if-selector is missing `then` logic"
                            ));
                        };

                        let otherwise = map.get("else").map(Node::try_from);

                        let otherwise = match otherwise {
                            Some(Ok(v)) => Some(v),
                            Some(Err(e)) => return Err(e),
                            None => None,
                        };

                        Ok(Self::Conditional(IfSelector::new(
                            cond, then, otherwise, span,
                        )))
                    } else {
                        Ok(Self::Simple(Node::Mapping(MappingNode::try_from(map)?)))
                    }
                } else {
                    Ok(Self::Simple(Node::Mapping(MappingNode::try_from(map)?)))
                }
            }
            marked_yaml::Node::Sequence(seq) => {
                Ok(Self::Simple(Node::Sequence(SequenceNode::try_from(seq)?)))
            }
        }
    }
}

/// Representation of the `if / then / else` selector in the recipe.
#[derive(Clone)]
pub struct IfSelector {
    pub(crate) cond: ScalarNode,
    pub(crate) then: Node,
    pub(crate) otherwise: Option<Node>,
    pub(crate) span: marked_yaml::Span,
}

impl IfSelector {
    /// Create a new if / then / else (otherwise) selector
    pub fn new(
        cond: ScalarNode,
        then: Node,
        otherwise: Option<Node>,
        span: marked_yaml::Span,
    ) -> Self {
        Self {
            cond,
            then,
            otherwise,
            span,
        }
    }

    /// Get the conditional value
    pub fn cond(&self) -> &ScalarNode {
        &self.cond
    }

    /// Get the then value
    pub fn then(&self) -> &Node {
        &self.then
    }

    /// Get the otherwise value if it exists
    pub fn otherwise(&self) -> Option<&Node> {
        self.otherwise.as_ref()
    }

    /// Get the span of the entire if / then / else map
    pub fn span(&self) -> &marked_yaml::Span {
        &self.span
    }

    /// Process the if-selector using the given jinja environment, returning the
    /// chosen node.
    pub fn process(&self, jinja: &Jinja) -> Result<Option<Node>, Vec<PartialParsingError>> {
        let cond = jinja.eval(self.cond.as_str()).map_err(|err| {
            vec![_partialerror!(
                *self.cond.span(),
                ErrorKind::JinjaRendering(err),
                label = err.to_string(),
                help = "error evaluating if-selector condition"
            )]
        })?;

        if cond.is_true() {
            Ok(Some(self.then.clone()))
        } else if let Some(otherwise) = &self.otherwise {
            Ok(Some(otherwise.clone()))
        } else {
            Ok(None)
        }
    }
}

impl PartialEq for IfSelector {
    fn eq(&self, other: &Self) -> bool {
        self.cond == other.cond && self.then == other.then && self.otherwise == other.otherwise
    }
}

impl Eq for IfSelector {}

impl Hash for IfSelector {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.cond.hash(state);
        self.then.hash(state);
        self.otherwise.hash(state);
    }
}

impl fmt::Debug for IfSelector {
    /// To include the span in the debug output, use `+` as the sign.
    ///
    /// E.x.: `{:+?}` or `{:+#?}
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let include_span = f.sign_plus();
        let mut debug = f.debug_struct("IfSelector");
        if include_span {
            debug.field("span", &self.span);
        }
        debug
            .field("cond", &self.cond)
            .field("then", &self.then)
            .field("otherwise", &self.otherwise)
            .finish()
    }
}

/// A trait that defines that the implementer can be converted from a node.
pub trait TryConvertNode<T> {
    /// Try to convert the implementer from a node.
    fn try_convert(&self, name: &str) -> Result<T, Vec<PartialParsingError>>;
}

impl<'a> TryConvertNode<&'a ScalarNode> for &'a Node {
    fn try_convert(&self, name: &str) -> Result<&'a ScalarNode, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a scalar value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
    }
}

impl<T: Clone> TryConvertNode<T> for T {
    fn try_convert(&self, _: &str) -> Result<T, Vec<PartialParsingError>> {
        Ok(self.clone())
    }
}

impl TryConvertNode<i32> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<i32, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a scalar value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<bool> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<bool, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a scalar value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<bool> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<bool, Vec<PartialParsingError>> {
        self.as_bool()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a boolean value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
    }
}

impl TryConvertNode<u64> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<u64, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a scalar value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<u64> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<u64, Vec<PartialParsingError>> {
        self.as_str()
            .parse()
            .map_err(|err| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::from(err),
                    label = format!("failed to parse `{}` as unsigned integer", self.as_str())
                )
            })
            .map_err(|e| vec![e])
    }
}

impl TryConvertNode<i32> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<i32, Vec<PartialParsingError>> {
        self.as_str()
            .parse()
            .map_err(|err| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::from(err),
                    label = format!("failed to parse `{}` as integer", self.as_str())
                )
            })
            .map_err(|e| vec![e])
    }
}

impl<'a> TryConvertNode<&'a RenderedScalarNode> for &'a RenderedNode {
    fn try_convert(&self, name: &str) -> Result<&'a RenderedScalarNode, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a scalar value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
    }
}

impl TryConvertNode<String> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<String, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )
            })
            .map(|s| s.as_str().to_owned())
            .map_err(|e| vec![e])
    }
}

impl TryConvertNode<String> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<String, Vec<PartialParsingError>> {
        Ok(self.as_str().to_owned())
    }
}

impl TryConvertNode<NormalizedKey> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<NormalizedKey, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<NormalizedKey> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<NormalizedKey, Vec<PartialParsingError>> {
        Ok(self.as_str().into())
    }
}

impl TryConvertNode<PathBuf> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PathBuf, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<PathBuf> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<PathBuf, Vec<PartialParsingError>> {
        Ok(PathBuf::from(self.as_str()))
    }
}

impl TryConvertNode<RenderedScalarNode> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<RenderedScalarNode, Vec<PartialParsingError>> {
        self.as_scalar()
            .cloned()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    help = format!("expected a string value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
    }
}

impl TryConvertNode<Url> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Url, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<Url> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<Url, Vec<PartialParsingError>> {
        Url::parse(self.as_str())
            .map_err(|err| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::from(err),
                    label = "failed to parse URL"
                )
            })
            .map_err(|e| vec![e])
    }
}

impl<T> TryConvertNode<Option<T>> for RenderedNode
where
    RenderedNode: TryConvertNode<T>,
{
    fn try_convert(&self, name: &str) -> Result<Option<T>, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Null(_) => Ok(None),
            _ => Ok(Some(self.try_convert(name)?)),
        }
    }
}

impl<T> TryConvertNode<Option<T>> for RenderedScalarNode
where
    RenderedScalarNode: TryConvertNode<T>,
{
    fn try_convert(&self, name: &str) -> Result<Option<T>, Vec<PartialParsingError>> {
        self.try_convert(name).map(|v| Some(v))
    }
}

impl<T: Hash + Eq> TryConvertNode<IndexSet<T>> for RenderedNode
where
    RenderedNode: TryConvertNode<T>,
    RenderedScalarNode: TryConvertNode<T>,
{
    fn try_convert(&self, name: &str) -> Result<IndexSet<T>, Vec<PartialParsingError>> {
        TryConvertNode::<Vec<T>>::try_convert(self, name).map(|v| v.into_iter().collect())
    }
}

impl<T> TryConvertNode<Vec<T>> for RenderedNode
where
    RenderedNode: TryConvertNode<T>,
    RenderedScalarNode: TryConvertNode<T>,
{
    /// # Caveats
    /// Converting the node into a vector may result in a empty vector if the
    /// node is null.
    ///
    /// If that is not the desired behavior, and you want to handle the case of
    /// a null node differently, specify the result to be `Option<Vec<_>>`
    /// instead.
    ///
    /// Alternatively, you can also specify the result to be `Vec<Option<_>>` to
    /// handle the case of a null node in other ways.
    fn try_convert(&self, name: &str) -> Result<Vec<T>, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Scalar(s) => {
                let item = s.try_convert(name)?;
                Ok(vec![item])
            }
            RenderedNode::Sequence(seq) => seq.iter().map(|item| item.try_convert(name)).collect(),
            RenderedNode::Null(_) => Ok(vec![]),
            RenderedNode::Mapping(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = format!("expected scalar or sequence for {name}")
            )]),
        }
    }
}

impl<T> TryConvertNode<Vec<T>> for RenderedScalarNode
where
    RenderedScalarNode: TryConvertNode<T>,
{
    fn try_convert(&self, name: &str) -> Result<Vec<T>, Vec<PartialParsingError>> {
        self.try_convert(name).map(|v| vec![v])
    }
}

impl<K, V> TryConvertNode<BTreeMap<K, V>> for RenderedNode
where
    K: Ord + Display,
    RenderedScalarNode: TryConvertNode<K>,
    RenderedNode: TryConvertNode<V>,
{
    fn try_convert(&self, name: &str) -> Result<BTreeMap<K, V>, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedMapping,
                    help = format!("expected a mapping for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|m| m.try_convert(name))
    }
}

impl<K, V> TryConvertNode<BTreeMap<K, V>> for RenderedMappingNode
where
    K: Ord + Display,
    RenderedScalarNode: TryConvertNode<K>,
    RenderedNode: TryConvertNode<V>,
{
    fn try_convert(&self, name: &str) -> Result<BTreeMap<K, V>, Vec<PartialParsingError>> {
        let mut map = BTreeMap::new();
        for (key, value) in self.iter() {
            let key = key.try_convert(name)?;
            let value = value.try_convert(name)?;
            map.insert(key, value);
        }
        Ok(map)
    }
}

impl<K, V> TryConvertNode<IndexMap<K, V>> for RenderedNode
where
    K: Ord + Display + Hash,
    RenderedScalarNode: TryConvertNode<K>,
    RenderedNode: TryConvertNode<V>,
{
    fn try_convert(&self, name: &str) -> Result<IndexMap<K, V>, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedMapping,
                    help = format!("expected a mapping for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|m| m.try_convert(name))
    }
}

impl<K, V> TryConvertNode<IndexMap<K, V>> for RenderedMappingNode
where
    K: Ord + Display + Hash,
    RenderedScalarNode: TryConvertNode<K>,
    RenderedNode: TryConvertNode<V>,
{
    fn try_convert(&self, name: &str) -> Result<IndexMap<K, V>, Vec<PartialParsingError>> {
        let mut map = IndexMap::new();
        for (key, value) in self.iter() {
            let key = key.try_convert(name)?;
            let value = value.try_convert(name)?;
            map.insert(key, value);
        }
        Ok(map)
    }
}

impl TryConvertNode<serde_yaml::Value> for RenderedNode {
    fn try_convert(&self, _name: &str) -> Result<serde_yaml::Value, Vec<PartialParsingError>> {
        serde_yaml::to_value(self).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = err.to_string()
            )]
        })
    }
}

impl TryConvertNode<VersionWithSource> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<VersionWithSource, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )
            })
            .map_err(|e| vec![e])
            .and_then(|s| s.try_convert(name))
    }
}

impl TryConvertNode<VersionWithSource> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<VersionWithSource, Vec<PartialParsingError>> {
        let s = self.as_str();
        if s.contains('-') {
            // version is not allowed to contain a `-`
            return Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::InvalidValue((name.to_string(), "version cannot contain `-`".into())),
                label = format!("version `{s}` cannot contain `-` "),
                help = "replace the `-` with `_` or remove it"
            )]);
        }

        VersionWithSource::from_str(self.as_str())
            .map_err(|err| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::InvalidValue((name.to_string(), err.to_string().into())),
                    label = "failed to parse `{name}`",
                )
            })
            .map_err(|e| vec![e])
    }
}
