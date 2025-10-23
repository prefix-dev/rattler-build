//! Stage 1: Evaluated recipe with all templates and conditionals resolved
//!
//! This module contains the "evaluated" or "rendered" recipe structure where:
//! - All Jinja templates have been evaluated to concrete values
//! - All conditionals have been flattened based on the evaluation context
//! - All types are concrete (String, Vec<String>, etc.) with no wrappers
//!
//! The transformation from stage0 to stage1 happens through the `Evaluate` trait.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use rattler_build_jinja::{JinjaConfig, Variable};

pub mod about;
pub mod build;
pub mod extra;
pub mod hash;
pub mod package;
pub mod recipe;
pub mod requirements;
pub mod source;
pub mod tests;

#[cfg(test)]
mod variant_tests;

pub use about::About;
pub use build::{Build, Rpaths};
pub use extra::Extra;
pub use hash::{HashInfo, HashInput, compute_hash};
use indexmap::IndexMap;
pub use package::Package;
use rattler_build_yaml_parser::ParseError;
pub use recipe::Recipe;
pub use requirements::{Dependency, PinCompatible, PinSubpackage, Requirements};
pub use source::Source;
pub use tests::TestType;

// Re-export glob types from rattler_build_types
pub use rattler_build_types::{AllOrGlobVec, GlobCheckerVec, GlobVec, GlobWithSource};

// TODO(refactor): Track more closely where variables come from (context, computed vars, variant vars)

/// Evaluation context containing variables for template rendering and conditional evaluation
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    /// Variables available during evaluation (e.g., "name", "version", "py", "target_platform")
    variables: IndexMap<String, Variable>,
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
            variables: IndexMap::new(),
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

    /// Create an evaluation context from a map of string variables
    #[cfg(test)]
    pub fn from_map(variables: IndexMap<String, String>) -> Self {
        Self {
            variables: variables
                .into_iter()
                .map(|(k, v)| (k, Variable::from(v)))
                .collect(),
            jinja_config: JinjaConfig::default(),
            accessed_variables: Arc::new(Mutex::new(HashSet::new())),
            undefined_variables: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Create an evaluation context from a map of Variable values
    pub fn from_variables(variables: IndexMap<String, Variable>) -> Self {
        Self {
            variables,
            jinja_config: JinjaConfig::default(),
            accessed_variables: Arc::new(Mutex::new(HashSet::new())),
            undefined_variables: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Create an evaluation context with variables and Jinja config
    pub fn with_config(variables: IndexMap<String, String>, jinja_config: JinjaConfig) -> Self {
        Self {
            variables: variables
                .into_iter()
                .map(|(k, v)| (k, Variable::from(v)))
                .collect(),
            jinja_config,
            accessed_variables: Arc::new(Mutex::new(HashSet::new())),
            undefined_variables: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Insert a variable into the context
    pub fn insert(&mut self, key: String, value: Variable) {
        self.variables.insert(key, value);
    }

    /// Get a variable from the context
    pub fn get(&self, key: &str) -> Option<&Variable> {
        self.variables.get(key)
    }

    /// Check if a variable exists in the context
    pub fn contains(&self, key: &str) -> bool {
        self.variables.contains_key(key)
    }

    /// Get all variables as a reference
    pub fn variables(&self) -> &IndexMap<String, Variable> {
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
    /// A new EvaluationContext with the evaluated context variables merged in, as well as a map of the evaluated context variables
    pub fn with_context(
        &self,
        context_vars: &indexmap::IndexMap<
            String,
            crate::stage0::Value<rattler_build_jinja::Variable>,
        >,
    ) -> Result<(Self, IndexMap<String, Variable>), ParseError> {
        // Clone the current context
        let mut new_context = self.clone();
        let mut evaluated_context = IndexMap::<String, Variable>::new();

        // Evaluate each context variable in order
        for (key, value) in context_vars {
            // Evaluate the value using the current context (which includes previously evaluated context vars)
            // This properly handles templates that evaluate to booleans, integers, or strings
            let evaluated_var =
                crate::stage0::evaluate::evaluate_value_to_variable(value, &new_context)?;

            // Insert the evaluated variable directly
            new_context
                .variables
                .insert(key.clone(), evaluated_var.clone());
            evaluated_context.insert(key.clone(), evaluated_var);
        }

        Ok((new_context, evaluated_context))
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
        ctx.insert("name".to_string(), Variable::from("foo"));
        ctx.insert("version".to_string(), Variable::from("1.0.0"));

        assert_eq!(ctx.get("name"), Some(&Variable::from("foo")));
        assert_eq!(ctx.get("version"), Some(&Variable::from("1.0.0")));
        assert_eq!(ctx.get("unknown"), None);
    }

    #[test]
    fn test_evaluation_context_from_map() {
        let mut map = IndexMap::new();
        map.insert("py".to_string(), "3.11".to_string());
        map.insert("target_platform".to_string(), "linux-64".to_string());

        let ctx = EvaluationContext::from_map(map);
        assert_eq!(ctx.get("py"), Some(&Variable::from("3.11")));
        assert_eq!(
            ctx.get("target_platform"),
            Some(&Variable::from("linux-64"))
        );
    }

    #[test]
    fn test_evaluation_context_contains() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), Variable::from("test"));

        assert!(ctx.contains("name"));
        assert!(!ctx.contains("version"));
    }

    #[test]
    fn test_context_evaluation_simple() {
        use crate::stage0::Value;
        use rattler_build_jinja::Variable;

        let ctx = EvaluationContext::new();

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert(
            "name".to_string(),
            Value::new_concrete(Variable::from("mypackage"), None),
        );
        context_vars.insert(
            "version".to_string(),
            Value::new_concrete(Variable::from("1.0.0"), None),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(
            evaluated_ctx.get("name"),
            Some(&Variable::from("mypackage"))
        );
        assert_eq!(evaluated_ctx.get("version"), Some(&Variable::from("1.0.0")));
    }

    #[test]
    fn test_context_evaluation_with_templates() {
        use crate::stage0::{JinjaTemplate, Value};
        use rattler_build_jinja::Variable;

        let mut ctx = EvaluationContext::new();
        ctx.insert("base".to_string(), Variable::from("myorg"));

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert(
            "name".to_string(),
            Value::new_concrete(Variable::from("mypackage"), None),
        );
        context_vars.insert(
            "full_name".to_string(),
            Value::new_template(
                JinjaTemplate::new("${{ base }}/${{ name }}".to_string()).unwrap(),
                None,
            ),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(
            evaluated_ctx.get("name"),
            Some(&Variable::from("mypackage"))
        );
        assert_eq!(
            evaluated_ctx.get("full_name"),
            Some(&Variable::from("myorg/mypackage"))
        );
    }

    #[test]
    fn test_context_evaluation_forward_references() {
        use crate::stage0::{JinjaTemplate, Value};
        use rattler_build_jinja::Variable;

        let ctx = EvaluationContext::new();

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert(
            "name".to_string(),
            Value::new_concrete(Variable::from("mypackage"), None),
        );
        context_vars.insert(
            "version".to_string(),
            Value::new_concrete(Variable::from("1.0.0"), None),
        );
        context_vars.insert(
            "package_version".to_string(),
            Value::new_template(
                JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap(),
                None,
            ),
        );
        context_vars.insert(
            "full_id".to_string(),
            Value::new_template(
                JinjaTemplate::new("pkg:${{ package_version }}".to_string()).unwrap(),
                None,
            ),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(
            evaluated_ctx.get("package_version"),
            Some(&Variable::from("mypackage-1.0.0"))
        );
        assert_eq!(
            evaluated_ctx.get("full_id"),
            Some(&Variable::from("pkg:mypackage-1.0.0"))
        );
    }

    #[test]
    fn test_context_evaluation_order_matters() {
        use crate::stage0::{JinjaTemplate, Value};
        use rattler_build_jinja::Variable;

        let ctx = EvaluationContext::new();

        // The order matters - package_version references name and version
        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert(
            "name".to_string(),
            Value::new_concrete(Variable::from("mypackage"), None),
        );
        context_vars.insert(
            "version".to_string(),
            Value::new_concrete(Variable::from("2.0.0"), None),
        );
        context_vars.insert(
            "package_version".to_string(),
            Value::new_template(
                JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap(),
                None,
            ),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        assert_eq!(
            evaluated_ctx.get("package_version"),
            Some(&Variable::from("mypackage-2.0.0"))
        );
    }

    #[test]
    fn test_context_evaluation_with_existing_context() {
        use crate::stage0::{JinjaTemplate, Value};
        use rattler_build_jinja::Variable;

        let mut ctx = EvaluationContext::new();
        ctx.insert("platform".to_string(), Variable::from("linux"));

        let mut context_vars = indexmap::IndexMap::new();
        context_vars.insert(
            "name".to_string(),
            Value::new_concrete(Variable::from("mypackage"), None),
        );
        context_vars.insert(
            "full_name".to_string(),
            Value::new_template(
                JinjaTemplate::new("${{ name }}-${{ platform }}".to_string()).unwrap(),
                None,
            ),
        );

        let evaluated_ctx = ctx.with_context(&context_vars).unwrap();

        // Both the original context and the evaluated context should be present
        assert_eq!(
            evaluated_ctx.get("platform"),
            Some(&Variable::from("linux"))
        );
        assert_eq!(
            evaluated_ctx.get("name"),
            Some(&Variable::from("mypackage"))
        );
        assert_eq!(
            evaluated_ctx.get("full_name"),
            Some(&Variable::from("mypackage-linux"))
        );
    }
}
