use rattler_build_recipe::stage0::{Source, Value, parse_recipe_from_source};

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

    match &recipe.source[0] {
        Source::Git(git) => {
            // Check that the git URL is correctly parsed
            match &git.url.0 {
                Value::Concrete(url) => {
                    assert_eq!(url, "https://github.com/example/repo.git");
                }
                _ => panic!("Expected concrete URL"),
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
  sha256: abc123def456
  file_name: archive.tar.gz

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 1);

    match &recipe.source[0] {
        Source::Url(url_src) => {
            assert_eq!(url_src.url.len(), 1);

            match &url_src.url[0] {
                Value::Concrete(url) => {
                    assert_eq!(url, "https://example.com/archive.tar.gz");
                }
                _ => panic!("Expected concrete URL"),
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

    match &recipe.source[0] {
        Source::Path(path_src) => {
            match &path_src.path {
                Value::Concrete(path) => {
                    assert_eq!(path.to_str().unwrap(), "./local/source");
                }
                _ => panic!("Expected concrete path"),
            }

            assert!(path_src.use_gitignore);
            assert_eq!(path_src.filter.len(), 2);
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
    sha256: abc123
  - path: ./local/source

build:
  number: 0
"#;

    let recipe = parse_recipe_from_source(yaml).unwrap();
    assert_eq!(recipe.source.len(), 3);

    // Check we have one of each type
    assert!(matches!(recipe.source[0], Source::Git(_)));
    assert!(matches!(recipe.source[1], Source::Url(_)));
    assert!(matches!(recipe.source[2], Source::Path(_)));
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

    match &recipe.source[0] {
        Source::Git(git) => {
            // Check that the URL is a template
            match &git.url.0 {
                Value::Template(template) => {
                    assert!(template.source().contains("repo_url"));
                }
                _ => panic!("Expected template URL"),
            }

            // Check tag is template
            assert!(git.tag.is_some());
        }
        _ => panic!("Expected Git source"),
    }
}
