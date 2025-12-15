use rattler_build_recipe::stage0::{
    IncludeExclude, Item, PythonVersion, Source, TestType, parse_recipe_from_source,
};

/// Helper to extract a concrete Source from a ConditionalList item
fn get_concrete_source(item: &Item<Source>) -> Option<&Source> {
    item.as_value()?.as_concrete()
}

/// Helper to check if an item is a conditional
fn is_conditional_source(item: &Item<Source>) -> bool {
    item.is_conditional()
}

/// Helper to check if a test item is a conditional
fn is_conditional_test(item: &Item<TestType>) -> bool {
    item.is_conditional()
}

#[test]
fn test_parse_recipe_with_git_source() {
    let yaml = r#"
package:
  name: test
  version: 1.0.0

source:
  git: https://github.com/example/repo.git
  tag: v1.0.0
  patches:
    - fix.patch

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 1);

    let source = get_concrete_source(&recipe.source.as_slice()[0]).unwrap();
    match source {
        Source::Git(git) => {
            // Check that the git URL is correctly parsed
            if let Some(url) = git.url.0.as_concrete() {
                assert_eq!(url, "https://github.com/example/repo.git");
            } else {
                panic!("Expected concrete URL");
            }

            // Check tag
            assert!(git.tag.is_some());

            // Check patches
            assert_eq!(git.patches.len(), 1);
        }
        _ => panic!("Expected Git source"),
    }
}

#[test]
fn test_parse_recipe_with_url_source() {
    let yaml = r#"
package:
  name: test
  version: 1.0.0

source:
  url: https://example.com/archive.tar.gz
  sha256: e03c8123866dd68f129e8a29082011db418ce90863948f563c01b814670782c6
  file_name: archive.tar.gz

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 1);

    let source = get_concrete_source(&recipe.source.as_slice()[0]).unwrap();
    match source {
        Source::Url(url_src) => {
            assert_eq!(url_src.url.len(), 1);

            if let Some(url) = url_src.url[0].as_concrete() {
                assert_eq!(url, "https://example.com/archive.tar.gz");
            } else {
                panic!("Expected concrete URL");
            }

            assert!(url_src.sha256.is_some());
            assert!(url_src.file_name.is_some());
        }
        _ => panic!("Expected URL source"),
    }
}

#[test]
fn test_parse_recipe_with_path_source() {
    let yaml = r#"
package:
  name: test
  version: 1.0.0

source:
  path: ./local/source
  use_gitignore: true
  filter:
    - "*.txt"
    - "src/"

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 1);

    let source = get_concrete_source(&recipe.source.as_slice()[0]).unwrap();
    match source {
        Source::Path(path_src) => {
            if let Some(path) = path_src.path.as_concrete() {
                assert_eq!(path.to_str().unwrap(), "./local/source");
            } else {
                panic!("Expected concrete path");
            }

            assert!(path_src.use_gitignore);

            // Check filter - it should be a List variant with 2 items
            match &path_src.filter {
                IncludeExclude::List(list) => {
                    assert_eq!(list.len(), 2);
                }
                _ => panic!("Expected List variant for filter"),
            }
        }
        _ => panic!("Expected Path source"),
    }
}

#[test]
fn test_parse_recipe_with_multiple_sources() {
    let yaml = r#"
package:
  name: test
  version: 1.0.0

source:
  - git: https://github.com/example/repo.git
    tag: v1.0.0
  - url: https://example.com/archive.tar.gz
    sha256: e03c8123866dd68f129e8a29082011db418ce90863948f563c01b814670782c6
  - path: ./local/source

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 3);

    // Check we have one of each type
    let sources: Vec<_> = recipe
        .source
        .as_slice()
        .iter()
        .filter_map(|item| get_concrete_source(item))
        .collect();
    assert!(matches!(sources[0], Source::Git(_)));
    assert!(matches!(sources[1], Source::Url(_)));
    assert!(matches!(sources[2], Source::Path(_)));
}

#[test]
fn test_parse_recipe_with_template_source() {
    let yaml = r#"
package:
  name: test
  version: ${{ version }}

source:
  git: ${{ repo_url }}
  tag: v${{ version }}

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 1);

    let source = get_concrete_source(&recipe.source.as_slice()[0]).unwrap();
    match source {
        Source::Git(git) => {
            // Check that the URL is a template
            if let Some(template) = git.url.0.as_template() {
                assert!(template.source().contains("repo_url"));
            } else {
                panic!("Expected template URL");
            }

            // Check tag is template
            assert!(git.tag.is_some());
        }
        _ => panic!("Expected Git source"),
    }
}

#[test]
fn test_parse_conditional_source_yaml() {
    // Load the test data file
    let yaml = include_str!("../test-data/conditional_source.yaml");

    let recipe = parse_recipe_from_source(yaml).unwrap();

    // Check package name
    assert_eq!(
        recipe.package.name.as_concrete().unwrap().0.as_normalized(),
        "conditional-source"
    );

    // We should have 3 source items: 1 unconditional URL + 2 conditional (win and linux)
    assert_eq!(recipe.source.len(), 3);

    let sources = recipe.source.as_slice();

    // First source should be unconditional (the additionalAttributions.txt URL)
    assert!(
        !is_conditional_source(&sources[0]),
        "First source should be unconditional"
    );
    let first_source = get_concrete_source(&sources[0]).unwrap();
    match first_source {
        Source::Url(url_src) => {
            // Check it's a template URL containing "additionalAttributions"
            let url = &url_src.url[0];
            if let Some(template) = url.as_template() {
                assert!(
                    template.source().contains("additionalAttributions"),
                    "URL should contain additionalAttributions"
                );
            } else {
                panic!("Expected template URL");
            }
        }
        _ => panic!("Expected URL source"),
    }

    // Second source should be conditional (if: win)
    assert!(
        is_conditional_source(&sources[1]),
        "Second source should be conditional"
    );
    let cond1 = sources[1].as_conditional().unwrap();
    assert!(
        cond1.condition.source().contains("win"),
        "Condition should check for win"
    );

    // Third source should be conditional (if: linux and x86_64)
    assert!(
        is_conditional_source(&sources[2]),
        "Third source should be conditional"
    );
    let cond2 = sources[2].as_conditional().unwrap();
    assert!(
        cond2.condition.source().contains("linux"),
        "Condition should check for linux"
    );
}

#[test]
fn test_parse_conditional_tests_yaml() {
    // Load the test data file
    let yaml = include_str!("../test-data/conditional_tests.yaml");

    let recipe = parse_recipe_from_source(yaml).unwrap();

    // Check package name
    assert_eq!(
        recipe.package.name.as_concrete().unwrap().0.as_normalized(),
        "conditional-test"
    );

    // We should have 2 test items: 1 conditional (if: unix) + 1 unconditional (package_contents)
    assert_eq!(recipe.tests.len(), 3);

    let tests = recipe.tests.as_slice();

    // First test should be conditional (if: unix)
    assert!(
        is_conditional_test(&tests[0]),
        "First test should be conditional"
    );
    let cond = tests[0].as_conditional().unwrap();
    assert!(
        cond.condition.source().contains("unix"),
        "Condition should check for unix"
    );

    // Second test should be unconditional (package_contents)
    assert!(
        !is_conditional_test(&tests[1]),
        "Second test should be unconditional"
    );
    let test = tests[1].as_value().unwrap().as_concrete().unwrap();
    match test {
        TestType::PackageContents { package_contents } => {
            // Check it has lib entries
            assert!(
                package_contents.lib.is_some(),
                "package_contents should have lib entries"
            );
        }
        _ => panic!("Expected PackageContents test"),
    }
    let test = tests[2].as_value().unwrap().as_concrete().unwrap();
    match test {
        TestType::Python { python } => {
            // Check it has python_version entries
            match python.python_version.as_ref().unwrap() {
                PythonVersion::Multiple(versions) => {
                    assert_eq!(versions.len(), 2);
                }
                _ => panic!("Expected Multiple python_version"),
            }
        }
        _ => panic!("Expected Python test"),
    }
}
