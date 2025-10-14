//! The stage0 recipe that contains the un-evaluated recipe information.
//! This means, it still contains Jinja templates and if-else statements.

mod about;
mod build;
pub mod evaluate;
mod extra;
pub mod jinja_functions;
mod match_spec;
mod output;
mod package;
mod parser;
mod requirements;
mod source;
mod tests;
mod types;

pub use about::{About, License};
pub use build::Build;
pub use extra::Extra;
pub use match_spec::SerializableMatchSpec;
pub use output::{
    CacheInherit, Inherit, MultiOutputRecipe, Output, PackageOutput, Recipe, RecipeMetadata,
    SingleOutputRecipe, StagingBuild, StagingMetadata, StagingOutput, StagingRequirements,
};
pub use package::{Package, PackageMetadata, PackageName};
pub use parser::{
    parse_recipe, parse_recipe_from_source, parse_recipe_or_multi,
    parse_recipe_or_multi_from_source,
};
pub use requirements::Requirements;
pub use source::Source;
pub use tests::TestType;
pub use types::{
    Conditional, ConditionalList, IncludeExclude, Item, JinjaExpression, JinjaTemplate, ListOrItem,
    Value,
};

/// Backwards compatibility alias for Stage0Recipe
/// This is now the same as SingleOutputRecipe
pub type Stage0Recipe = SingleOutputRecipe;

#[cfg(test)]
mod test {
    // make a roundtrip test from yaml -> struct -> yaml
    use super::*;

    fn roundtrip(yaml: &str) -> String {
        let recipe = parse_recipe_from_source(yaml).unwrap();
        serde_yaml::to_string(&recipe).unwrap()
    }

    #[test]
    fn test_roundtrip() {
        // load `conditionals.yaml` from the test-data directory
        let yaml = include_str!("../../test-data/conditionals.yaml");
        let roundtripped = roundtrip(yaml);
        insta::assert_snapshot!(roundtripped);
    }
}
