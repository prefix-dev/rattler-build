//! Jinja template engine integration for rattler-build, powered by minijinja,
//! providing template rendering, variable extraction, and built-in functions for recipes.

mod ast_variables;
mod env;
mod git;
mod jinja;
mod utils;
mod variable;

pub use ast_variables::{JinjaExpression, JinjaTemplate};
pub use jinja::{Jinja, JinjaConfig};
pub use rattler_build_types::NormalizedKey;
pub use variable::Variable;

// re-export undefined behavior enum
pub use jinja::UndefinedBehavior;
