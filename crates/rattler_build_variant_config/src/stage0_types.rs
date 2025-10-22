//! Stage0 types for variant configuration - templates and conditionals before evaluation
//!
//! This module defines types that can contain Jinja templates and conditionals,
//! similar to the stage0 types in rattler_build_recipe.

use rattler_build_jinja::{JinjaExpression, JinjaTemplate};
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::fmt::{Debug, Display};

/// Core enum for values that can be either concrete or templated
#[derive(Debug, Clone, PartialEq)]
pub enum Value<T> {
    Concrete {
        value: T,
        span: Option<marked_yaml::Span>,
    },
    Template {
        template: JinjaTemplate,
        span: Option<marked_yaml::Span>,
    },
}

impl<T> Value<T> {
    /// Create a new concrete value with span information
    pub fn new_concrete(value: T, span: Option<marked_yaml::Span>) -> Self {
        Value::Concrete { value, span }
    }

    /// Create a new template value with span information
    pub fn new_template(template: JinjaTemplate, span: Option<marked_yaml::Span>) -> Self {
        Value::Template { template, span }
    }

    /// Get the span information for this value
    pub fn span(&self) -> Option<marked_yaml::Span> {
        match self {
            Value::Concrete { span, .. } => *span,
            Value::Template { span, .. } => *span,
        }
    }

    /// Get the list of variables used in this value
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Value::Concrete { .. } => Vec::new(),
            Value::Template { template, .. } => template.used_variables().to_vec(),
        }
    }

    /// Check if this is a template
    pub fn is_template(&self) -> bool {
        matches!(self, Value::Template { .. })
    }

    /// Check if this is concrete
    pub fn is_concrete(&self) -> bool {
        matches!(self, Value::Concrete { .. })
    }

    /// Get the concrete value if it is concrete
    pub fn concrete(&self) -> Option<&T> {
        if let Value::Concrete { value, .. } = self {
            Some(value)
        } else {
            None
        }
    }
}

// Custom serialization for Value (span is not serialized)
impl<T: Serialize> Serialize for Value<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Value::Concrete { value, .. } => value.serialize(serializer),
            Value::Template { template, .. } => template.serialize(serializer),
        }
    }
}

// Custom deserialization for Value
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Value<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = serde_yaml::Value::deserialize(deserializer)?;

        if let Some(s) = value.as_str() {
            // Check if it's a template
            if s.contains("${{") {
                let template = JinjaTemplate::new(s.to_string()).map_err(D::Error::custom)?;
                return Ok(Value::Template {
                    template,
                    span: None,
                });
            }
        }

        // Otherwise, deserialize as T
        let concrete = T::deserialize(value).map_err(D::Error::custom)?;
        Ok(Value::Concrete {
            value: concrete,
            span: None,
        })
    }
}

impl<T: Display> Display for Value<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Concrete { value, .. } => write!(f, "{value}"),
            Value::Template { template, .. } => write!(f, "{template}"),
        }
    }
}

/// Any item in a list can be either a value or a conditional
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item<T> {
    Value(Value<T>),
    Conditional(Conditional<T>),
}

impl<T: Display> Display for Item<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Item::Value(value) => write!(f, "{value}"),
            Item::Conditional(cond) => write!(f, "{cond}"),
        }
    }
}

impl<T: PartialEq> PartialEq for Item<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Item::Value(Value::Concrete { value: a, .. }),
                Item::Value(Value::Concrete { value: b, .. }),
            ) => a == b,
            (Item::Conditional(a), Item::Conditional(b)) => {
                a.condition == b.condition && a.then == b.then && a.else_value == b.else_value
            }
            _ => false,
        }
    }
}

impl<T: Debug> Debug for Item<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Item::Value(value) => write!(f, "Value({value:?})"),
            Item::Conditional(cond) => write!(f, "Conditional({cond:?})"),
        }
    }
}

impl<T> From<Conditional<T>> for Item<T> {
    fn from(value: Conditional<T>) -> Self {
        Self::Conditional(value)
    }
}

/// A list or single item wrapper
#[derive(Clone)]
pub struct ListOrItem<T>(pub Vec<T>);

impl<T> Default for ListOrItem<T> {
    fn default() -> Self {
        ListOrItem(Vec::new())
    }
}

impl<T: PartialEq> PartialEq for ListOrItem<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Debug> Debug for ListOrItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            write!(f, "ListOrItem([])")
        } else if self.0.len() == 1 {
            write!(f, "ListOrItem({:?})", self.0[0])
        } else {
            write!(f, "ListOrItem({:?})", self.0)
        }
    }
}

impl<T> serde::Serialize for ListOrItem<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0.len() {
            1 => self.0[0].serialize(serializer),
            _ => self.0.serialize(serializer),
        }
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for ListOrItem<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use std::fmt;

        use serde::de::{Error, Visitor};

        struct ListOrItemVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: serde::Deserialize<'de>> Visitor<'de> for ListOrItemVisitor<T> {
            type Value = ListOrItem<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a single item or a list of items")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(item) = seq.next_element()? {
                    vec.push(item);
                }
                Ok(ListOrItem(vec))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let item = T::deserialize(serde::de::value::StrDeserializer::new(value))?;
                Ok(ListOrItem(vec![item]))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let item = T::deserialize(serde::de::value::StringDeserializer::new(value))?;
                Ok(ListOrItem(vec![item]))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let item = T::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(ListOrItem(vec![item]))
            }
        }

        deserializer.deserialize_any(ListOrItemVisitor(std::marker::PhantomData))
    }
}

impl<T: ToString> Display for ListOrItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.len() {
            0 => write!(f, "[]"),
            1 => write!(f, "{}", self.0[0].to_string()),
            _ => write!(
                f,
                "[{}]",
                self.0
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl<T> ListOrItem<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self(items)
    }

    pub fn single(item: T) -> Self {
        Self(vec![item])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<T> {
        self.0.iter()
    }
}

/// Conditional structure for if-else logic
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conditional<T> {
    /// The condition to be evaluated
    #[serde(rename = "if")]
    pub condition: JinjaExpression,

    /// The then branch
    pub then: ListOrItem<Value<T>>,

    /// The optional else branch
    #[serde(skip_serializing_if = "Option::is_none", rename = "else")]
    pub else_value: Option<ListOrItem<Value<T>>>,
}

impl<T: Display> Display for Conditional<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "if {} then {}", self.condition.source(), self.then,)?;
        if let Some(else_value) = &self.else_value {
            write!(f, " else {}", else_value)?;
        }
        Ok(())
    }
}

impl<T: ToString + Debug> Conditional<T> {
    pub fn new(condition: String, then_value: ListOrItem<Value<T>>) -> Result<Self, String> {
        let condition = JinjaExpression::new(condition)?;
        Ok(Self {
            condition,
            then: then_value,
            else_value: None,
        })
    }

    pub fn with_else(mut self, else_value: ListOrItem<Value<T>>) -> Self {
        self.else_value = Some(else_value);
        self
    }

    /// Collect all variables used in this conditional
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = self.condition.used_variables().to_vec();

        // Collect variables from then values
        for value in self.then.iter() {
            vars.extend(value.used_variables());
        }

        // Collect variables from else values
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

/// Newtype for lists that can contain conditionals
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConditionalList<T>(Vec<Item<T>>);

impl<T> Default for ConditionalList<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> ConditionalList<T> {
    pub fn new(items: Vec<Item<T>>) -> Self {
        Self(items)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<Item<T>> {
        self.0.iter()
    }

    pub fn into_inner(self) -> Vec<Item<T>> {
        self.0
    }
}

impl<'a, T> IntoIterator for &'a ConditionalList<T> {
    type Item = &'a Item<T>;
    type IntoIter = std::slice::Iter<'a, Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: ToString + Debug> ConditionalList<T> {
    /// Get all variables used in all items
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for item in self.iter() {
            match item {
                Item::Value(v) => vars.extend(v.used_variables()),
                Item::Conditional(c) => {
                    vars.extend(c.used_variables());
                }
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
