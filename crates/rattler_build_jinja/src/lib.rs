mod ast_variables;
mod env;
mod git;
mod jinja;
mod utils;
mod variable;

pub use ast_variables::{
    JinjaExpression, JinjaTemplate, extract_default_guarded_variables_from_expression,
    extract_default_guarded_variables_from_template,
};
pub use jinja::{Jinja, JinjaConfig};
pub use rattler_build_types::NormalizedKey;
pub use variable::Variable;

// re-export undefined behavior enum
pub use jinja::UndefinedBehavior;
