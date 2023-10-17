//! Module to define an `Node` type that is specific to the first stage of the
//! new Conda recipe format parser.

use std::{fmt, hash::Hash, ops};

use linked_hash_map::LinkedHashMap;
use marked_yaml::types::MarkedScalarNode;

use crate::_partialerror;
use crate::recipe::{
    error::{ErrorKind, PartialParsingError},
    jinja::Jinja,
};

/// A marked new Conda Recipe YAML node
///
/// This is a reinterpretation of the [`marked_yaml::Node`] type that is specific
/// for the first stage of the new Conda recipe format parser. This type handles
/// the `if / then / else` selector (or if-selector for simplicity) as a special
/// case of the sequence node, i.e., the occurences of if-selector in the recipe
/// are syntactically parsed in the conversion of [`marked_yaml::Node`] to this type.
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
}

impl Node {
    /// Retrieve the Span from the contained Node
    pub fn span(&self) -> &marked_yaml::Span {
        match self {
            Self::Mapping(map) => map.span(),
            Self::Scalar(scalar) => scalar.span(),
            Self::Sequence(seq) => seq.span(),
        }
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

impl From<ScalarNode> for Node {
    fn from(value: ScalarNode) -> Self {
        Self::Scalar(value)
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

impl From<LinkedHashMap<ScalarNode, Node>> for Node {
    fn from(value: LinkedHashMap<ScalarNode, Node>) -> Self {
        Self::Mapping(MappingNode::from(value))
    }
}

impl From<String> for Node {
    fn from(value: String) -> Self {
        Self::Scalar(ScalarNode::from(value))
    }
}

impl From<&str> for Node {
    fn from(value: &str) -> Self {
        Self::Scalar(ScalarNode::from(value.to_owned()))
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
            marked_yaml::Node::Scalar(scalar) => Ok(Self::Scalar(scalar.into())),
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
}

impl ScalarNode {
    pub fn new(span: marked_yaml::Span, value: String) -> Self {
        Self { span, value }
    }

    pub fn span(&self) -> &marked_yaml::Span {
        &self.span
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
        match self.value.as_str() {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        }
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
        Self::new(marked_yaml::Span::new_blank(), value.to_owned())
    }
}

impl From<String> for ScalarNode {
    /// Convert from any owned string into a node
    fn from(value: String) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value)
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
        Self::new(*value.span(), value.as_str().to_owned())
    }
}

impl From<bool> for ScalarNode {
    /// Convert from a boolean into a node
    fn from(value: bool) -> Self {
        if value {
            "true".into()
        } else {
            "false".into()
        }
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
    pub fn new(span: marked_yaml::Span, value: Vec<SequenceNodeInternal>) -> Self {
        Self { span, value }
    }

    pub fn span(&self) -> &marked_yaml::Span {
        &self.span
    }

    /// Check if this sequence node is only conditional.
    ///
    /// This is convenient for places that accept if-selectors but don't accept simple sequence.
    pub fn is_only_conditional(&self) -> bool {
        self.value
            .iter()
            .all(|v| matches!(v, SequenceNodeInternal::Conditional(_)))
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
/// Because ther is an example that on the `context` key-value definition, a later
/// key was defined as a jinja string using previous values, we need to care about
/// insertion order we use [`LinkedHashMap`] for this.
///
/// **NOTE**: Nodes are considered equal even if they don't come from the same
/// place.  *i.e. their spans are ignored for equality and hashing*
#[derive(Clone)]
pub struct MappingNode {
    span: marked_yaml::Span,
    value: LinkedHashMap<ScalarNode, Node>,
}

impl MappingNode {
    pub fn new(span: marked_yaml::Span, value: LinkedHashMap<ScalarNode, Node>) -> Self {
        Self { span, value }
    }

    pub fn span(&self) -> &marked_yaml::Span {
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
        self.value.hash(state);
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

impl From<LinkedHashMap<ScalarNode, Node>> for MappingNode {
    fn from(value: LinkedHashMap<ScalarNode, Node>) -> Self {
        Self::new(marked_yaml::Span::new_blank(), value)
    }
}

impl TryFrom<marked_yaml::types::MarkedMappingNode> for MappingNode {
    type Error = PartialParsingError;

    fn try_from(value: marked_yaml::types::MarkedMappingNode) -> Result<Self, Self::Error> {
        let val: Result<LinkedHashMap<_, _>, _> = value
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
    type Target = LinkedHashMap<ScalarNode, Node>;

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
pub enum SequenceNodeInternal {
    /// A simple node
    Simple(Node),
    /// A conditional node
    Conditional(IfSelector),
}

impl SequenceNodeInternal {
    pub fn span(&self) -> &marked_yaml::Span {
        match self {
            Self::Simple(node) => node.span(),
            Self::Conditional(selector) => selector.span(),
        }
    }

    /// Process the sequence node using the given jinja environment, returning the chosen node.
    pub fn process(&self, jinja: &Jinja) -> Result<Option<Node>, PartialParsingError> {
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

    pub fn span(&self) -> &marked_yaml::Span {
        &self.span
    }

    /// Process the if-selector using the given jinja environment, returning the chosen node.
    pub fn process(&self, jinja: &Jinja) -> Result<Option<Node>, PartialParsingError> {
        let cond = jinja.eval(self.cond.as_str()).map_err(|err| {
            _partialerror!(
                *self.cond.span(),
                ErrorKind::JinjaRendering(err),
                label = "error evaluating if-selector condition"
            )
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
