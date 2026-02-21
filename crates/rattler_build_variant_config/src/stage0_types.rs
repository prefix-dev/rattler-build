//! Stage0 types for variant configuration - templates and conditionals before evaluation
//!
//! This module re-exports types from rattler_build_yaml_parser specialized for Variable.

use rattler_build_jinja::Variable;

// Re-export all types from the shared parser, specialized for Variable
pub type Value<T = Variable> = rattler_build_yaml_parser::Value<T>;
pub type ConditionalList<T = Variable> = rattler_build_yaml_parser::ConditionalList<T>;
pub type Conditional<T = Variable> = rattler_build_yaml_parser::Conditional<T>;
pub type Item<T = Variable> = rattler_build_yaml_parser::Item<T>;
pub type ListOrItem<T = Variable> = rattler_build_yaml_parser::ListOrItem<T>;
