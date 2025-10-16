use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

// Re-export Jinja types from rattler_build_jinja
pub use rattler_build_jinja::{JinjaExpression, JinjaTemplate};

/// Trait for types that can report which template variables they use
#[allow(dead_code)]
pub trait UsedVariables {
    fn used_variables(&self) -> Vec<String>;
}

// Core enum for values that can be either concrete or templated
// Each variant now carries span information for error reporting
#[derive(Debug, Clone, PartialEq)]
pub enum Value<T> {
    Concrete {
        value: T,
        span: crate::span::Span,
    },
    Template {
        template: JinjaTemplate,
        span: crate::span::Span,
    },
}

impl<T> Value<T> {
    /// Create a new concrete value with span information
    pub fn new_concrete(value: T, span: crate::span::Span) -> Self {
        Value::Concrete { value, span }
    }

    /// Create a new template value with span information
    pub fn new_template(template: JinjaTemplate, span: crate::span::Span) -> Self {
        Value::Template { template, span }
    }

    /// Get the span information for this value
    pub fn span(&self) -> crate::span::Span {
        match self {
            Value::Concrete { span, .. } => *span,
            Value::Template { span, .. } => *span,
        }
    }

    /// Get the list of variables used in this value (cached for templates)
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Value::Concrete { .. } => Vec::new(),
            Value::Template { template, .. } => template.used_variables().to_vec(),
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
// Note: Deserialization creates values with unknown spans since we don't have source location info
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Value<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // First try to deserialize as a string
        let value = serde_json::Value::deserialize(deserializer)?;

        if let Some(s) = value.as_str() {
            // Check if it's a template
            if s.contains("${{") {
                let template = JinjaTemplate::new(s.to_string()).map_err(D::Error::custom)?;
                return Ok(Value::Template {
                    template,
                    span: crate::span::Span::unknown(),
                });
            }
        }

        // Otherwise, deserialize as T
        let concrete = T::deserialize(value).map_err(D::Error::custom)?;
        Ok(Value::Concrete {
            value: concrete,
            span: crate::span::Span::unknown(),
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

impl<T: ToString> Value<T> {
    pub fn concrete(&self) -> Option<&T> {
        if let Value::Concrete { value, .. } = self {
            Some(value)
        } else {
            None
        }
    }
}

impl<T: ToString + FromStr> FromStr for Value<T>
where
    T::Err: std::fmt::Display,
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("${{") {
            // If it contains some template syntax, validate and create template
            let template = JinjaTemplate::new(s.to_string())?;
            return Ok(Value::Template {
                template,
                span: crate::span::Span::unknown(),
            });
        }

        T::from_str(s)
            .map(|value| Value::Concrete {
                value,
                span: crate::span::Span::unknown(),
            })
            .map_err(|e| format!("Failed to parse concrete value: {}", e))
    }
}

// Any item in a list can be either a value or a conditional
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item<T> {
    Value(Value<T>),
    Conditional(Conditional<T>),
}

impl<T> Item<T> {
    pub fn new_from_conditional(
        condition: String,
        then: Vec<Value<T>>,
        else_value: Vec<Value<T>>,
    ) -> Result<Self, String> {
        let condition = JinjaExpression::new(condition)?;
        Ok(Item::Conditional(Conditional {
            condition,
            then: ListOrItem::new(then),
            else_value: Some(ListOrItem::new(else_value)),
        }))
    }
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

impl<T: ToString + FromStr> FromStr for Item<T>
where
    T::Err: std::fmt::Display,
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("${{") {
            // If it contains some template syntax, validate and create template
            let template = JinjaTemplate::new(s.to_string())?;
            return Ok(Item::Value(Value::Template {
                template,
                span: crate::span::Span::unknown(),
            }));
        }

        let value = T::from_str(s).map_err(|e| format!("Failed to parse: {}", e))?;
        Ok(Item::Value(Value::Concrete {
            value,
            span: crate::span::Span::unknown(),
        }))
    }
}

impl<T> Item<T> {
    /// Collect all variables used in this item
    /// Note: Requires T to be ToString for Conditional variant
    pub fn used_variables(&self) -> Vec<String>
    where
        T: ToString + Debug,
    {
        match self {
            Item::Value(v) => v.used_variables(),
            Item::Conditional(c) => c.used_variables(),
        }
    }
}
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

impl<T: FromStr> FromStr for ListOrItem<T> {
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ListOrItem::single(s.parse()?))
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

// Conditional structure for if-else logic
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
    // used variables in all items
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

/// Generic include/exclude pattern type - can be a simple list or include/exclude mapping
/// This is commonly used for glob patterns that support filtering
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum IncludeExclude<T = String> {
    /// Simple list of items
    List(ConditionalList<T>),
    /// Include/exclude mapping
    Mapping {
        /// Items to include
        #[serde(default)]
        include: ConditionalList<T>,
        /// Items to exclude
        #[serde(default)]
        exclude: ConditionalList<T>,
    },
}

impl<T> Default for IncludeExclude<T> {
    fn default() -> Self {
        Self::List(ConditionalList::default())
    }
}

impl<T: ToString + Debug> IncludeExclude<T> {
    /// Collect all variables used in this include/exclude pattern
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        match self {
            IncludeExclude::List(list) => {
                vars.extend(list.used_variables());
            }
            IncludeExclude::Mapping { include, exclude } => {
                vars.extend(include.used_variables());
                vars.extend(exclude.used_variables());
            }
        }
        vars.sort();
        vars.dedup();
        vars
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

impl<T: ToString> Value<T> {
    pub fn is_template(&self) -> bool {
        matches!(self, Value::Template { .. })
    }

    pub fn is_concrete(&self) -> bool {
        matches!(self, Value::Concrete { .. })
    }
}

/// Script content - either a simple command string or an inline script with options
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptContent {
    /// Simple command string or file path
    Command(String),
    /// Inline script with optional interpreter, env vars, and content/file
    Inline(Box<InlineScript>),
}

impl Display for ScriptContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptContent::Command(s) => write!(f, "{}", s),
            ScriptContent::Inline(inline) => {
                if inline.content.is_some() {
                    write!(f, "InlineScript(content: [...])")
                } else if let Some(file) = &inline.file {
                    write!(f, "InlineScript(file: {})", file)
                } else {
                    write!(f, "InlineScript(empty)")
                }
            }
        }
    }
}

impl FromStr for ScriptContent {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ScriptContent::Command(s.to_string()))
    }
}

impl Default for ScriptContent {
    fn default() -> Self {
        ScriptContent::Command(String::new())
    }
}

impl ScriptContent {
    /// Collect all variables used in this script content
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            ScriptContent::Command(_) => Vec::new(),
            ScriptContent::Inline(inline) => inline.used_variables(),
        }
    }
}

/// Inline script specification with content or file reference
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineScript {
    /// Optional interpreter (e.g., "bash", "python")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Value<String>>,

    /// Environment variables for the script
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub env: indexmap::IndexMap<String, Value<String>>,

    /// Secrets to expose to the script
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,

    /// Inline script content - can be a string or array of commands with conditionals
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ConditionalList<String>>,

    /// File path to script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<Value<String>>,
}

impl InlineScript {
    /// Collect all variables used in this inline script
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        if let Some(interpreter) = &self.interpreter {
            vars.extend(interpreter.used_variables());
        }

        for value in self.env.values() {
            vars.extend(value.used_variables());
        }

        if let Some(content) = &self.content {
            vars.extend(content.used_variables());
        }

        if let Some(file) = &self.file {
            vars.extend(file.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_concrete_no_variables() {
        let value: Value<String> =
            Value::new_concrete("hello".to_string(), crate::span::Span::unknown());
        assert_eq!(value.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_value_template_simple_variable() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ name }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        );
        let vars = value.used_variables();
        assert_eq!(vars, vec!["name"]);
    }

    #[test]
    fn test_value_template_multiple_variables() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["name", "version"]);
    }

    #[test]
    fn test_value_template_with_filter() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ name | lower }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        );
        let vars = value.used_variables();
        assert_eq!(vars, vec!["name"]);
    }

    #[test]
    fn test_value_template_with_complex_expression() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ name ~ '-' ~ version if linux else name }}".to_string())
                .unwrap(),
            crate::span::Span::unknown(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["linux", "name", "version"]);
    }

    #[test]
    fn test_conditional_simple() {
        let cond = Conditional::new(
            "linux".to_string(),
            ListOrItem::single(Value::new_concrete(
                "gcc".to_string(),
                crate::span::Span::unknown(),
            )),
        )
        .unwrap()
        .with_else(ListOrItem::single(Value::new_concrete(
            "clang".to_string(),
            crate::span::Span::unknown(),
        )));
        let vars = cond.used_variables();
        assert_eq!(vars, vec!["linux"]);
    }

    #[test]
    fn test_conditional_complex_expression() {
        let cond = Conditional::new(
            "target_platform == 'linux' and version >= '3.0'".to_string(),
            ListOrItem::single(Value::new_concrete(
                "gcc".to_string(),
                crate::span::Span::unknown(),
            )),
        )
        .unwrap();
        let mut vars = cond.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["target_platform", "version"]);
    }

    #[test]
    fn test_item_value_variant() {
        let item: Item<String> = Item::Value(Value::new_template(
            JinjaTemplate::new("${{ compiler }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        ));
        let vars = item.used_variables();
        assert_eq!(vars, vec!["compiler"]);
    }

    #[test]
    fn test_item_conditional_variant() {
        let cond = Conditional::new(
            "unix".to_string(),
            ListOrItem::single(Value::new_concrete(
                "bash".to_string(),
                crate::span::Span::unknown(),
            )),
        )
        .unwrap()
        .with_else(ListOrItem::single(Value::new_concrete(
            "cmd".to_string(),
            crate::span::Span::unknown(),
        )));
        let item: Item<String> = Item::Conditional(cond);
        let vars = item.used_variables();
        assert_eq!(vars, vec!["unix"]);
    }

    #[test]
    fn test_conditional_list_mixed_items() {
        let items = vec![
            Item::Value(Value::new_concrete(
                "static-dep".to_string(),
                crate::span::Span::unknown(),
            )),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap(),
                crate::span::Span::unknown(),
            )),
            Item::Conditional(
                Conditional::new(
                    "linux".to_string(),
                    ListOrItem::single(Value::new_concrete(
                        "linux-gcc".to_string(),
                        crate::span::Span::unknown(),
                    )),
                )
                .unwrap()
                .with_else(ListOrItem::single(Value::new_concrete(
                    "other-compiler".to_string(),
                    crate::span::Span::unknown(),
                ))),
            ),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ python }}".to_string()).unwrap(),
                crate::span::Span::unknown(),
            )),
        ];

        let list = ConditionalList::new(items);
        let mut vars = list.used_variables();
        vars.sort();
        // compiler('c') expands to c_compiler and c_compiler_version
        assert_eq!(
            vars,
            vec!["c_compiler", "c_compiler_version", "linux", "python"]
        );
    }

    #[test]
    fn test_conditional_list_deduplication() {
        let items = vec![
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ name }}".to_string()).unwrap(),
                crate::span::Span::unknown(),
            )),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap(),
                crate::span::Span::unknown(),
            )),
            Item::Conditional(
                Conditional::new(
                    "name == 'foo'".to_string(),
                    ListOrItem::single(Value::new_concrete(
                        "bar".to_string(),
                        crate::span::Span::unknown(),
                    )),
                )
                .unwrap(),
            ),
        ];

        let list = ConditionalList::new(items);
        let vars = list.used_variables();
        // Should deduplicate "name"
        assert_eq!(vars, vec!["name", "version"]);
    }

    #[test]
    fn test_conditional_list_empty() {
        let list: ConditionalList<String> = ConditionalList::new(vec![]);
        assert_eq!(list.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_value_from_str_template() {
        let value: Value<String> = "${{ version }}".parse().unwrap();
        assert!(value.is_template());
        assert_eq!(value.used_variables(), vec!["version"]);
    }

    #[test]
    fn test_value_from_str_concrete() {
        let value: Value<String> = "plain_string".parse().unwrap();
        assert!(value.is_concrete());
        assert_eq!(value.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_template_with_binary_operators() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ x + y * z }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["x", "y", "z"]);
    }

    #[test]
    fn test_template_with_comparison() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new(
                "${{ version >= min_version and version < max_version }}".to_string(),
            )
            .unwrap(),
            crate::span::Span::unknown(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["max_version", "min_version", "version"]);
    }

    #[test]
    fn test_template_with_attribute_access() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ build.number }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        );
        let vars = value.used_variables();
        assert_eq!(vars, vec!["build"]);
    }

    #[test]
    fn test_template_with_list() {
        let value: Value<String> = Value::new_template(
            JinjaTemplate::new("${{ [a, b, c] }}".to_string()).unwrap(),
            crate::span::Span::unknown(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["a", "b", "c"]);
    }
}
