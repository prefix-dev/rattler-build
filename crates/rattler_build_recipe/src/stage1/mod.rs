//! Stage 1: Evaluated recipe with all templates and conditionals resolved
//!
//! This module contains the "evaluated" or "rendered" recipe structure where:
//! - All Jinja templates have been evaluated to concrete values
//! - All conditionals have been flattened based on the evaluation context
//! - All types are concrete (String, Vec<String>, etc.) with no wrappers
//!
//! The transformation from stage0 to stage1 happens through the `Evaluate` trait.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

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
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    /// Variables available during evaluation (e.g., "name", "version", "py", "target_platform")
    variables: HashMap<String, String>,
    /// Configuration for Jinja functions (compiler, cdt, etc.)
    jinja_config: JinjaConfig,
    /// Set of variables that were actually accessed during evaluation (tracked via thread-safe interior mutability)
    accessed_variables: Arc<Mutex<HashSet<String>>>,
    /// Set of variables that were accessed but undefined (tracked via thread-safe interior mutability)
    undefined_variables: Arc<Mutex<HashSet<String>>>,
}

impl Default for EvaluationContext {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
            jinja_config: JinjaConfig::default(),
            accessed_variables: Arc::new(Mutex::new(HashSet::new())),
            undefined_variables: Arc::new(Mutex::new(HashSet::new())),
        }
    }
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
            accessed_variables: Arc::new(Mutex::new(HashSet::new())),
            undefined_variables: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Create an evaluation context with variables and Jinja config
    pub fn with_config(variables: HashMap<String, String>, jinja_config: JinjaConfig) -> Self {
        Self {
            variables,
            jinja_config,
            accessed_variables: Arc::new(Mutex::new(HashSet::new())),
            undefined_variables: Arc::new(Mutex::new(HashSet::new())),
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

    /// Track that a variable was accessed
    pub(crate) fn track_access(&self, key: &str) {
        if let Ok(mut accessed) = self.accessed_variables.lock() {
            accessed.insert(key.to_string());
        }
    }

    /// Track that a variable was accessed but undefined
    pub(crate) fn track_undefined(&self, key: &str) {
        if let Ok(mut undefined) = self.undefined_variables.lock() {
            undefined.insert(key.to_string());
        }
    }

    /// Get the set of variables that were accessed during evaluation
    pub fn accessed_variables(&self) -> HashSet<String> {
        self.accessed_variables
            .lock()
            .map(|accessed| accessed.clone())
            .unwrap_or_default()
    }

    /// Get the set of variables that were accessed but undefined
    pub fn undefined_variables(&self) -> HashSet<String> {
        self.undefined_variables
            .lock()
            .map(|undefined| undefined.clone())
            .unwrap_or_default()
    }

    /// Clear the accessed variables tracker
    pub fn clear_accessed(&self) {
        if let Ok(mut accessed) = self.accessed_variables.lock() {
            accessed.clear();
        }
        if let Ok(mut undefined) = self.undefined_variables.lock() {
            undefined.clear();
        }
    }

    /// Evaluate and merge context variables into the evaluation context
    ///
    /// Context variables are evaluated in order, allowing later variables to reference earlier ones.
    /// The context can contain templates that reference:
    /// - Previously defined context variables
    /// - Variables from the original context
    ///
    /// # Arguments
    /// * `context_vars` - The context variables to evaluate (from the recipe's context section)
    ///
    /// # Returns
    /// A new EvaluationContext with the evaluated context variables merged in
    pub fn with_context(
        &self,
        context_vars: &indexmap::IndexMap<String, crate::stage0::Value<String>>,
    ) -> Result<Self, ParseError> {
        use crate::stage0::evaluate::evaluate_string_value;

        // Clone the current context
        let mut new_context = self.clone();

        // Evaluate each context variable in order
        for (key, value) in context_vars {
            // Evaluate the value using the current context (which includes previously evaluated context vars)
            let evaluated = evaluate_string_value(value, &new_context)?;

            // Add the evaluated value to the context
            new_context.variables.insert(key.clone(), evaluated);
        }

        Ok(new_context)
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

    #[test]
    fn test_context_evaluation_simple() {
        use crate::stage0::Value;

        let ctx = EvaluationContext::new();

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert("name".to_string(), Value::Concrete("mypackage".to_string()));
        context_vars.insert("version".to_string(), Value::Concrete("1.0.0".to_string()));

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(evaluated_ctx.get("name"), Some(&"mypackage".to_string()));
        assert_eq!(evaluated_ctx.get("version"), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_context_evaluation_with_templates() {
        use crate::stage0::{JinjaTemplate, Value};

        let mut ctx = EvaluationContext::new();
        ctx.insert("base".to_string(), "myorg".to_string());

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert("name".to_string(), Value::Concrete("mypackage".to_string()));
        context_vars.insert(
            "full_name".to_string(),
            Value::Template(JinjaTemplate::new("${{ base }}/${{ name }}".to_string()).unwrap()),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(evaluated_ctx.get("name"), Some(&"mypackage".to_string()));
        assert_eq!(
            evaluated_ctx.get("full_name"),
            Some(&"myorg/mypackage".to_string())
        );
    }

    #[test]
    fn test_context_evaluation_forward_references() {
        use crate::stage0::{JinjaTemplate, Value};

        let ctx = EvaluationContext::new();

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert("name".to_string(), Value::Concrete("mypackage".to_string()));
        context_vars.insert("version".to_string(), Value::Concrete("1.0.0".to_string()));
        context_vars.insert(
            "package_version".to_string(),
            Value::Template(JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap()),
        );
        context_vars.insert(
            "full_id".to_string(),
            Value::Template(JinjaTemplate::new("pkg:${{ package_version }}".to_string()).unwrap()),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(
            evaluated_ctx.get("package_version"),
            Some(&"mypackage-1.0.0".to_string())
        );
        assert_eq!(
            evaluated_ctx.get("full_id"),
            Some(&"pkg:mypackage-1.0.0".to_string())
        );
    }

    #[test]
    fn test_context_evaluation_order_matters() {
        use crate::stage0::{JinjaTemplate, Value};

        let ctx = EvaluationContext::new();

        // The order matters - package_version references name and version
        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert("name".to_string(), Value::Concrete("mypackage".to_string()));
        context_vars.insert("version".to_string(), Value::Concrete("2.0.0".to_string()));
        context_vars.insert(
            "package_version".to_string(),
            Value::Template(JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap()),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(
            evaluated_ctx.get("package_version"),
            Some(&"mypackage-2.0.0".to_string())
        );
    }

    #[test]
    fn test_context_evaluation_with_existing_context() {
        use crate::stage0::{JinjaTemplate, Value};

        let mut ctx = EvaluationContext::new();
        ctx.insert("platform".to_string(), "linux".to_string());

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert("name".to_string(), Value::Concrete("mypackage".to_string()));
        context_vars.insert(
            "full_name".to_string(),
            Value::Template(JinjaTemplate::new("${{ name }}-${{ platform }}".to_string()).unwrap()),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        // Both the original context and the evaluated context should be present
        assert_eq!(evaluated_ctx.get("platform"), Some(&"linux".to_string()));
        assert_eq!(evaluated_ctx.get("name"), Some(&"mypackage".to_string()));
        assert_eq!(
            evaluated_ctx.get("full_name"),
            Some(&"mypackage-linux".to_string())
        );
    }
}
