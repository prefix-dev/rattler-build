//! Stage 1: Evaluated recipe with all templates and conditionals resolved
//!
//! This module contains the "evaluated" or "rendered" recipe structure where:
//! - All Jinja templates have been evaluated to concrete values
//! - All conditionals have been flattened based on the evaluation context
//! - All types are concrete (String, Vec<String>, etc.) with no wrappers
//!
//! The transformation from stage0 to stage1 happens through the `Evaluate` trait.

use std::collections::HashMap;

use crate::{ParseError, stage0::jinja_functions::JinjaConfig};

pub mod about;
pub mod all_or_glob_vec;
pub mod build;
pub mod extra;
pub mod glob_vec;
pub mod package;
pub mod recipe;
pub mod requirements;
pub mod source;
pub mod tests;

pub use about::About;
pub use all_or_glob_vec::AllOrGlobVec;
pub use build::Build;
pub use extra::Extra;
pub use glob_vec::GlobVec;
pub use package::Package;
pub use recipe::Recipe;
pub use requirements::Requirements;
pub use source::Source;
pub use tests::TestType;

/// Evaluation context containing variables for template rendering and conditional evaluation
#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    /// Variables available during evaluation (e.g., "name", "version", "py", "target_platform")
    variables: HashMap<String, String>,
    /// Configuration for Jinja functions (compiler, cdt, etc.)
    jinja_config: JinjaConfig,
}

impl EvaluationContext {
    /// Create a new empty evaluation context
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an evaluation context from a map of variables
    pub fn from_map(variables: HashMap<String, String>) -> Self {
        Self {
            variables,
            jinja_config: JinjaConfig::default(),
        }
    }

    /// Create an evaluation context with variables and Jinja config
    pub fn with_config(variables: HashMap<String, String>, jinja_config: JinjaConfig) -> Self {
        Self {
            variables,
            jinja_config,
        }
    }

    /// Insert a variable into the context
    pub fn insert(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }

    /// Get a variable from the context
    pub fn get(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }

    /// Check if a variable exists in the context
    pub fn contains(&self, key: &str) -> bool {
        self.variables.contains_key(key)
    }

    /// Get all variables as a reference
    pub fn variables(&self) -> &HashMap<String, String> {
        &self.variables
    }

    /// Get the Jinja configuration
    pub fn jinja_config(&self) -> &JinjaConfig {
        &self.jinja_config
    }

    /// Set the Jinja configuration
    pub fn set_jinja_config(&mut self, config: JinjaConfig) {
        self.jinja_config = config;
    }
}

/// Trait for evaluating stage0 types into stage1 types
///
/// This trait is implemented by stage0 types to convert themselves into
/// their stage1 equivalents by:
/// - Rendering Jinja templates
/// - Flattening conditionals based on the evaluation context
/// - Validating the results
pub trait Evaluate {
    /// The stage1 type that this stage0 type evaluates to
    type Output;

    /// Evaluate this stage0 type into its stage1 equivalent
    ///
    /// # Arguments
    /// * `context` - The evaluation context containing variables
    ///
    /// # Returns
    /// The evaluated stage1 type, or an error if evaluation fails
    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError>;
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_evaluation_context_creation() {
        let ctx = EvaluationContext::new();
        assert!(ctx.variables().is_empty());
    }

    #[test]
    fn test_evaluation_context_insert_get() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), "foo".to_string());
        ctx.insert("version".to_string(), "1.0.0".to_string());

        assert_eq!(ctx.get("name"), Some(&"foo".to_string()));
        assert_eq!(ctx.get("version"), Some(&"1.0.0".to_string()));
        assert_eq!(ctx.get("unknown"), None);
    }

    #[test]
    fn test_evaluation_context_from_map() {
        let mut map = HashMap::new();
        map.insert("py".to_string(), "3.11".to_string());
        map.insert("target_platform".to_string(), "linux-64".to_string());

        let ctx = EvaluationContext::from_map(map);
        assert_eq!(ctx.get("py"), Some(&"3.11".to_string()));
        assert_eq!(ctx.get("target_platform"), Some(&"linux-64".to_string()));
    }

    #[test]
    fn test_evaluation_context_contains() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), "test".to_string());

        assert!(ctx.contains("name"));
        assert!(!ctx.contains("version"));
    }
}
