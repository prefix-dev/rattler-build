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
    SingleOutputRecipe, StagingBuild, StagingMetadata, StagingOutput,
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

    #[test]
    fn test_used_vars_from_expressions() {
        let recipe = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
requirements:
  build:
    - if: llvm_variant > 10
      then: llvm >=10
    - if: linux
      then: linux-gcc
    - if: osx
      then: osx-clang
    - ${{ compiler('c') }}
    - ${{ stdlib('c') }}
    - ${{ pin_subpackage(abcdef) }}
    - ${{ pin_subpackage("foobar") }}
    - ${{ pin_compatible(compatible) }}
    - ${{ pin_compatible(abc ~ def) }}
    - if: match(xpython, ">=3.7")
      then: numpy ==100
    - ${{ testexprvar is string }}
    - ${{ match(match_a_var, match_b_var) }}
        "#;

        let parsed = parse_recipe_from_source(recipe).unwrap();
        let used_vars = parsed.used_variables();

        assert!(used_vars.contains(&"llvm_variant".to_string()));
        assert!(used_vars.contains(&"linux".to_string()));
        assert!(used_vars.contains(&"osx".to_string()));
        assert!(used_vars.contains(&"c_compiler".to_string()));
        assert!(used_vars.contains(&"c_compiler_version".to_string()));
        assert!(used_vars.contains(&"c_stdlib".to_string()));
        assert!(used_vars.contains(&"c_stdlib_version".to_string()));
        assert!(used_vars.contains(&"abcdef".to_string()));
        assert!(!used_vars.contains(&"foobar".to_string()));
        assert!(used_vars.contains(&"compatible".to_string()));
        assert!(used_vars.contains(&"abc".to_string()));
        assert!(used_vars.contains(&"def".to_string()));
        assert!(used_vars.contains(&"xpython".to_string()));
        assert!(used_vars.contains(&"testexprvar".to_string()));
        assert!(used_vars.contains(&"match_a_var".to_string()));
        assert!(used_vars.contains(&"match_b_var".to_string()));
    }

    #[test]
    fn test_conditional_compiler() {
        let recipe = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
requirements:
  build:
    - ${{ compiler('c') if linux }}
    - ${{ bla if linux else foo }}
        "#;

        let parsed = parse_recipe_from_source(recipe).unwrap();
        let used_vars = parsed.used_variables();

        assert!(used_vars.contains(&"c_compiler".to_string()));
        assert!(used_vars.contains(&"c_compiler_version".to_string()));
        assert!(used_vars.contains(&"linux".to_string()));
        assert!(used_vars.contains(&"bla".to_string()));
        assert!(used_vars.contains(&"foo".to_string()));
    }

    #[test]
    fn test_used_vars_from_expressions_with_skip() {
        let recipe = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
build:
  skip:
    - ${{ llvm_variant > 10 }}
    - ${{ linux }}
    - ${{ cuda }}
        "#;

        let parsed = parse_recipe_from_source(recipe).unwrap();
        let used_vars = parsed.used_variables();

        assert!(used_vars.contains(&"llvm_variant".to_string()));
        assert!(used_vars.contains(&"cuda".to_string()));
        assert!(used_vars.contains(&"linux".to_string()));
        assert!(!used_vars.contains(&"osx".to_string()));
    }
}
