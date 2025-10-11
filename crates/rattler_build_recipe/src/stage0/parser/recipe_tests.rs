//! Integration tests for complete recipe parsing

use marked_yaml::Node as MarkedNode;

use crate::stage0::parser::{parse_recipe, parse_recipe_from_source};

fn parse_yaml_recipe(yaml_str: &str) -> MarkedNode {
    marked_yaml::parse_yaml(0, yaml_str).expect("Failed to parse test YAML")
}

#[test]
fn test_parse_minimal_recipe() {
    let yaml_str = r#"
package:
  name: my-package
  version: 1.0.0
"#;
    let yaml = parse_yaml_recipe(yaml_str);
    let recipe = parse_recipe(&yaml).unwrap();

    assert!(recipe.package.name.is_concrete());
    assert!(recipe.about.homepage.is_none());
    assert!(recipe.requirements.is_empty());
    assert!(recipe.extra.recipe_maintainers.is_empty());
}

#[test]
fn test_parse_full_recipe() {
    let yaml_str = r#"
package:
  name: my-package
  version: 1.0.0

about:
  homepage: https://example.com
  license: MIT
  summary: A test package

requirements:
  build:
    - gcc
    - make
  run:
    - python

extra:
  recipe-maintainers:
    - alice
    - bob
"#;
    let yaml = parse_yaml_recipe(yaml_str);
    let recipe = parse_recipe(&yaml).unwrap();

    // Verify package
    assert!(recipe.package.name.is_concrete());
    assert!(recipe.package.version.is_concrete());

    // Verify about
    assert!(recipe.about.homepage.is_some());
    assert!(recipe.about.license.is_some());
    assert!(recipe.about.summary.is_some());

    // Verify requirements
    assert_eq!(recipe.requirements.build.len(), 2);
    assert_eq!(recipe.requirements.run.len(), 1);

    // Verify extra
    assert_eq!(recipe.extra.recipe_maintainers.len(), 2);
}

#[test]
fn test_parse_recipe_with_templates() {
    let yaml_str = r#"
package:
  name: '${{ name }}'
  version: '${{ version }}'

about:
  summary: '${{ name }} - version ${{ version }}'

requirements:
  build:
    - '${{ compiler("c") }}'
"#;
    let yaml = parse_yaml_recipe(yaml_str);
    let recipe = parse_recipe(&yaml).unwrap();

    // Check that templates are parsed
    assert!(recipe.package.name.is_template());
    assert!(recipe.package.version.is_template());

    // Check variable extraction
    let vars = recipe.used_variables();
    assert!(vars.contains(&"name".to_string()));
    assert!(vars.contains(&"version".to_string()));
    assert!(vars.contains(&"compiler".to_string()));
}

#[test]
fn test_parse_recipe_with_conditionals() {
    let yaml_str = r#"
package:
  name: my-package
  version: 1.0.0

requirements:
  build:
    - gcc
    - if: win
      then: vs2019
      else: clang
  run:
    - python
"#;
    let yaml = parse_yaml_recipe(yaml_str);
    let recipe = parse_recipe(&yaml).unwrap();

    assert_eq!(recipe.requirements.build.len(), 2);

    let vars = recipe.used_variables();
    assert!(vars.contains(&"win".to_string()));
}

#[test]
fn test_parse_recipe_missing_package() {
    let yaml_str = r#"
about:
  license: MIT
"#;
    let yaml = parse_yaml_recipe(yaml_str);
    let result = parse_recipe(&yaml);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.as_ref().unwrap().contains("missing"));
    assert!(err.message.as_ref().unwrap().contains("package"));
}

#[test]
fn test_parse_recipe_unknown_top_level_field() {
    let yaml_str = r#"
package:
  name: my-package
  version: 1.0.0

unknown_field:
  value: something
"#;
    let yaml = parse_yaml_recipe(yaml_str);
    let result = parse_recipe(&yaml);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.as_ref().unwrap().contains("unknown"));
}

#[test]
fn test_parse_recipe_from_source() {
    let yaml_str = r#"
package:
  name: my-package
  version: 1.0.0

about:
  license: Apache-2.0
"#;
    let recipe = parse_recipe_from_source(yaml_str).unwrap();

    assert!(recipe.package.name.is_concrete());
    assert!(recipe.about.license.is_some());
}

#[test]
fn test_parse_recipe_from_source_with_complex_requirements() {
    let yaml_str = r#"
package:
  name: complex-package
  version: 2.5.1

requirements:
  build:
    - '${{ compiler("c") }}'
    - '${{ compiler("cxx") }}'
  host:
    - python
    - setuptools
  run:
    - python
    - if: linux
      then: libgcc
    - if: osx
      then: libc++
  run_exports:
    strong:
      - complex-package
"#;
    let recipe = parse_recipe_from_source(yaml_str).unwrap();

    assert_eq!(recipe.requirements.build.len(), 2);
    assert_eq!(recipe.requirements.host.len(), 2);
    assert_eq!(recipe.requirements.run.len(), 3);
    assert!(!recipe.requirements.run_exports.is_empty());

    let vars = recipe.used_variables();
    assert!(vars.contains(&"compiler".to_string()));
    assert!(vars.contains(&"linux".to_string()));
    assert!(vars.contains(&"osx".to_string()));
}
