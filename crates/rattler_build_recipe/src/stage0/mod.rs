//! The stage0 recipe that contains the un-evaluated recipe information.
//! This means, it still contains Jinja templates and if-else statements.

mod about;
mod build;
pub mod evaluate;
mod extra;
mod match_spec;
mod output;
mod package;
mod parser;
mod requirements;
mod source;
mod tests;
mod types;

pub use about::{About, License};
pub use build::{BinaryRelocation, Build};
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
pub use tests::{PythonVersion, TestType};
pub use types::{
    Conditional, ConditionalList, ConditionalListOrItem, IncludeExclude, Item, JinjaExpression,
    JinjaTemplate, ListOrItem, NestedItemList, Value,
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

    #[test]
    fn test_build_platform_from_conditional() {
        // Test that build_platform and target_platform are extracted from
        // conditional if expressions in requirements
        let recipe = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
requirements:
  build:
    - gcc
    - if: build_platform != target_platform
      then:
        - cross-python_${{ target_platform }}
        - numpy
        - python
        "#;

        let parsed = parse_recipe_from_source(recipe).unwrap();
        let used_vars = parsed.used_variables();

        // The if condition uses build_platform and target_platform
        assert!(
            used_vars.contains(&"build_platform".to_string()),
            "build_platform should be extracted from 'if: build_platform != target_platform'"
        );
        assert!(
            used_vars.contains(&"target_platform".to_string()),
            "target_platform should be extracted from 'if: build_platform != target_platform'"
        );
    }

    #[test]
    fn test_skip_plain_string_variable_tracking() {
        // Test that skip conditions as plain strings (not templates) still track variables
        let recipe = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
build:
  skip:
    - not (match(python, python_min ~ ".*") and is_abi3)
        "#;

        let parsed = parse_recipe_from_source(recipe).unwrap();
        let used_vars = parsed.used_variables();

        // Skip expressions should track variables even when not wrapped in ${{ }}
        assert!(
            used_vars.contains(&"python".to_string()),
            "python should be extracted from skip expression"
        );
        assert!(
            used_vars.contains(&"python_min".to_string()),
            "python_min should be extracted from skip expression"
        );
        assert!(
            used_vars.contains(&"is_abi3".to_string()),
            "is_abi3 should be extracted from skip expression"
        );
    }

    #[test]
    fn test_used_vars_in_scripts() {
        let yaml = include_str!("../../test-data/used_vars_in_scripts.yaml");
        let parsed = parse_recipe_from_source(yaml).unwrap();
        let used_vars = parsed.used_variables();

        // Build script variables
        assert!(
            used_vars.contains(&"simple_var".to_string()),
            "simple_var should be extracted from build script. Got: {:?}",
            used_vars
        );
        assert!(
            used_vars.contains(&"unix_var".to_string()),
            "unix_var should be extracted from build script conditional"
        );
        assert!(
            used_vars.contains(&"win_var".to_string()),
            "win_var should be extracted from build script conditional"
        );

        // Condition variables
        assert!(
            used_vars.contains(&"unix".to_string()),
            "unix should be extracted from if conditions"
        );
        assert!(
            used_vars.contains(&"win".to_string()),
            "win should be extracted from if conditions"
        );

        // Test script variables
        assert!(
            used_vars.contains(&"test_var".to_string()),
            "test_var should be extracted from test script. Got: {:?}",
            used_vars
        );
        assert!(
            used_vars.contains(&"test_unix_var".to_string()),
            "test_unix_var should be extracted from test script conditional. Got: {:?}",
            used_vars
        );
        assert!(
            used_vars.contains(&"test_win_var".to_string()),
            "test_win_var should be extracted from test script conditional. Got: {:?}",
            used_vars
        );
    }

    #[test]
    fn test_source_patch_conditional_variable_extraction() {
        // Test that variables from conditional if expressions in source patches are extracted
        let recipe = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
source:
  url: https://example.com/test.tar.gz
  sha256: 0000000000000000000000000000000000000000000000000000000000000000
  patches:
    - if: win
      then: msvc_warnings.patch
    - if: "python_impl == 'pypy'"
      then: pypy-compat.patch
        "#;

        let parsed = parse_recipe_from_source(recipe).unwrap();
        let used_vars = parsed.used_variables();

        // Variables from patch conditionals should be extracted
        assert!(
            used_vars.contains(&"win".to_string()),
            "win should be extracted from patch conditional 'if: win'"
        );
        assert!(
            used_vars.contains(&"python_impl".to_string()),
            "python_impl should be extracted from patch conditional 'if: python_impl == 'pypy''"
        );
    }
}
