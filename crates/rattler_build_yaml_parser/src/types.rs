//! Core types for YAML parsing with Jinja2 template support

use marked_yaml::Span;
use rattler_build_jinja::{JinjaExpression, JinjaTemplate};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A value that can be either a concrete value or a Jinja2 template
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Value<T> {
    /// The inner value - either concrete or template
    inner: ValueInner<T>,
    /// Optional span for error reporting (not serialized)
    #[serde(skip)]
    span: Option<Span>,
    /// When true, the evaluated result of a template must be coerced to a string.
    /// This is set when the original YAML scalar was quoted or a block scalar,
    /// signaling that the user intended a string value even if the expression
    /// evaluates to a number or boolean.
    #[serde(skip)]
    force_string: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
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
            force_string: false,
        }
    }

    /// Create a new template value
    pub fn new_template(template: JinjaTemplate, span: Option<Span>) -> Self {
        Self {
            inner: ValueInner::Template(template),
            span,
            force_string: false,
        }
    }

    /// Mark this value as requiring string coercion after template evaluation.
    /// Used when the original YAML scalar was quoted or a block scalar.
    pub fn with_force_string(mut self) -> Self {
        self.force_string = true;
        self
    }

    /// Returns true if the evaluated result should be coerced to a string.
    pub fn force_string(&self) -> bool {
        self.force_string
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

    /// Get the list of variables used in this value
    /// Returns empty vector for concrete values, template variables for templates
    pub fn used_variables(&self) -> Vec<String> {
        match &self.inner {
            ValueInner::Concrete(_) => Vec::new(),
            ValueInner::Template(t) => t.used_variables().to_vec(),
        }
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

impl<T: Default> Default for Value<T> {
    fn default() -> Self {
        Self::new_concrete(T::default(), None)
    }
}

/// A list or a single item
#[derive(Debug, Clone, PartialEq)]
pub struct ListOrItem<T>(Vec<T>);

// Custom serialization: single item => just the item, multiple => array
impl<T: Serialize> Serialize for ListOrItem<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0.as_slice() {
            [single] => single.serialize(serializer),
            multiple => multiple.serialize(serializer),
        }
    }
}

// Custom deserialization: accept either single value or array
impl<'de, T: Deserialize<'de>> Deserialize<'de> for ListOrItem<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::marker::PhantomData;

        struct ListOrItemVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ListOrItemVisitor<T> {
            type Value = ListOrItem<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a single value or a list of values")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(ListOrItem(items))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                T::deserialize(de::value::StrDeserializer::new(v))
                    .map(|item| ListOrItem(vec![item]))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                T::deserialize(de::value::StringDeserializer::new(v))
                    .map(|item| ListOrItem(vec![item]))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                T::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|item| ListOrItem(vec![item]))
            }
        }

        deserializer.deserialize_any(ListOrItemVisitor(PhantomData))
    }
}

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

/// A list of items for then/else branches that supports nested conditionals
#[derive(Debug, Clone, PartialEq)]
pub struct NestedItemList<T> {
    items: Vec<Item<T>>,
}

// Custom serialization: single item => just the item, multiple => array
impl<T: Serialize> Serialize for NestedItemList<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.items.as_slice() {
            [single] => single.serialize(serializer),
            multiple => multiple.serialize(serializer),
        }
    }
}

// Custom deserialization: accept either single value or array
impl<'de, T: Deserialize<'de>> Deserialize<'de> for NestedItemList<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::marker::PhantomData;

        struct NestedItemListVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for NestedItemListVisitor<T> {
            type Value = NestedItemList<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a single value or a list of values")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<Item<T>>()? {
                    items.push(item);
                }
                Ok(NestedItemList { items })
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Deserialize a single string as a Value wrapped in Item
                let item = Item::<T>::deserialize(de::value::StrDeserializer::new(v))?;
                Ok(NestedItemList { items: vec![item] })
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let item = Item::<T>::deserialize(de::value::StringDeserializer::new(v))?;
                Ok(NestedItemList { items: vec![item] })
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                // A map could be a conditional (if/then/else) or a value - deserialize as Item
                let item = Item::<T>::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(NestedItemList { items: vec![item] })
            }
        }

        deserializer.deserialize_any(NestedItemListVisitor(PhantomData))
    }
}

impl<T> NestedItemList<T> {
    /// Create from a list of items
    pub fn new(items: Vec<Item<T>>) -> Self {
        Self { items }
    }

    /// Create from a single item
    pub fn single(item: Item<T>) -> Self {
        Self { items: vec![item] }
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

    /// Convert to a vector
    pub fn into_vec(self) -> Vec<Item<T>> {
        self.items
    }

    /// Get as a slice
    pub fn as_slice(&self) -> &[Item<T>] {
        &self.items
    }

    /// Collect all used variables
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for item in &self.items {
            vars.extend(item.used_variables());
        }
        vars
    }
}

impl<T> IntoIterator for NestedItemList<T> {
    type Item = Item<T>;
    type IntoIter = std::vec::IntoIter<Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a NestedItemList<T> {
    type Item = &'a Item<T>;
    type IntoIter = std::slice::Iter<'a, Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// A conditional with if/then/else branches
/// Supports nested conditionals in then/else branches
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conditional<T> {
    /// The condition to evaluate
    #[serde(rename = "if")]
    pub condition: JinjaExpression,
    /// The values to use if condition is true (can include nested conditionals)
    pub then: NestedItemList<T>,
    /// The values to use if condition is false (can include nested conditionals)
    #[serde(rename = "else", skip_serializing_if = "Option::is_none")]
    pub else_value: Option<NestedItemList<T>>,
    /// Optional span for the condition expression (for error reporting)
    #[serde(skip)]
    pub condition_span: Option<Span>,
}

impl<T> Conditional<T> {
    /// Get all variables used in this conditional
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = self.condition.used_variables().to_vec();

        // Collect from then branch
        for value in self.then.iter() {
            vars.extend(value.used_variables());
        }

        // Collect from else branch if present
        if let Some(else_value) = &self.else_value {
            for value in else_value.iter() {
                vars.extend(value.used_variables());
            }
        }

        vars.sort();
        vars.dedup();
        vars
    }
}

/// An item in a conditional list - either a value or a conditional
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item<T> {
    /// A conditional if/then/else (must be first for untagged to work)
    Conditional(Conditional<T>),
    /// A concrete value or template
    Value(Value<T>),
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

    /// Get all variables used in this item
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Item::Value(v) => v.used_variables(),
            Item::Conditional(c) => c.used_variables(),
        }
    }
}

impl<T: fmt::Display> fmt::Display for Item<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Item::Value(v) => write!(f, "{}", v),
            Item::Conditional(_) => write!(f, "<conditional>"),
        }
    }
}

/// A list that may contain conditionals
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
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

    /// Get all variables used in all items
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for item in self.items.iter() {
            vars.extend(item.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
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

/// A list that may contain conditionals, but also accepts a single value during deserialization
/// Use this type when a field should accept both `field: value` and `field: [value1, value2]`
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConditionalListOrItem<T> {
    items: Vec<Item<T>>,
}

// Custom deserialization: accept either single value or array
impl<'de, T: Deserialize<'de>> Deserialize<'de> for ConditionalListOrItem<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::marker::PhantomData;

        struct ConditionalListOrItemVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ConditionalListOrItemVisitor<T> {
            type Value = ConditionalListOrItem<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a single value or a list of values")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<Item<T>>()? {
                    items.push(item);
                }
                Ok(ConditionalListOrItem { items })
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Deserialize a single string as a Value<T> wrapped in Item
                let value = Value::<T>::deserialize(de::value::StrDeserializer::new(v))?;
                Ok(ConditionalListOrItem {
                    items: vec![Item::Value(value)],
                })
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = Value::<T>::deserialize(de::value::StringDeserializer::new(v))?;
                Ok(ConditionalListOrItem {
                    items: vec![Item::Value(value)],
                })
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                // A map could be a conditional (if/then/else) - deserialize as Item
                let item = Item::<T>::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(ConditionalListOrItem { items: vec![item] })
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = Value::<T>::deserialize(de::value::BoolDeserializer::new(v))?;
                Ok(ConditionalListOrItem {
                    items: vec![Item::Value(value)],
                })
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = Value::<T>::deserialize(de::value::I64Deserializer::new(v))?;
                Ok(ConditionalListOrItem {
                    items: vec![Item::Value(value)],
                })
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = Value::<T>::deserialize(de::value::U64Deserializer::new(v))?;
                Ok(ConditionalListOrItem {
                    items: vec![Item::Value(value)],
                })
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = Value::<T>::deserialize(de::value::F64Deserializer::new(v))?;
                Ok(ConditionalListOrItem {
                    items: vec![Item::Value(value)],
                })
            }
        }

        deserializer.deserialize_any(ConditionalListOrItemVisitor(PhantomData))
    }
}

impl<T> ConditionalListOrItem<T> {
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

    /// Get all variables used in all items
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for item in self.items.iter() {
            vars.extend(item.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }

    /// Convert to a ConditionalList
    pub fn into_conditional_list(self) -> ConditionalList<T> {
        ConditionalList::new(self.items)
    }
}

impl<T> IntoIterator for ConditionalListOrItem<T> {
    type Item = Item<T>;
    type IntoIter = std::vec::IntoIter<Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a ConditionalListOrItem<T> {
    type Item = &'a Item<T>;
    type IntoIter = std::slice::Iter<'a, Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T> Default for ConditionalListOrItem<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T> From<ConditionalListOrItem<T>> for ConditionalList<T> {
    fn from(value: ConditionalListOrItem<T>) -> Self {
        ConditionalList::new(value.items)
    }
}
