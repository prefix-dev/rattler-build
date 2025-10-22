//! YAML parser with Jinja2 template support for rattler-build
//!
//! This crate provides shared parsing infrastructure for YAML configuration files
//! that support Jinja2 templates and conditional structures (if/then/else).
//!
//! # Core Types
//!
//! - [`Value<T>`] - A value that can be either concrete or a Jinja2 template
//! - [`ConditionalList<T>`] - A list that may contain conditional if/then/else items
//! - [`Item<T>`] - An item in a conditional list (either a value or a conditional)
//! - [`ListOrItem<T>`] - Either a single item or a list of items
//! - [`Conditional<T>`] - An if/then/else conditional structure
//!
//! # Parsing Functions
//!
//! - [`parse_value`] - Parse a single value (concrete or template)
//! - [`parse_conditional_list`] - Parse a list that may contain conditionals
//! - [`parse_list_or_item`] - Parse either a single value or a list
//!
//! # Example
//!
//! ```rust
//! use rattler_build_yaml_parser::{parse_conditional_list, ConditionalList, Item};
//!
//! let yaml = marked_yaml::parse_yaml(0, r#"
//! python:
//!   - "3.9"
//!   - "3.10"
//!   - if: win
//!     then: "3.8"
//! "#).unwrap();
//!
//! let node = yaml.as_mapping().unwrap().get("python").unwrap();
//! let list: ConditionalList<String> = parse_conditional_list(node).unwrap();
//! assert_eq!(list.len(), 3);
//! ```

pub mod conditional;
pub mod error;
pub mod helpers;
pub mod list;
pub mod types;
pub mod value;

// Re-export commonly used items
pub use conditional::parse_conditional_list;
pub use error::{FileParseError, ParseError, ParseResult};
pub use helpers::{contains_jinja_template, get_span, validate_mapping_fields};
pub use list::parse_list_or_item;
pub use types::{Conditional, ConditionalList, Item, ListOrItem, Value, ValueInner};
pub use value::{parse_value, parse_value_with_name};
