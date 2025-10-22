//! Core types for YAML parsing with Jinja2 template support

use marked_yaml::Span;
use rattler_build_jinja::{JinjaExpression, JinjaTemplate};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A value that can be either a concrete value or a Jinja2 template
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Value<T> {
    /// The inner value - either concrete or template
    inner: ValueInner<T>,
    /// Optional span for error reporting
    #[serde(skip)]
    span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ValueInner<T> {
    Concrete(T),
    Template(JinjaTemplate),
}

impl<T> Value<T> {
    /// Create a new concrete value
    pub fn new_concrete(value: T, span: Option<Span>) -> Self {
        Self {
            inner: ValueInner::Concrete(value),
            span,
        }
    }

    /// Create a new template value
    pub fn new_template(template: JinjaTemplate, span: Option<Span>) -> Self {
        Self {
            inner: ValueInner::Template(template),
            span,
        }
    }

    /// Check if this is a template
    pub fn is_template(&self) -> bool {
        matches!(self.inner, ValueInner::Template(_))
    }

    /// Check if this is a concrete value
    pub fn is_concrete(&self) -> bool {
        matches!(self.inner, ValueInner::Concrete(_))
    }

    /// Get the concrete value if available
    pub fn as_concrete(&self) -> Option<&T> {
        match &self.inner {
            ValueInner::Concrete(v) => Some(v),
            ValueInner::Template(_) => None,
        }
    }

    /// Get the template if available
    pub fn as_template(&self) -> Option<&JinjaTemplate> {
        match &self.inner {
            ValueInner::Concrete(_) => None,
            ValueInner::Template(t) => Some(t),
        }
    }

    /// Get the span for error reporting
    pub fn span(&self) -> Option<&Span> {
        self.span.as_ref()
    }

    /// Convert into the inner value, if concrete
    pub fn into_concrete(self) -> Option<T> {
        match self.inner {
            ValueInner::Concrete(v) => Some(v),
            ValueInner::Template(_) => None,
        }
    }

    /// Decompose into inner value and span
    pub fn into_parts(self) -> (ValueInner<T>, Option<Span>) {
        (self.inner, self.span)
    }

    /// Get a reference to the inner value type
    pub fn inner(&self) -> &ValueInner<T> {
        &self.inner
    }
}

impl<T> ValueInner<T> {
    /// Check if this is a concrete value
    pub fn is_concrete(&self) -> bool {
        matches!(self, ValueInner::Concrete(_))
    }

    /// Check if this is a template
    pub fn is_template(&self) -> bool {
        matches!(self, ValueInner::Template(_))
    }
}

impl<T: fmt::Display> fmt::Display for Value<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            ValueInner::Concrete(v) => write!(f, "{}", v),
            ValueInner::Template(t) => write!(f, "{}", t),
        }
    }
}

/// A list or a single item
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListOrItem<T>(Vec<T>);

impl<T> ListOrItem<T> {
    /// Create from a list of items
    pub fn new(items: Vec<T>) -> Self {
        Self(items)
    }

    /// Create from a single item
    pub fn single(item: T) -> Self {
        Self(vec![item])
    }

    /// Get the number of items
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get an iterator over the items
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }

    /// Convert to a vector
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Get as a slice
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> IntoIterator for ListOrItem<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a ListOrItem<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// A conditional with if/then/else branches
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conditional<T> {
    /// The condition to evaluate
    pub condition: JinjaExpression,
    /// The values to use if condition is true
    pub then: ListOrItem<Value<T>>,
    /// The values to use if condition is false
    pub else_value: Option<ListOrItem<Value<T>>>,
}

/// An item in a conditional list - either a value or a conditional
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Item<T> {
    /// A concrete value or template
    Value(Value<T>),
    /// A conditional if/then/else
    Conditional(Conditional<T>),
}

impl<T> Item<T> {
    /// Check if this is a value
    pub fn is_value(&self) -> bool {
        matches!(self, Item::Value(_))
    }

    /// Check if this is a conditional
    pub fn is_conditional(&self) -> bool {
        matches!(self, Item::Conditional(_))
    }

    /// Get the value if this is a value item
    pub fn as_value(&self) -> Option<&Value<T>> {
        match self {
            Item::Value(v) => Some(v),
            Item::Conditional(_) => None,
        }
    }

    /// Get the conditional if this is a conditional item
    pub fn as_conditional(&self) -> Option<&Conditional<T>> {
        match self {
            Item::Value(_) => None,
            Item::Conditional(c) => Some(c),
        }
    }
}

/// A list that may contain conditionals
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalList<T> {
    items: Vec<Item<T>>,
}

impl<T> ConditionalList<T> {
    /// Create a new conditional list
    pub fn new(items: Vec<Item<T>>) -> Self {
        Self { items }
    }

    /// Create an empty conditional list
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    /// Get the number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get an iterator over the items
    pub fn iter(&self) -> impl Iterator<Item = &Item<T>> {
        self.items.iter()
    }

    /// Get a mutable iterator over the items
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Item<T>> {
        self.items.iter_mut()
    }

    /// Convert to a vector
    pub fn into_vec(self) -> Vec<Item<T>> {
        self.items
    }

    /// Get as a slice
    pub fn as_slice(&self) -> &[Item<T>] {
        &self.items
    }
}

impl<T> IntoIterator for ConditionalList<T> {
    type Item = Item<T>;
    type IntoIter = std::vec::IntoIter<Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a ConditionalList<T> {
    type Item = &'a Item<T>;
    type IntoIter = std::slice::Iter<'a, Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T> Default for ConditionalList<T> {
    fn default() -> Self {
        Self::empty()
    }
}
