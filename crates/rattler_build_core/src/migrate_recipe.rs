//! Migration from deprecated `cache:` format to `staging:` outputs.
//!
//! This module provides functionality to detect and migrate recipes that use the
//! old `cache:` top-level key to the new `staging:` output format.

use std::path::Path;

use fs_err as fs;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

/// Errors that can occur during recipe migration.
#[derive(Debug, Error)]
pub enum MigrateRecipeError {
    /// I/O error reading/writing recipe file
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to parse YAML
    #[error("Failed to parse YAML: {0}")]
    YamlParse(String),

    /// Recipe does not contain a `cache:` key
    #[error("Recipe does not contain a top-level 'cache:' key — nothing to migrate")]
    NoCacheKey,

    /// Failed to serialize YAML
    #[error("Failed to serialize YAML: {0}")]
    SerializeError(String),
}

/// Check whether raw YAML content contains a top-level `cache:` key.
///
/// Uses `serde_yaml::Value` for a lightweight structural check (not just a
/// regex), so it won't false-positive on comments or nested keys.
pub fn has_cache_key(content: &str) -> bool {
    serde_yaml::from_str::<YamlValue>(content)
        .ok()
        .and_then(|v| v.as_mapping().cloned())
        .is_some_and(|m| m.contains_key(YamlValue::String("cache".to_string())))
}

/// Migrate a recipe file from `cache:` format to `staging:` outputs.
///
/// If `dry_run` is true, prints the migrated content to stdout without writing.
/// Returns the migrated content.
pub fn migrate_recipe(path: &Path, dry_run: bool) -> Result<String, MigrateRecipeError> {
    let content = fs::read_to_string(path)?;
    let migrated = migrate_cache_to_staging(&content)?;

    if dry_run {
        println!("{migrated}");
    } else {
        fs::write(path, &migrated)?;
        tracing::info!("Migrated recipe: {}", path.display());
    }

    Ok(migrated)
}

/// Core migration: transform recipe text from `cache:` to `staging:` format.
pub fn migrate_cache_to_staging(content: &str) -> Result<String, MigrateRecipeError> {
    let yaml: YamlValue =
        serde_yaml::from_str(content).map_err(|e| MigrateRecipeError::YamlParse(e.to_string()))?;

    let mapping = yaml
        .as_mapping()
        .ok_or_else(|| MigrateRecipeError::YamlParse("expected top-level mapping".to_string()))?;

    let cache = mapping
        .get(YamlValue::String("cache".to_string()))
        .ok_or(MigrateRecipeError::NoCacheKey)?;

    let staging_name = derive_staging_name(&yaml);
    let staging_text = build_staging_text(&staging_name, cache);

    // Remove the cache section from the text
    let mut content = remove_cache_section(content);
    content = insert_staging_output(&content, &staging_text);
    content = insert_inherit(&content, &staging_name);

    Ok(content)
}

/// Derive a staging output name from the recipe.
///
/// Uses `recipe.name` + `-build` if available, otherwise falls back to
/// `"build-cache"`.
fn derive_staging_name(yaml: &YamlValue) -> String {
    yaml.get("recipe")
        .and_then(|r| r.get("name"))
        .and_then(|n| n.as_str())
        .map(|name| format!("{name}-build"))
        .unwrap_or_else(|| "build-cache".to_string())
}

/// Find the line range of a top-level YAML section (key at column 0).
///
/// Returns `Some((start_line, end_line))` where the range is exclusive on end.
/// `start_line` is the line containing `key:`, and `end_line` is the first line
/// of the *next* top-level section (or the total line count).
fn find_section_range(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let prefix = format!("{key}:");
    let start = lines.iter().position(|line| {
        let trimmed = line.trim_start();
        // Must be at column 0 (no leading whitespace) and start with `key:`
        trimmed.len() == line.len()
            && (trimmed == prefix || trimmed.starts_with(&format!("{key}: ")))
    })?;

    // Find the end: next non-blank, non-comment line at column 0
    let mut end = start + 1;
    while end < lines.len() {
        let line = lines[end];
        if line.is_empty() || line.chars().all(|c| c.is_whitespace()) {
            end += 1;
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            // Comment line — could belong to cache or next section.
            // Check if the *next* non-blank, non-comment line is indented.
            // If it is, this comment belongs to the cache section.
            let mut lookahead = end + 1;
            while lookahead < lines.len() {
                let la = lines[lookahead];
                if la.is_empty()
                    || la.chars().all(|c| c.is_whitespace())
                    || la.trim_start().starts_with('#')
                {
                    lookahead += 1;
                    continue;
                }
                break;
            }
            if lookahead < lines.len() && lines[lookahead].starts_with(|c: char| c.is_whitespace())
            {
                // Next content line is indented → comment belongs to cache section
                end += 1;
                continue;
            }
            // Comment is at top level → belongs to next section
            break;
        }
        // Non-blank, non-comment line
        if line.starts_with(|c: char| c.is_whitespace()) {
            // Indented → still part of the current section
            end += 1;
        } else {
            // At column 0 → next top-level section
            break;
        }
    }

    Some((start, end))
}

/// Remove the `cache:` section from the recipe text.
fn remove_cache_section(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = find_section_range(&lines, "cache") else {
        return content.to_string();
    };

    let mut result_lines: Vec<&str> = Vec::new();
    result_lines.extend_from_slice(&lines[..start]);
    // Skip trailing blank lines after the removed section
    let mut rest_start = end;
    while rest_start < lines.len()
        && (lines[rest_start].is_empty() || lines[rest_start].chars().all(|c| c.is_whitespace()))
    {
        rest_start += 1;
    }
    result_lines.extend_from_slice(&lines[rest_start..]);

    let mut result = result_lines.join("\n");
    // Preserve trailing newline if original had one
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Build the YAML text for a staging output from parsed cache data.
fn build_staging_text(name: &str, cache: &YamlValue) -> String {
    let mut lines = Vec::new();
    lines.push("  - staging:".to_string());
    lines.push(format!("      name: {name}"));

    let Some(cache_map) = cache.as_mapping() else {
        return lines.join("\n");
    };

    // Process `build:` section
    if let Some(build) = cache_map.get(YamlValue::String("build".to_string())) {
        lines.push("    build:".to_string());
        append_yaml_indented(&mut lines, build, 6);
    }

    // Process `requirements:` section
    if let Some(reqs) = cache_map.get(YamlValue::String("requirements".to_string())) {
        lines.push("    requirements:".to_string());
        append_yaml_indented(&mut lines, reqs, 6);
    }

    // Process `source:` section
    if let Some(source) = cache_map.get(YamlValue::String("source".to_string())) {
        lines.push("    source:".to_string());
        append_yaml_indented(&mut lines, source, 6);
    }

    lines.join("\n")
}

/// Append a YAML value as indented text lines.
fn append_yaml_indented(lines: &mut Vec<String>, value: &YamlValue, base_indent: usize) {
    let yaml_str = serde_yaml::to_string(value)
        .unwrap_or_default()
        .trim_end()
        .to_string();

    let indent = " ".repeat(base_indent);
    for line in yaml_str.lines() {
        lines.push(format!("{indent}{line}"));
    }
}

/// Insert the staging output text as the first item under `outputs:`.
fn insert_staging_output(content: &str, staging_text: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();

    // Find the `outputs:` line
    let Some(outputs_line_idx) = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed == "outputs:" || trimmed.starts_with("outputs: ")
    }) else {
        return content.to_string();
    };

    // Find the first `- ` item under outputs
    let mut first_item_idx = None;
    for (i, line) in lines.iter().enumerate().skip(outputs_line_idx + 1) {
        if line.is_empty() || line.chars().all(|c| c.is_whitespace()) {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("- ") {
            first_item_idx = Some(i);
            break;
        }
        // Non-list content under outputs — unexpected
        break;
    }

    let mut result_lines: Vec<String> = lines[..=outputs_line_idx]
        .iter()
        .map(|s| s.to_string())
        .collect();

    result_lines.push(staging_text.to_string());
    // Add blank line between staging and first package output
    result_lines.push(String::new());

    if let Some(idx) = first_item_idx {
        for line in &lines[idx..] {
            result_lines.push(line.to_string());
        }
    }

    let mut result = result_lines.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Add `inherit: <staging_name>` to each `- package:` output that doesn't
/// already have one.
fn insert_inherit(content: &str, staging_name: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        result_lines.push(line.to_string());

        // Detect `  - package:` lines
        let trimmed = line.trim_start();
        let leading_spaces = line.len() - trimmed.len();

        if trimmed.starts_with("- package:") && leading_spaces > 0 {
            let item_indent = leading_spaces;
            let content_indent = item_indent + 2;

            // Scan the entire output block to check for existing `inherit:`
            let mut has_inherit = false;
            let mut j = i + 1;
            while j < lines.len() {
                let next_line = lines[j];
                let next_trimmed = next_line.trim_start();
                let next_indent = next_line.len() - next_trimmed.len();

                if next_line.is_empty() || next_trimmed.is_empty() {
                    j += 1;
                    continue;
                }
                if next_trimmed.starts_with("- ") && next_indent <= item_indent {
                    break;
                }
                if next_indent == 0 && !next_trimmed.starts_with('#') {
                    break;
                }
                if next_indent == content_indent && next_trimmed.starts_with("inherit:") {
                    has_inherit = true;
                }
                j += 1;
            }

            if !has_inherit {
                let inherit_line = format!("{}inherit: {staging_name}", " ".repeat(content_indent));

                // Find the end of the `package:` sub-block (lines deeper than
                // content_indent). Stop at blank lines or lines at
                // content_indent or less.
                let mut pkg_end = i + 1;
                while pkg_end < lines.len() {
                    let pl = lines[pkg_end];
                    let pt = pl.trim_start();
                    let pindent = pl.len() - pt.len();

                    if pl.is_empty() || pt.is_empty() {
                        break;
                    }
                    if pindent > content_indent {
                        pkg_end += 1;
                        continue;
                    }
                    break;
                }

                // Push package sub-block lines, then inherit
                for line in lines.iter().take(pkg_end).skip(i + 1) {
                    result_lines.push(line.to_string());
                }
                result_lines.push(inherit_line);
                i = pkg_end;
                continue;
            }
        }

        i += 1;
    }

    let mut result = result_lines.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_cache_key_positive() {
        let content = "\
cache:
  build:
    script:
      - echo hello
outputs:
  - package:
      name: foo
";
        assert!(has_cache_key(content));
    }

    #[test]
    fn test_has_cache_key_negative() {
        let content = "\
outputs:
  - package:
      name: foo
";
        assert!(!has_cache_key(content));
    }

    #[test]
    fn test_has_cache_key_nested_not_toplevel() {
        let content = "\
something:
  cache:
    build: true
";
        assert!(!has_cache_key(content));
    }

    #[test]
    fn test_migrate_no_cache_key_errors() {
        let content = "\
outputs:
  - package:
      name: foo
";
        let err = migrate_cache_to_staging(content).unwrap_err();
        assert!(matches!(err, MigrateRecipeError::NoCacheKey));
    }

    #[test]
    fn test_migrate_basic_cache() {
        let content = "\
context:
  version: 0.1.0
  build_num: 0

recipe:
  name: cache-installation
  version: ${{ version }}

build:
  number: ${{ build_num }}

cache:
  requirements:
    build:
      - cmake
  build:
    script:
      - cmake --version

outputs:
  - package:
      name: check-1

  - package:
      name: check-2
";
        let result = migrate_cache_to_staging(content).unwrap();
        insta::assert_snapshot!(result, @r###"
        context:
          version: 0.1.0
          build_num: 0

        recipe:
          name: cache-installation
          version: ${{ version }}

        build:
          number: ${{ build_num }}

        outputs:
          - staging:
              name: cache-installation-build
            build:
              script:
              - cmake --version
            requirements:
              build:
              - cmake

          - package:
              name: check-1
            inherit: cache-installation-build

          - package:
              name: check-2
            inherit: cache-installation-build
        "###);
    }

    #[test]
    fn test_migrate_cache_with_compiler() {
        let content = "\
# test for issue https://github.com/prefix-dev/rattler-build/issues/1290
recipe:
  name: foo
  version: 0.1.0

cache:
  build:
    script:
      - mkdir -p $PREFIX/lib
      - mkdir -p $PREFIX/include
      - touch $PREFIX/include/test.h
      - touch $PREFIX/lib/libdav1d.so.7.0.0
      - ln -s $PREFIX/lib/libdav1d.so.7.0.0 $PREFIX/lib/libdav1d.so.7
      - ln -s $PREFIX/lib/libdav1d.so.7 $PREFIX/lib/libdav1d.so

  requirements:
    build:
      - ${{ compiler('c') }}

outputs:
  - package:
      name: testlib-so-version
      version: 2.1.4

    build:
      files:
        include:
          - lib/*.so.*
";
        let result = migrate_cache_to_staging(content).unwrap();
        insta::assert_snapshot!(result, @r###"
        # test for issue https://github.com/prefix-dev/rattler-build/issues/1290
        recipe:
          name: foo
          version: 0.1.0

        outputs:
          - staging:
              name: foo-build
            build:
              script:
              - mkdir -p $PREFIX/lib
              - mkdir -p $PREFIX/include
              - touch $PREFIX/include/test.h
              - touch $PREFIX/lib/libdav1d.so.7.0.0
              - ln -s $PREFIX/lib/libdav1d.so.7.0.0 $PREFIX/lib/libdav1d.so.7
              - ln -s $PREFIX/lib/libdav1d.so.7 $PREFIX/lib/libdav1d.so
            requirements:
              build:
              - ${{ compiler('c') }}

          - package:
              name: testlib-so-version
              version: 2.1.4
            inherit: foo-build

            build:
              files:
                include:
                  - lib/*.so.*
        "###);
    }

    #[test]
    fn test_migrate_cache_without_recipe_name() {
        let content = "\
cache:
  build:
    script:
      - echo hello

outputs:
  - package:
      name: my-pkg
";
        let result = migrate_cache_to_staging(content).unwrap();
        insta::assert_snapshot!(result, @r###"
        outputs:
          - staging:
              name: build-cache
            build:
              script:
              - echo hello

          - package:
              name: my-pkg
            inherit: build-cache
        "###);
    }

    #[test]
    fn test_migrate_cache_with_source() {
        let content = "\
recipe:
  name: my-lib
  version: 1.0.0

cache:
  source:
    url: https://example.com/source.tar.gz
    sha256: abc123
  build:
    script:
      - make install

outputs:
  - package:
      name: my-lib-bin
";
        let result = migrate_cache_to_staging(content).unwrap();
        insta::assert_snapshot!(result, @r###"
        recipe:
          name: my-lib
          version: 1.0.0

        outputs:
          - staging:
              name: my-lib-build
            build:
              script:
              - make install
            source:
              url: https://example.com/source.tar.gz
              sha256: abc123

          - package:
              name: my-lib-bin
            inherit: my-lib-build
        "###);
    }

    #[test]
    fn test_migrate_cache_symlinks() {
        let content = "\
recipe:
  name: cache-symlinks
  version: 1.0.0

cache:
  build:
    script: |
      mkdir -p $PREFIX/bin
      touch $PREFIX/bin/exe
      ln -s $PREFIX/bin/exe $PREFIX/bin/exe-symlink
      ln -s $PREFIX/bin/exe $PREFIX/bin/absolute-exe-symlink
      touch $PREFIX/foo.txt
      ln -s $PREFIX/foo.txt $PREFIX/foo-symlink.txt
      ln -s $PREFIX/foo.txt $PREFIX/absolute-symlink.txt
      ln -s $PREFIX/non-existent-file $PREFIX/broken-symlink.txt
      ln -s ./foo.txt $PREFIX/relative-symlink.txt
      echo ${{ PREFIX }} > $PREFIX/prefix.txt

outputs:
  - package:
      name: cache-symlinks
    build:
      files:
        include:
          - \"**/*\"
        exclude:
          - \"absolute-symlink.txt\"
          - \"bin/absolute-exe-symlink\"
  - package:
      name: absolute-cache-symlinks
    build:
      files:
        - \"absolute-symlink.txt\"
        - \"bin/absolute-exe-symlink\"
";
        let result = migrate_cache_to_staging(content).unwrap();
        insta::assert_snapshot!(result, @r###"
        recipe:
          name: cache-symlinks
          version: 1.0.0

        outputs:
          - staging:
              name: cache-symlinks-build
            build:
              script: |
                mkdir -p $PREFIX/bin
                touch $PREFIX/bin/exe
                ln -s $PREFIX/bin/exe $PREFIX/bin/exe-symlink
                ln -s $PREFIX/bin/exe $PREFIX/bin/absolute-exe-symlink
                touch $PREFIX/foo.txt
                ln -s $PREFIX/foo.txt $PREFIX/foo-symlink.txt
                ln -s $PREFIX/foo.txt $PREFIX/absolute-symlink.txt
                ln -s $PREFIX/non-existent-file $PREFIX/broken-symlink.txt
                ln -s ./foo.txt $PREFIX/relative-symlink.txt
                echo ${{ PREFIX }} > $PREFIX/prefix.txt

          - package:
              name: cache-symlinks
            inherit: cache-symlinks-build
            build:
              files:
                include:
                  - "**/*"
                exclude:
                  - "absolute-symlink.txt"
                  - "bin/absolute-exe-symlink"
          - package:
              name: absolute-cache-symlinks
            inherit: cache-symlinks-build
            build:
              files:
                - "absolute-symlink.txt"
                - "bin/absolute-exe-symlink"
        "###);
    }

    #[test]
    fn test_comments_outside_cache_preserved() {
        let content = "\
# This is a top-level comment
recipe:
  name: foo
  version: 1.0.0

cache:
  build:
    script:
      - echo hello

# This comment should be preserved
outputs:
  - package:
      name: bar
";
        let result = migrate_cache_to_staging(content).unwrap();
        assert!(result.contains("# This is a top-level comment"));
        assert!(result.contains("# This comment should be preserved"));
    }

    #[test]
    fn test_existing_inherit_not_duplicated() {
        let content = "\
recipe:
  name: foo
  version: 1.0.0

cache:
  build:
    script:
      - echo hello

outputs:
  - package:
      name: bar
    inherit: something-else
";
        let result = migrate_cache_to_staging(content).unwrap();
        // Should not add a second inherit
        assert_eq!(
            result.matches("inherit:").count(),
            1,
            "should not duplicate inherit"
        );
    }
}
