//! Error types for recipe parsing
//!
//! This module re-exports error types from rattler_build_yaml_parser.

// Re-export all error types from the shared YAML parser
pub use rattler_build_yaml_parser::{ParseError, ParseErrorWithSource, ParseResult};
