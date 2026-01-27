//! Extension trait for marked_yaml::Node to add convenient parsing methods
//!
//! This module provides the `ParseNode` trait which adds ergonomic parsing methods
//! directly to YAML nodes, making the parsing code much more concise.
//!
//! # Example
//!
//! ```ignore
//! use rattler_build_yaml_parser::ParseNode;
//!
//! // Parse a single value
//! let name: String = node.parse_value("package.name")?;
//!
//! // Parse a list
//! let items: Vec<String> = node.parse_sequence("items")?;
//!
//! // Parse with custom converter
//! let value: Variable = node.parse_with_converter("key", &VariableConverter::new())?;
//! ```

use marked_yaml::Node as MarkedNode;

use crate::{
    conditional::parse_conditional_list_with_converter,
    converter::{FromStrConverter, NodeConverter},
    error::ParseResult,
    list::parse_list_or_item_with_converter,
    types::{ConditionalList, ListOrItem, Value},
    value::parse_value_with_converter,
};

/// Extension trait that adds convenient parsing methods to `marked_yaml::Node`
///
/// This trait provides a fluent, ergonomic API for parsing YAML nodes into Rust types.
/// It automatically uses the appropriate converter based on the target type's trait bounds.
pub trait ParseNode {
    /// Parse this node as a single value using the default FromStr converter
    ///
    /// # Example
    /// ```ignore
    /// let count: i32 = node.parse_value("count")?;
    /// let name: String = node.parse_value("name")?;
    /// ```
    fn parse_value<T>(&self, field_name: &str) -> ParseResult<Value<T>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display;

    /// Parse this node as a single value using a custom converter
    ///
    /// # Example
    /// ```ignore
    /// let var: Variable = node.parse_with_converter("key", &VariableConverter::new())?;
    /// ```
    fn parse_with_converter<T, C>(&self, field_name: &str, converter: &C) -> ParseResult<Value<T>>
    where
        C: NodeConverter<T>;

    /// Parse this node as a list or single item using the default FromStr converter
    ///
    /// # Example
    /// ```ignore
    /// let items: ListOrItem<Value<String>> = node.parse_list_or_item("items")?;
    /// ```
    fn parse_list_or_item<T>(&self, field_name: &str) -> ParseResult<ListOrItem<Value<T>>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display;

    /// Parse this node as a list or single item using a custom converter
    fn parse_list_or_item_with<T, C>(
        &self,
        field_name: &str,
        converter: &C,
    ) -> ParseResult<ListOrItem<Value<T>>>
    where
        C: NodeConverter<T>;

    /// Parse this node as a conditional list (supports if/then/else) using the default converter
    ///
    /// # Example
    /// ```ignore
    /// let items: ConditionalList<String> = node.parse_conditional_list()?;
    /// ```
    fn parse_conditional_list<T>(&self) -> ParseResult<ConditionalList<T>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display;

    /// Parse this node as a conditional list using a custom converter
    fn parse_conditional_list_with<T, C>(&self, converter: &C) -> ParseResult<ConditionalList<T>>
    where
        C: NodeConverter<T>;

    /// Parse this node as a sequence of scalars into a Vec
    ///
    /// This is a convenience method for the common pattern of parsing a list of simple values.
    ///
    /// # Example
    /// ```ignore
    /// let secrets: Vec<String> = node.parse_sequence("secrets")?;
    /// let counts: Vec<i32> = node.parse_sequence("counts")?;
    /// ```
    fn parse_sequence<T>(&self, field_name: &str) -> ParseResult<Vec<T>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display;

    /// Parse this node as a sequence using a custom converter
    ///
    /// # Example
    /// ```ignore
    /// let vars: Vec<Variable> = node.parse_sequence_with("vars", &VariableConverter::new())?;
    /// ```
    fn parse_sequence_with<T, C>(&self, field_name: &str, converter: &C) -> ParseResult<Vec<T>>
    where
        C: NodeConverter<T>;
}

impl ParseNode for MarkedNode {
    fn parse_value<T>(&self, field_name: &str) -> ParseResult<Value<T>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        parse_value_with_converter(self, field_name, &FromStrConverter::new())
    }

    fn parse_with_converter<T, C>(&self, field_name: &str, converter: &C) -> ParseResult<Value<T>>
    where
        C: NodeConverter<T>,
    {
        parse_value_with_converter(self, field_name, converter)
    }

    fn parse_list_or_item<T>(&self, _field_name: &str) -> ParseResult<ListOrItem<Value<T>>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display,
    {
        parse_list_or_item_with_converter(self, &FromStrConverter::new())
    }

    fn parse_list_or_item_with<T, C>(
        &self,
        _field_name: &str,
        converter: &C,
    ) -> ParseResult<ListOrItem<Value<T>>>
    where
        C: NodeConverter<T>,
    {
        parse_list_or_item_with_converter(self, converter)
    }

    fn parse_conditional_list<T>(&self) -> ParseResult<ConditionalList<T>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display,
    {
        parse_conditional_list_with_converter(self, &FromStrConverter::new())
    }

    fn parse_conditional_list_with<T, C>(&self, converter: &C) -> ParseResult<ConditionalList<T>>
    where
        C: NodeConverter<T>,
    {
        parse_conditional_list_with_converter(self, converter)
    }

    fn parse_sequence<T>(&self, field_name: &str) -> ParseResult<Vec<T>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        self.parse_sequence_with(field_name, &FromStrConverter::new())
    }

    fn parse_sequence_with<T, C>(&self, field_name: &str, converter: &C) -> ParseResult<Vec<T>>
    where
        C: NodeConverter<T>,
    {
        use crate::error::ParseError;
        use crate::helpers::get_span;

        let sequence = self
            .as_sequence()
            .ok_or_else(|| ParseError::expected_type("sequence", "non-sequence", get_span(self)))?;

        sequence
            .iter()
            .map(|item| converter.convert_scalar(item, field_name))
            .collect()
    }
}

/// Extension trait for working with YAML mappings (objects)
///
/// Provides convenient methods for parsing mapping fields and validating field names.
pub trait ParseMapping {
    /// Try to get and parse an optional field from the mapping
    ///
    /// Returns `Ok(None)` if the field doesn't exist, or `Ok(Some(value))` if it does.
    ///
    /// # Example
    /// ```ignore
    /// let homepage: Option<Value<String>> = mapping.try_get_field("homepage")?;
    /// ```
    fn try_get_field<T>(&self, field_name: &str) -> ParseResult<Option<Value<T>>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display;

    /// Try to get and parse an optional field using a custom converter
    fn try_get_field_with<T, C>(
        &self,
        field_name: &str,
        converter: &C,
    ) -> ParseResult<Option<Value<T>>>
    where
        C: NodeConverter<T>;

    /// Try to get and parse an optional field that can be a list or single item
    fn try_get_list_or_item<T>(
        &self,
        field_name: &str,
    ) -> ParseResult<Option<ListOrItem<Value<T>>>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display;

    /// Try to get and parse an optional conditional list field
    fn try_get_conditional_list<T>(
        &self,
        field_name: &str,
    ) -> ParseResult<Option<ConditionalList<T>>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display;

    /// Try to get and parse an optional conditional list field with custom converter
    fn try_get_conditional_list_with<T, C>(
        &self,
        field_name: &str,
        converter: &C,
    ) -> ParseResult<Option<ConditionalList<T>>>
    where
        C: NodeConverter<T>;

    /// Validate that all keys in the mapping are in the allowed list
    ///
    /// Returns an error with helpful suggestions if unknown fields are found.
    ///
    /// # Example
    /// ```ignore
    /// mapping.validate_keys("about", &["homepage", "license", "summary"])?;
    /// ```
    fn validate_keys(&self, section_name: &str, allowed: &[&str]) -> ParseResult<()>;
}

impl ParseMapping for MarkedNode {
    fn try_get_field<T>(&self, field_name: &str) -> ParseResult<Option<Value<T>>>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        self.try_get_field_with(field_name, &FromStrConverter::new())
    }

    fn try_get_field_with<T, C>(
        &self,
        field_name: &str,
        converter: &C,
    ) -> ParseResult<Option<Value<T>>>
    where
        C: NodeConverter<T>,
    {
        let mapping = self.as_mapping().ok_or_else(|| {
            use crate::error::ParseError;
            use crate::helpers::get_span;
            ParseError::expected_type("mapping", "non-mapping", get_span(self))
        })?;

        if let Some(node) = mapping.get(field_name) {
            Ok(Some(parse_value_with_converter(
                node, field_name, converter,
            )?))
        } else {
            Ok(None)
        }
    }

    fn try_get_list_or_item<T>(&self, field_name: &str) -> ParseResult<Option<ListOrItem<Value<T>>>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display,
    {
        let mapping = self.as_mapping().ok_or_else(|| {
            use crate::error::ParseError;
            use crate::helpers::get_span;
            ParseError::expected_type("mapping", "non-mapping", get_span(self))
        })?;

        if let Some(node) = mapping.get(field_name) {
            Ok(Some(node.parse_list_or_item(field_name)?))
        } else {
            Ok(None)
        }
    }

    fn try_get_conditional_list<T>(
        &self,
        field_name: &str,
    ) -> ParseResult<Option<ConditionalList<T>>>
    where
        T: std::str::FromStr + ToString,
        T::Err: std::fmt::Display,
    {
        self.try_get_conditional_list_with(field_name, &FromStrConverter::new())
    }

    fn try_get_conditional_list_with<T, C>(
        &self,
        field_name: &str,
        converter: &C,
    ) -> ParseResult<Option<ConditionalList<T>>>
    where
        C: NodeConverter<T>,
    {
        let mapping = self.as_mapping().ok_or_else(|| {
            use crate::error::ParseError;
            use crate::helpers::get_span;
            ParseError::expected_type("mapping", "non-mapping", get_span(self))
        })?;

        if let Some(node) = mapping.get(field_name) {
            // Handle null/empty values - in YAML, `field:` with no value or `field: null`
            // becomes an empty scalar. We treat this as "not present" rather than an error.
            if let Some(scalar) = node.as_scalar() {
                let s = scalar.as_str();
                if s.is_empty() || s == "null" || s == "~" {
                    return Ok(None);
                }
            }
            Ok(Some(node.parse_conditional_list_with(converter)?))
        } else {
            Ok(None)
        }
    }

    fn validate_keys(&self, section_name: &str, allowed: &[&str]) -> ParseResult<()> {
        use crate::error::ParseError;

        let mapping = self.as_mapping().ok_or_else(|| {
            use crate::helpers::get_span;
            ParseError::expected_type("mapping", "non-mapping", get_span(self))
        })?;

        for (key, _) in mapping.iter() {
            let key_str = key.as_str();
            if !allowed.contains(&key_str) {
                return Err(ParseError::invalid_value(
                    section_name,
                    format!("unknown field '{}'", key_str),
                    *key.span(),
                )
                .with_suggestion(format!("valid fields are: {}", allowed.join(", "))));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_value() {
        let yaml = marked_yaml::parse_yaml(0, "val: 42").unwrap();
        let node = yaml.as_mapping().unwrap().get("val").unwrap();

        let value: Value<i32> = node.parse_value("val").unwrap();
        assert!(value.is_concrete());
        assert_eq!(value.as_concrete(), Some(&42));
    }

    #[test]
    fn test_try_get_conditional_list_null_value() {
        // Test that `run:` with no value is treated as None
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
requirements:
  host:
    - pkg
  run:
"#,
        )
        .unwrap();
        let req = yaml.as_mapping().unwrap().get("requirements").unwrap();

        // run: with nothing after it should be treated as None
        let run: Option<ConditionalList<String>> = req.try_get_conditional_list("run").unwrap();
        assert!(run.is_none(), "Empty run: should be treated as None");

        // host: with values should work normally
        let host: Option<ConditionalList<String>> = req.try_get_conditional_list("host").unwrap();
        assert!(host.is_some());
        assert_eq!(host.unwrap().len(), 1);
    }

    #[test]
    fn test_try_get_conditional_list_explicit_null() {
        // Test that `run: null` is treated as None
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
requirements:
  run: null
"#,
        )
        .unwrap();
        let req = yaml.as_mapping().unwrap().get("requirements").unwrap();

        let run: Option<ConditionalList<String>> = req.try_get_conditional_list("run").unwrap();
        assert!(run.is_none(), "run: null should be treated as None");
    }

    #[test]
    fn test_try_get_conditional_list_tilde_null() {
        // Test that `run: ~` (YAML null syntax) is treated as None
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
requirements:
  run: ~
"#,
        )
        .unwrap();
        let req = yaml.as_mapping().unwrap().get("requirements").unwrap();

        let run: Option<ConditionalList<String>> = req.try_get_conditional_list("run").unwrap();
        assert!(run.is_none(), "run: ~ should be treated as None");
    }

    #[test]
    fn test_parse_sequence() {
        let yaml = marked_yaml::parse_yaml(0, "vals: [1, 2, 3]").unwrap();
        let node = yaml.as_mapping().unwrap().get("vals").unwrap();

        let values: Vec<i32> = node.parse_sequence("vals").unwrap();
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_string_sequence() {
        let yaml = marked_yaml::parse_yaml(0, r#"items: ["foo", "bar", "baz"]"#).unwrap();
        let node = yaml.as_mapping().unwrap().get("items").unwrap();

        let items: Vec<String> = node.parse_sequence("items").unwrap();
        assert_eq!(items, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_parse_conditional_list() {
        let yaml = marked_yaml::parse_yaml(
            0,
            r#"
items:
  - "plain"
  - if: unix
    then: "unix-only"
"#,
        )
        .unwrap();
        let node = yaml.as_mapping().unwrap().get("items").unwrap();

        let list: ConditionalList<String> = node.parse_conditional_list().unwrap();
        assert_eq!(list.len(), 2);
    }
}
