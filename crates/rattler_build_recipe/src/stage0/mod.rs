//! The stage0 recipe that contains the un-evaluated recipe information.
//! This means, it still contains Jinja templates and if-else statements.

mod about;
mod build;
pub mod evaluate;
mod extra;
pub mod jinja_functions;
mod package;
mod parser;
mod requirements;
mod source;
mod tests;
mod types;

pub use about::About;
pub use build::Build;
pub use extra::Extra;
pub use package::Package;
pub use parser::{parse_recipe, parse_recipe_from_source};
pub use requirements::Requirements;
pub use source::Source;
pub use tests::TestType;
pub use types::{
    Conditional, ConditionalList, Item, JinjaExpression, JinjaTemplate, ListOrItem, Value,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Stage0Recipe {
    pub package: package::Package,
    pub build: build::Build,
    pub requirements: Requirements,
    pub about: about::About,
    pub extra: extra::Extra,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<source::Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<tests::TestType>,
}

impl Stage0Recipe {
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = self.package.used_variables();
        vars.extend(self.build.used_variables());
        vars.extend(self.requirements.used_variables());
        vars.extend(self.about.used_variables());
        vars.extend(self.extra.used_variables());
        for src in &self.source {
            vars.extend(src.used_variables());
        }
        for test in &self.tests {
            vars.extend(test.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
