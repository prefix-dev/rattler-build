//! Error types for recipe parsing
//!
//! This module re-exports error types from rattler_build_yaml_parser.

// Re-export all error types from the shared YAML parser
pub use rattler_build_yaml_parser::{ParseError, ParseResult};

// Re-export the enhanced error wrapper for better diagnostics
#[cfg(feature = "miette")]
pub use rattler_build_yaml_parser::ParseErrorWithSource;
