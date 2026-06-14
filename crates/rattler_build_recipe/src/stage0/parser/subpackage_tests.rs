//! Tests for parsing the `subpackages` section of an output.

use crate::stage0::{
    IncludeExclude, Recipe,
    parser::{parse_recipe_from_source, parse_recipe_or_multi_from_source},
};

fn file_count(files: &IncludeExclude) -> usize {
    match files {
        IncludeExclude::List(list) => list.iter().count(),
        IncludeExclude::Mapping { include, exclude } => {
            include.iter().count() + exclude.iter().count()
        }
    }
}

#[test]
fn test_single_output_with_subpackages() {
    let yaml = r#"
package:
  name: mylib
  version: 1.2.3

build:
  script: make install

requirements:
  run:
    - libstdcxx

subpackages:
  - package:
      name: mylib-dev
    files:
      - include/**
      - lib/**/*.so
    requirements:
      run:
        - ${{ pin_subpackage('mylib', exact=true) }}
    about:
      summary: Development files for mylib
  - package:
      name: mylib-doc
    files:
      - share/man/**
      - share/doc/**
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.subpackages.len(), 2);

    let dev = &recipe.subpackages[0];
    assert!(dev.package.name.is_concrete());
    // version is inherited from the parent when omitted
    assert!(dev.package.version.is_none());
    assert_eq!(file_count(&dev.files), 2);
    assert!(!dev.requirements.run.is_empty());
    assert!(dev.about.summary.is_some());

    let doc = &recipe.subpackages[1];
    assert_eq!(file_count(&doc.files), 2);
    assert!(doc.requirements.is_empty());
}

#[test]
fn test_subpackage_files_include_exclude_mapping() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - package:
      name: mylib-static
    files:
      include:
        - lib/*.a
      exclude:
        - lib/excludeme.a
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    let static_pkg = &recipe.subpackages[0];
    match &static_pkg.files {
        IncludeExclude::Mapping { include, exclude } => {
            assert_eq!(include.iter().count(), 1);
            assert_eq!(exclude.iter().count(), 1);
        }
        other => panic!("expected include/exclude mapping, got {other:?}"),
    }
}

#[test]
fn test_subpackage_in_multi_output() {
    let yaml = r#"
recipe:
  name: mylib
  version: 1.2.3

outputs:
  - package:
      name: mylib
    requirements:
      run:
        - libstdcxx
    subpackages:
      - package:
          name: mylib-dev
        files:
          - include/**
        requirements:
          run:
            - ${{ pin_subpackage('mylib', exact=true) }}
"#;

    let recipe = parse_recipe_or_multi_from_source(yaml).unwrap();
    let Recipe::MultiOutput(multi) = recipe else {
        panic!("expected a multi-output recipe");
    };
    assert_eq!(multi.outputs.len(), 1);
    let crate::stage0::Output::Package(pkg) = &multi.outputs[0] else {
        panic!("expected a package output");
    };
    assert_eq!(pkg.subpackages.len(), 1);
    assert_eq!(file_count(&pkg.subpackages[0].files), 1);
}

#[test]
fn test_subpackage_rejects_build_requirements() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - package:
      name: mylib-dev
    files:
      - include/**
    requirements:
      build:
        - cmake
"#;

    let err = parse_recipe_from_source(yaml).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("build") && msg.contains("host"),
        "unexpected error: {msg}"
    );
}

#[test]
fn test_subpackage_rejects_host_requirements() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - package:
      name: mylib-dev
    files:
      - include/**
    requirements:
      host:
        - libfoo
"#;

    assert!(parse_recipe_from_source(yaml).is_err());
}

#[test]
fn test_subpackage_rejects_unknown_field() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - package:
      name: mylib-dev
    files:
      - include/**
    bogus: true
"#;

    assert!(parse_recipe_from_source(yaml).is_err());
}

#[test]
fn test_subpackage_requires_package_name() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - files:
      - include/**
"#;

    assert!(parse_recipe_from_source(yaml).is_err());
}

#[test]
fn test_subpackage_version_override() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - package:
      name: mylib-dev
      version: 2.0.0
    files:
      - include/**
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert!(recipe.subpackages[0].package.version.is_some());
}

#[test]
fn test_subpackage_used_variables() {
    let yaml = r#"
package:
  name: mylib
  version: 1.0.0

subpackages:
  - package:
      name: mylib-dev
    files:
      - ${{ subdir_var }}/**
    requirements:
      run:
        - ${{ pin_subpackage('mylib') }}
        - if: my_cond
          then: some-dep
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    let used = recipe.used_variables();
    assert!(used.contains(&"subdir_var".to_string()), "got: {used:?}");
    assert!(used.contains(&"my_cond".to_string()), "got: {used:?}");
}
