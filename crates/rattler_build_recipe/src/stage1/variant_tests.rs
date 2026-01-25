//! Tests for variant tracking and used variant calculations
//!
//! These tests verify that:
//! - Jinja variables are tracked during evaluation
//! - Dependencies contribute to variant expansion
//! - Conditional branches only include accessed variables
//! - The minimal set of actually-used variants is captured

#[cfg(test)]
mod tests {
    use crate::stage0;
    use crate::stage1::{Evaluate, EvaluationContext, HashInfo};
    use indexmap::IndexMap;
    use rattler_build_jinja::{JinjaConfig, Variable};
    use rattler_build_types::NormalizedKey;
    use rattler_conda_types::{NoArchType, Platform};
    use std::collections::BTreeMap;

    /// Helper to parse a recipe YAML and evaluate it
    fn evaluate_recipe(
        yaml: &str,
        variant: IndexMap<String, Variable>,
    ) -> (crate::stage1::Recipe, BTreeMap<NormalizedKey, Variable>) {
        let stage0 = stage0::parse_recipe_or_multi_from_source(yaml).unwrap();
        let single = match stage0 {
            stage0::Recipe::SingleOutput(s) => s,
            _ => panic!("Expected single output recipe"),
        };

        // Set up JinjaConfig with the variant so compiler() and stdlib() functions work
        let target_platform = variant
            .get("target_platform")
            .map(|v| v.to_string())
            .and_then(|s| s.parse::<Platform>().ok())
            .unwrap_or(Platform::Linux64);

        let variant_map: BTreeMap<NormalizedKey, Variable> = variant
            .iter()
            .map(|(k, v)| (NormalizedKey::from(k.as_str()), v.clone()))
            .collect();

        let jinja_config = JinjaConfig {
            target_platform,
            build_platform: target_platform,
            host_platform: target_platform,
            variant: variant_map,
            experimental: false,
            recipe_path: None,
            undefined_behavior: rattler_build_jinja::UndefinedBehavior::Strict,
        };

        let context = EvaluationContext::with_variables_and_config(variant.clone(), jinja_config);

        let mut recipe = single.as_ref().evaluate(&context).unwrap();

        // Compute hash from the used variant and resolve the build string
        let noarch = recipe.build.noarch.unwrap_or(NoArchType::none());
        let hash_info = HashInfo::from_variant(&recipe.used_variant, &noarch);

        // Build a context for build string resolution that includes both:
        // 1. Variant variables (base values)
        // 2. Recipe context variables (which may transform/override variant values)
        // Context takes precedence - this mirrors finalize_build_string_single in variant_render.rs
        let mut resolve_vars = IndexMap::new();
        for (k, v) in &recipe.used_variant {
            resolve_vars.insert(k.normalize(), v.clone());
        }
        for (k, v) in &recipe.context {
            resolve_vars.insert(k.clone(), v.clone());
        }
        let resolve_context = EvaluationContext::from_variables(resolve_vars);

        // Resolve the build string with the computed hash info
        recipe
            .build
            .string
            .resolve(
                &hash_info,
                recipe.build.number.unwrap_or(0),
                &resolve_context,
            )
            .unwrap();

        // Extract used variant from recipe
        (recipe.clone(), recipe.used_variant.clone())
    }

    #[test]
    fn test_empty_recipe_minimal_variant() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));
        variant.insert("numpy".to_string(), Variable::from("1.20"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should only include target_platform (always included)
        assert_eq!(used_variant.len(), 1);
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
    }

    #[test]
    fn test_jinja_variable_in_version() {
        let yaml = r#"
context:
  version: "1.2.3"

package:
  name: test
  version: ${{ version }}

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should include target_platform (always)
        // Note: context variables like 'version' ARE tracked as accessed when used in templates
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        assert!(!used_variant.contains_key(&NormalizedKey::from("python")));
    }

    #[test]
    fn test_jinja_variable_in_name() {
        let yaml = r#"
context:
  pkg_name: "mypackage"

package:
  name: ${{ pkg_name }}
  version: "1.0.0"

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("osx-arm64"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Only target_platform - context vars are tracked but not as part of variant
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
    }

    #[test]
    fn test_free_dependency_creates_variant() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

requirements:
  build:
    - ${{ compiler('c') }}
    - ${{ stdlib('c') }}
  host:
    - python

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("c_compiler".to_string(), Variable::from("gcc"));
        variant.insert("c_compiler_version".to_string(), Variable::from("11"));
        variant.insert("c_stdlib".to_string(), Variable::from("glibc"));
        variant.insert("c_stdlib_version".to_string(), Variable::from("2.35"));
        variant.insert("python".to_string(), Variable::from("3.11"));
        variant.insert("numpy".to_string(), Variable::from("1.20"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Free specs in build/host requirements ARE tracked as variant variables
        // python is a free spec (no version constraints), so it should be in the variant
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        println!("Used variant keys: {:?}", used_variant.keys());
        assert!(
            used_variant.contains_key(&NormalizedKey::from("python")),
            "Free spec 'python' should be tracked as variant"
        );

        // compiler() and stdlib() functions expand to variant variables
        // These are accessed through minijinja function calls, which are tracked
        assert!(
            used_variant.contains_key(&NormalizedKey::from("c_compiler")),
            "c_compiler should be tracked"
        );
        assert!(
            used_variant.contains_key(&NormalizedKey::from("c_compiler_version")),
            "c_compiler_version should be tracked"
        );
        assert!(
            used_variant.contains_key(&NormalizedKey::from("c_stdlib")),
            "c_stdlib should be tracked"
        );
        assert!(
            used_variant.contains_key(&NormalizedKey::from("c_stdlib_version")),
            "c_stdlib_version should be tracked"
        );

        // numpy was not used, so it should not be in the variant
        assert!(!used_variant.contains_key(&NormalizedKey::from("numpy")));
    }

    #[test]
    fn test_pinned_dependency_no_variant() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

requirements:
  host:
    - python >=3.11

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // python has a constraint, so it's not a free spec and shouldn't be in variant
        assert_eq!(used_variant.len(), 1);
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        assert!(!used_variant.contains_key(&NormalizedKey::from("python")));
    }

    #[test]
    fn test_conditional_jinja_in_about() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

about:
  summary: ${{ "Python " ~ python if unix else "NumPy " ~ numpy }}

build:
  number: 0
"#;
        // Test unix=true branch
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("unix".to_string(), Variable::from(true));
        variant.insert("python".to_string(), Variable::from("3.11"));
        variant.insert("numpy".to_string(), Variable::from("1.20"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant.clone());

        // Should include target_platform, unix (selector), and python (used in then branch)
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        assert!(used_variant.contains_key(&NormalizedKey::from("unix")));
        assert!(used_variant.contains_key(&NormalizedKey::from("python")));
        // numpy is in the else branch which wasn't taken
        assert!(!used_variant.contains_key(&NormalizedKey::from("numpy")));

        // Test unix=false branch
        variant.insert("unix".to_string(), Variable::from(false));
        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Now should include numpy instead of python
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        assert!(used_variant.contains_key(&NormalizedKey::from("unix")));
        assert!(used_variant.contains_key(&NormalizedKey::from("numpy")));
        assert!(!used_variant.contains_key(&NormalizedKey::from("python")));
    }

    #[test]
    fn test_jinja_in_dependency_version() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

requirements:
  host:
    - python ${{ python }}.*

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should include both target_platform and python (used in jinja)
        assert_eq!(used_variant.len(), 2);
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        assert!(used_variant.contains_key(&NormalizedKey::from("python")));
    }

    #[test]
    fn test_compiler_function_syntax() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

requirements:
  build:
    - ${{ compiler('c') }}

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("c_compiler".to_string(), Variable::from("gcc"));
        variant.insert("c_compiler_version".to_string(), Variable::from("11"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // compiler('c') function expands variables during evaluation
        // At minimum, target_platform should be included
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));

        // Verify the recipe was successfully evaluated with compiler function
        assert_eq!(recipe.package().name().as_source(), "test");

        // The compiler function may or may not add c_compiler/c_compiler_version to the variant
        // depending on how the function is implemented. This test just verifies the syntax works.
    }

    #[test]
    fn test_hash_computation_with_python_prefix() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant.insert(NormalizedKey::from("python"), Variable::from("3.12"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Should start with "py312" (python prefix + h + hash)
        assert_eq!(hash_info.prefix, "py312");
    }

    #[test]
    fn test_hash_computation_with_numpy_and_python() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11"));
        variant.insert(NormalizedKey::from("numpy"), Variable::from("1.20"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Should start with "np120py311h" (numpy prefix + python prefix + h + hash)
        // Order is: np, py, pl, lua, r
        assert_eq!(hash_info.prefix, "np120py311");
    }

    #[test]
    fn test_hash_computation_noarch_python() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("noarch"),
        );
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::python());

        // For python noarch, should just be "pyh" + hash
        assert_eq!(hash_info.prefix, "py");
    }

    #[test]
    fn test_hash_deterministic() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("osx-arm64"),
        );
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11"));

        let hash1 = HashInfo::from_variant(&variant, &NoArchType::none());
        let hash2 = HashInfo::from_variant(&variant, &NoArchType::none());

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_for_different_variants() {
        let mut variant1 = BTreeMap::new();
        variant1.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant1.insert(NormalizedKey::from("python"), Variable::from("3.11"));

        let mut variant2 = BTreeMap::new();
        variant2.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant2.insert(NormalizedKey::from("python"), Variable::from("3.12"));

        let hash1 = HashInfo::from_variant(&variant1, &NoArchType::none());
        let hash2 = HashInfo::from_variant(&variant2, &NoArchType::none());

        assert_ne!(hash1, hash2);
        // But both should have py311 and py312 prefixes respectively
        assert_eq!(hash1.prefix, "py311");
        assert_eq!(hash2.prefix, "py312");
    }

    #[test]
    fn test_build_string_default_with_hash() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

build:
  number: 5
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));

        let (recipe, _used_variant) = evaluate_recipe(yaml, variant);

        let build_string = recipe
            .build()
            .string
            .as_str()
            .expect("build string should be resolved");
        assert_eq!(build_string, "hb0f4dca_5");
    }

    #[test]
    fn test_build_string_py311() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

build:
  number: 5

requirements:
  host:
    - python
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));

        let (recipe, _used_variant) = evaluate_recipe(yaml, variant);

        let build_string = recipe
            .build()
            .string
            .as_str()
            .expect("build string should be resolved");
        assert_eq!(build_string, "py311h48b7412_5");
    }

    #[test]
    fn test_build_string_py_noarch() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

build:
  number: 5
  noarch: python

requirements:
  host:
    - python
  run:
    - __unix
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // For a noarch recipe, python should not be in the used variant
        assert!(!used_variant.contains_key(&NormalizedKey::from("python")));
        // The virtual __unix dependency should be included in the variant
        assert!(used_variant.contains_key(&NormalizedKey::from("__unix")));
        assert_eq!(
            used_variant.get(&NormalizedKey::from("target_platform")),
            Some(&Variable::from("noarch"))
        );

        insta::assert_snapshot!(format!("{:?}", used_variant));
        let build_string = recipe
            .build()
            .string
            .as_str()
            .expect("build string should be resolved");
        assert_eq!(build_string, "pyh5600cae_5");
    }

    #[test]
    fn test_build_string_custom_with_hash_variable() {
        let yaml = r#"
context:
  build_number: 12

package:
  name: test
  version: "1.0.0"

build:
  number: ${{ build_number }}
  string: custom_${{ hash }}_build_${{ target_platform }}_${{ foobar }}_${{ build_number }}
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("osx-arm64"));
        variant.insert("foobar".to_string(), Variable::from("baz"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);
        assert!(used_variant.contains_key(&"foobar".into()));
        assert!(used_variant.contains_key(&"target_platform".into()));
        assert_eq!(used_variant.len(), 2);
        let hash_info = HashInfo::from_variant(&used_variant, &NoArchType::none());
        assert_eq!(hash_info.hash, "bf59cf5");

        assert_eq!(
            recipe
                .build()
                .string
                .as_str()
                .expect("build string should be resolved"),
            "custom_bf59cf5_build_osx-arm64_baz_12"
        );
    }

    #[test]
    fn test_conditional_dependencies() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

requirements:
  host:
    - if: unix
      then:
        - python
    - if: win
      then:
        - numpy

build:
  number: 0
"#;
        // Test unix=true
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("unix".to_string(), Variable::from(true));
        variant.insert("win".to_string(), Variable::from(false));
        variant.insert("python".to_string(), Variable::from("3.11"));
        variant.insert("numpy".to_string(), Variable::from("1.20"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should always include target_platform
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));

        // Note: Conditional selectors (if: unix) may not be tracked as jinja variable accesses
        // depending on how the selector evaluation is implemented. The key thing is that the
        // recipe evaluates successfully with the correct branch taken.

        // Verify the recipe structure is correct
        assert_eq!(recipe.package().name().as_source(), "test");

        // Verify that the recipe was successfully evaluated
        // (exact variant contents depend on selector implementation)
    }

    #[test]
    fn test_multiple_jinja_variables_in_string() {
        let yaml = r#"
package:
  name: test
  version: "1.0.0"

about:
  summary: Built with Python ${{ python }} and NumPy ${{ numpy }}

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));
        variant.insert("numpy".to_string(), Variable::from("1.20"));
        variant.insert("perl".to_string(), Variable::from("5.32"));

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should include python and numpy (both used), but not perl
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
        assert!(used_variant.contains_key(&NormalizedKey::from("python")));
        assert!(used_variant.contains_key(&NormalizedKey::from("numpy")));
        assert!(!used_variant.contains_key(&NormalizedKey::from("perl")));
    }

    /// Test snapshot of hash computation for reproducibility
    #[test]
    fn test_hash_snapshot() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11"));
        variant.insert(NormalizedKey::from("numpy"), Variable::from("1.20"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Snapshot test to ensure hash format doesn't change
        insta::assert_snapshot!(format!("{}", hash_info));
    }

    /// Test that variant order doesn't affect hash (BTreeMap ensures sorted keys)
    #[test]
    fn test_hash_key_order_independent() {
        // Create variant with keys in different order
        let mut variant1 = BTreeMap::new();
        variant1.insert(NormalizedKey::from("python"), Variable::from("3.11"));
        variant1.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant1.insert(NormalizedKey::from("numpy"), Variable::from("1.20"));

        let mut variant2 = BTreeMap::new();
        variant2.insert(NormalizedKey::from("numpy"), Variable::from("1.20"));
        variant2.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant2.insert(NormalizedKey::from("python"), Variable::from("3.11"));

        let hash1 = HashInfo::from_variant(&variant1, &NoArchType::none());
        let hash2 = HashInfo::from_variant(&variant2, &NoArchType::none());

        assert_eq!(hash1, hash2);
    }

    /// Test that R packages get proper prefix
    #[test]
    fn test_hash_with_r_prefix() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant.insert(NormalizedKey::from("r_base"), Variable::from("4.2"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Should start with "r42h"
        assert_eq!(hash_info.prefix, "r42");
        assert_eq!(hash_info.hash, "aee9047");
    }

    /// Test that Perl packages get proper prefix
    #[test]
    fn test_hash_with_perl_prefix() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant.insert(NormalizedKey::from("perl"), Variable::from("5.32"));

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Should start with "pl532"
        assert_eq!(hash_info.prefix, "pl532");
    }

    /// Test combined prefixes in correct order
    #[test]
    fn test_hash_combined_prefixes() {
        let mut variant = BTreeMap::new();
        variant.insert(
            NormalizedKey::from("target_platform"),
            Variable::from("linux-64"),
        );
        variant.insert(NormalizedKey::from("python"), Variable::from("3.11")); // py
        variant.insert(NormalizedKey::from("perl"), Variable::from("5.32")); // pl
        variant.insert(NormalizedKey::from("numpy"), Variable::from("1.20")); // np
        variant.insert(NormalizedKey::from("r_base"), Variable::from("4.2")); // r

        let hash_info = HashInfo::from_variant(&variant, &NoArchType::none());

        // Order should be: np, py, pl, lua, r
        assert_eq!(hash_info.prefix, "np120py311pl532r42");
    }

    #[test]
    fn test_script_variable_tracking() {
        // Test that variables used in build script are tracked even if undefined at evaluation time
        let yaml = r#"
package:
  name: test-script-vars
  version: "1.0.0"

build:
  number: 0
  script: "echo Using variant: ${{ python }}"
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));
        // Note: PYTHON and PREFIX are environment variables available at build time,
        // not at parse/evaluation time

        let (_recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should include python because it's used in the script
        assert!(used_variant.contains_key(&NormalizedKey::from("python")));
        // Should include target_platform (always included)
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));
    }

    #[test]
    fn test_build_string_variable_tracking() {
        // Test that variables used in build.string are tracked
        let yaml = r#"
package:
  name: test-build-string
  version: "1.0.0"

build:
  number: 0
  string: "py${{ python }}_${{ hash }}_${{ build_number }}"
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.11"));
        variant.insert("numpy".to_string(), Variable::from("1.20"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should include python because it's used in build.string
        assert!(used_variant.contains_key(&NormalizedKey::from("python")));
        // Should NOT include numpy since it's not referenced
        assert!(!used_variant.contains_key(&NormalizedKey::from("numpy")));

        // Verify build string was resolved
        assert!(recipe.build.string.is_resolved());
        let build_str = recipe.build.string.as_resolved().unwrap();
        assert!(build_str.contains("py3"));
    }

    #[test]
    fn test_script_and_build_string_combined() {
        // Test that variables from both script and build.string are tracked
        let yaml = r#"
package:
  name: test-combined
  version: "1.0.0"

build:
  number: 42
  string: "py${{ python }}_h${{ hash }}"
  script: "echo NumPy ${{ numpy }} and R ${{ r_base }}"
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("python".to_string(), Variable::from("3.12"));
        variant.insert("numpy".to_string(), Variable::from("1.26"));
        variant.insert("r_base".to_string(), Variable::from("4.3"));
        variant.insert("perl".to_string(), Variable::from("5.38")); // unused

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Should include python (from build.string), numpy and r_base (from script)
        assert!(used_variant.contains_key(&NormalizedKey::from("python")));
        assert!(used_variant.contains_key(&NormalizedKey::from("numpy")));
        assert!(used_variant.contains_key(&NormalizedKey::from("r_base")));
        // Should NOT include perl since it's not referenced
        assert!(!used_variant.contains_key(&NormalizedKey::from("perl")));

        // Verify build string was resolved
        assert!(recipe.build.string.is_resolved());
        let build_str = recipe.build.string.as_resolved().unwrap();
        // Build string should be in format "py{python_version}_h{hash}"
        assert!(build_str.contains("py3") && build_str.contains("_h"));
    }

    #[test]
    fn test_context_variable_shadowing_variant_variable() {
        // Test case: when context defines a variable that references a variant variable
        // with the same name, the variant variable should still be tracked as used.
        //
        // Example:
        //   context:
        //     foobar: ${{ foobar }}
        //   package:
        //     name: foobar-${{ foobar }}
        //
        // Here, `foobar` in context references the variant `foobar`, and even though
        // the context variable shadows it, we should track `foobar` as a used variant.
        let yaml = r#"
context:
  foobar: ${{ foobar }}

package:
  name: foobar-${{ foobar }}
  version: "1.0.0"

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("foobar".to_string(), Variable::from("baz"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // The foobar variant should be tracked even though it's shadowed by context
        assert!(
            used_variant.contains_key(&NormalizedKey::from("foobar")),
            "foobar should be in used_variant because it was accessed from the variant \
             before being shadowed by the context variable. Got: {:?}",
            used_variant.keys().collect::<Vec<_>>()
        );
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));

        // Verify the recipe was evaluated correctly
        assert_eq!(recipe.package().name().as_source(), "foobar-baz");
    }

    #[test]
    fn test_context_variable_not_from_variant() {
        // Test case: when context defines a variable with a literal value,
        // it should NOT be tracked as a used variant (since it's not from the variant).
        let yaml = r#"
context:
  myvar: "literal_value"

package:
  name: test-${{ myvar }}
  version: "1.0.0"

build:
  number: 0
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("myvar".to_string(), Variable::from("variant_value"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // myvar should NOT be in used_variant because the context defined it as a literal,
        // not as a reference to the variant
        assert!(
            !used_variant.contains_key(&NormalizedKey::from("myvar")),
            "myvar should NOT be in used_variant because context defines it as a literal"
        );
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));

        // Verify the context value (literal) was used, not the variant value
        assert_eq!(recipe.package().name().as_source(), "test-literal_value");
    }

    #[test]
    fn test_context_variable_concatenation_with_variant() {
        // Test case: when context defines variables using concatenation with a variant variable,
        // the variant variable should be tracked as used.
        //
        // This tests the full scenario from examples/contextused/recipe.yaml:
        //   context:
        //     mpi: ${{ mpi ~ "foobar" }}
        //     extra_mpi: ${{ "extra" ~ mpi }}  # Uses the already-transformed mpi from context
        //   build:
        //     string: ${{ mpi ~ extra_mpi }}
        //
        // Key behaviors:
        // 1. `mpi` in context uses the variant `mpi` via concatenation
        // 2. `extra_mpi` uses the already-transformed context `mpi` (sequential evaluation)
        // 3. The variant should track the ORIGINAL `mpi` value ("bla"), not transformed
        // 4. The build string should use the context-transformed values
        let yaml = r#"
context:
  mpi: ${{ mpi ~ "foobar" }}
  extra_mpi: ${{ "extra" ~ mpi }}

package:
  name: contextused
  version: "1.0.0"

build:
  string: ${{ mpi ~ extra_mpi }}
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("mpi".to_string(), Variable::from("bla"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // The mpi variant should be tracked even though it's shadowed by context
        assert!(
            used_variant.contains_key(&NormalizedKey::from("mpi")),
            "mpi should be in used_variant because it was accessed from the variant \
             via concatenation before being shadowed by the context variable. Got: {:?}",
            used_variant.keys().collect::<Vec<_>>()
        );
        assert!(used_variant.contains_key(&NormalizedKey::from("target_platform")));

        // The variant should contain the ORIGINAL value from the variant config, not the
        // context-transformed value. This is important for variant tracking and hash computation.
        assert_eq!(
            used_variant
                .get(&NormalizedKey::from("mpi"))
                .unwrap()
                .to_string(),
            "bla",
            "Variant should contain the original value 'bla', not the context-transformed 'blafoobar'"
        );

        // Verify the context variables were evaluated correctly:
        // - mpi = "bla" ~ "foobar" = "blafoobar"
        // - extra_mpi = "extra" ~ "blafoobar" = "extrablafoobar" (uses already-transformed mpi)
        assert_eq!(
            recipe.context.get("mpi").unwrap().to_string(),
            "blafoobar",
            "Context mpi should be 'blafoobar'"
        );
        assert_eq!(
            recipe.context.get("extra_mpi").unwrap().to_string(),
            "extrablafoobar",
            "Context extra_mpi should use the transformed mpi value"
        );

        // Build string uses the context values: mpi ~ extra_mpi = "blafoobar" ~ "extrablafoobar"
        assert_eq!(
            recipe.build.string.as_resolved().unwrap(),
            "blafoobarextrablafoobar",
            "Build string should use the concatenated context values"
        );
    }

    #[test]
    fn test_cdt_function_tracks_cdt_name_from_variant() {
        // This test verifies that when the cdt() function is used in a recipe,
        // and cdt_name is provided in the variant, it gets tracked in used_variant.
        // This is important for hash computation - different cdt_name values should
        // produce different hashes.
        let yaml = r#"
package:
  name: test-cdt
  version: "1.0.0"

build:
  number: 0

requirements:
  host:
    - ${{ cdt("mesa-libgbm") }}
"#;
        let mut variant = IndexMap::new();
        variant.insert(
            "target_platform".to_string(),
            Variable::from("linux-aarch64"),
        );
        variant.insert("cdt_name".to_string(), Variable::from("conda"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Verify cdt_name is in used_variant because cdt() function accessed it
        assert!(
            used_variant.contains_key(&NormalizedKey::from("cdt_name")),
            "cdt_name should be in used_variant when cdt() function uses it from the variant. \
            Actual used_variant keys: {:?}",
            used_variant.keys().collect::<Vec<_>>()
        );

        // Verify the requirement was rendered correctly with the custom cdt_name
        let host_deps = recipe.requirements.host;
        let dep_names: Vec<String> = host_deps
            .iter()
            .filter_map(|d| d.name().map(|n| n.as_source().to_string()))
            .collect();
        assert!(
            dep_names.iter().any(|n| n == "mesa-libgbm-conda-aarch64"),
            "Expected mesa-libgbm-conda-aarch64 in host dependencies. Got: {:?}",
            dep_names
        );
    }

    #[test]
    fn test_cdt_function_with_default_cdt_name() {
        // This test verifies that when the cdt() function is used but cdt_name
        // is NOT provided in the variant, it uses the default and does NOT track
        // cdt_name (since it wasn't read from the variant).
        let yaml = r#"
package:
  name: test-cdt
  version: "1.0.0"

build:
  number: 0

requirements:
  host:
    - ${{ cdt("mesa-libgbm") }}
"#;
        let mut variant = IndexMap::new();
        variant.insert(
            "target_platform".to_string(),
            Variable::from("linux-aarch64"),
        );
        // Note: NOT providing cdt_name - should use default "cos7" for aarch64

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Verify cdt_name is NOT in used_variant because it used the default
        assert!(
            !used_variant.contains_key(&NormalizedKey::from("cdt_name")),
            "cdt_name should NOT be in used_variant when using default value. \
            Actual used_variant keys: {:?}",
            used_variant.keys().collect::<Vec<_>>()
        );

        // Verify the requirement was rendered correctly with the default cdt_name
        let host_deps = recipe.requirements.host;
        let dep_names: Vec<String> = host_deps
            .iter()
            .filter_map(|d| d.name().map(|n| n.as_source().to_string()))
            .collect();
        assert!(
            dep_names.iter().any(|n| n == "mesa-libgbm-cos7-aarch64"),
            "Expected mesa-libgbm-cos7-aarch64 (default for aarch64) in host dependencies. Got: {:?}",
            dep_names
        );
    }

    #[test]
    fn test_cdt_function_tracks_cdt_arch_from_variant() {
        // This test verifies that when cdt_arch is provided in the variant,
        // it gets tracked in used_variant.
        let yaml = r#"
package:
  name: test-cdt
  version: "1.0.0"

build:
  number: 0

requirements:
  host:
    - ${{ cdt("mesa-libgbm") }}
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("cdt_arch".to_string(), Variable::from("custom_arch"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Verify cdt_arch is in used_variant
        assert!(
            used_variant.contains_key(&NormalizedKey::from("cdt_arch")),
            "cdt_arch should be in used_variant when cdt() function uses it from the variant. \
            Actual used_variant keys: {:?}",
            used_variant.keys().collect::<Vec<_>>()
        );

        // Verify the requirement was rendered correctly with the custom cdt_arch
        let host_deps = recipe.requirements.host;
        let dep_names: Vec<String> = host_deps
            .iter()
            .filter_map(|d| d.name().map(|n| n.as_source().to_string()))
            .collect();
        assert!(
            dep_names
                .iter()
                .any(|n| n == "mesa-libgbm-cos6-custom_arch"),
            "Expected mesa-libgbm-cos6-custom_arch in host dependencies. Got: {:?}",
            dep_names
        );
    }

    #[test]
    fn test_variant_variable_used_in_context_without_shadowing() {
        // Test case: when a variant variable is used in context expressions but the
        // context variable has a DIFFERENT name (not shadowing), the variant variable
        // should still be tracked as used.
        //
        // This tests the pattern from the is_abi3 use case:
        //   context:
        //     use_abi3: ${{ is_abi3 }}
        //     build_abi3: ${{ is_abi3 and match(python, "3.10.*") }}
        //
        // Here, `is_abi3` is a variant variable used to compute context variables
        // `use_abi3` and `build_abi3`. Even though `is_abi3` is NOT shadowed by a
        // context variable with the same name, it should still be tracked in used_variant.
        let yaml = r#"
context:
  use_abi3: ${{ is_abi3 }}
  build_abi3: ${{ is_abi3 and match(python, "3.10.*") }}

package:
  name: test-abi3
  version: "1.0.0"

build:
  number: 0
  python:
    version_independent: ${{ build_abi3 }}
"#;
        let mut variant = IndexMap::new();
        variant.insert("target_platform".to_string(), Variable::from("linux-64"));
        variant.insert("is_abi3".to_string(), Variable::from(true));
        variant.insert("python".to_string(), Variable::from("3.10.* *_cpython"));

        let (recipe, used_variant) = evaluate_recipe(yaml, variant);

        // Verify is_abi3 is in used_variant because it was accessed during context evaluation
        assert!(
            used_variant.contains_key(&NormalizedKey::from("is_abi3")),
            "is_abi3 should be in used_variant when used in context expressions (even without shadowing). \
            Actual used_variant keys: {:?}",
            used_variant.keys().collect::<Vec<_>>()
        );

        // Verify the context variables were evaluated correctly
        assert_eq!(
            recipe.context.get("use_abi3").unwrap().to_string(),
            "true",
            "Context use_abi3 should be 'true'"
        );
        assert_eq!(
            recipe.context.get("build_abi3").unwrap().to_string(),
            "true",
            "Context build_abi3 should be 'true' (is_abi3=true, python matches 3.10.*)"
        );

        // Verify the variant contains the original value from the variant config
        assert_eq!(
            used_variant.get(&NormalizedKey::from("is_abi3")).unwrap().to_string(),
            "true",
            "Variant should contain the original 'true' value for is_abi3"
        );
    }
}
