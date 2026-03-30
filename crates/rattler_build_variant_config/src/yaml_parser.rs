//! YAML parser for variant configuration using the shared rattler_build_yaml_parser
//!
//! This parser uses the shared parser with Variable type specialization.

use crate::error::VariantConfigError;
use crate::stage0_types::ConditionalList;
use crate::variable_converter::VariableConverter;
use marked_yaml::{Node, Span};
use rattler_build_types::NormalizedKey;
use rattler_build_yaml_parser::{ParseError, ParseNode, ParseResult, parse_yaml};
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

/// Stage0 variant configuration - contains templates and conditionals before evaluation
#[derive(Debug, Clone, Default)]
pub struct Stage0VariantConfig {
    /// Keys that should be "zipped" together when creating the build matrix
    pub zip_keys: Option<Vec<Vec<NormalizedKey>>>,

    /// The variant values - a mapping of keys to lists with conditionals and templates
    pub variants: BTreeMap<NormalizedKey, ConditionalList>,

    /// The path to the variant config file (for error reporting)
    pub path: Option<PathBuf>,
}

/// Check if content contains legacy `# [selector]` syntax and warn about it
#[cfg(not(target_arch = "wasm32"))]
fn warn_about_legacy_selectors(content: &str, path: &Path) {
    // Look for patterns like `# [win]`, `# [unix]`, `# [osx]`, `# [linux]` etc.
    let selector_pattern = regex::Regex::new(r"#\s*\[[\w\s]+\]").unwrap();
    if selector_pattern.is_match(content) {
        tracing::warn!(
            "Variant file '{}' appears to contain legacy `# [selector]` syntax (e.g. `# [win]`). \
             This syntax is only supported in files named 'conda_build_config.yaml'. \
             For other variant files, use the `if/then/else` syntax instead:\n\
             \n\
             # Old style (only works in conda_build_config.yaml):\n\
             python:\n\
             \x20 - 3.10  # [unix]\n\
             \n\
             # New style (recommended):\n\
             python:\n\
             \x20 - if: unix\n\
             \x20   then: \"3.10\"",
            path.display()
        );
    }
}

/// Parse a variant configuration file using marked_yaml
#[cfg(not(target_arch = "wasm32"))]
pub fn parse_variant_file(path: &Path) -> Result<Stage0VariantConfig, VariantConfigError> {
    let content = fs_err::read_to_string(path)
        .map_err(|e| VariantConfigError::IoError(path.to_path_buf(), e))?;

    // Check for legacy selector syntax and warn
    warn_about_legacy_selectors(&content, path);

    let mut config = parse_variant_str(&content, Some(path.to_path_buf()))?;
    config.path = Some(path.to_path_buf());
    Ok(config)
}

/// Parse a variant configuration string using marked_yaml
pub fn parse_variant_str(
    yaml: &str,
    path: Option<PathBuf>,
) -> Result<Stage0VariantConfig, VariantConfigError> {
    let path_buf = path.unwrap_or_default();

    let node = parse_yaml(yaml)
        .map_err(|e| ParseError::generic(e.to_string(), Span::new_blank()))
        .map_err(|source| VariantConfigError::ParseError {
            path: path_buf.clone(),
            source,
        })?;

    parse_node(&node).map_err(|source| VariantConfigError::ParseError {
        path: path_buf,
        source,
    })
}

/// Parse a marked_yaml Node into a Stage0VariantConfig
fn parse_node(node: &Node) -> ParseResult<Stage0VariantConfig> {
    let mapping = node
        .as_mapping()
        .ok_or_else(|| ParseError::expected_type("mapping", "other", *node.span()))?;

    let mut zip_keys = None;
    let mut variants = BTreeMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key_str = key_node.as_str();

        if key_str == "zip_keys" {
            zip_keys = Some(parse_zip_keys(value_node)?);
            continue;
        }

        if key_str == "pin_run_as_build" {
            tracing::warn!(
                "Found 'pin_run_as_build' in variant config - this is not supported and will be ignored"
            );
            continue;
        }

        // Parse variant values (which may contain conditionals or templates)
        let values = parse_variant_values(value_node, key_str)?;
        if !values.is_empty() {
            variants.insert(key_str.into(), values);
        }
    }

    Ok(Stage0VariantConfig {
        zip_keys,
        variants,
        path: None,
    })
}

/// Parse zip_keys from a marked_yaml Node
fn parse_zip_keys(node: &Node) -> ParseResult<Vec<Vec<NormalizedKey>>> {
    let sequences = node
        .as_sequence()
        .ok_or_else(|| ParseError::expected_type("list", "other", *node.span()))?;

    let mut result = Vec::new();
    for seq_node in sequences.iter() {
        let keys: Vec<String> = seq_node.parse_sequence("zip_keys")?;
        result.push(keys.into_iter().map(NormalizedKey::from).collect());
    }

    Ok(result)
}

/// Parse variant values from a marked_yaml Node, handling conditionals and templates
fn parse_variant_values(node: &Node, key: &str) -> ParseResult<ConditionalList> {
    // Provide a helpful error message if not a sequence
    if node.as_sequence().is_none() {
        return Err(ParseError::generic(
            format!("Variant values for '{}' must be a list", key),
            *node.span(),
        ));
    }

    node.parse_conditional_list_with(&VariableConverter::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage0_types::Item;

    #[test]
    fn test_simple_parsing() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
"#;
        let config = parse_variant_str(yaml, None).unwrap();
        assert_eq!(config.variants.len(), 2);
        assert_eq!(config.variants.get(&"python".into()).unwrap().len(), 2);
    }

    #[test]
    fn test_conditional() {
        let yaml = r#"
vc:
  - if: win
    then: "14"
  - if: unix
    then: "16"
"#;
        let config = parse_variant_str(yaml, None).unwrap();
        assert_eq!(config.variants.len(), 1);

        let vc_items = &config.variants.get(&"vc".into()).unwrap();
        assert_eq!(vc_items.len(), 2);

        // Check first conditional
        match &vc_items.into_iter().next().unwrap() {
            Item::Conditional(cond) => {
                assert_eq!(cond.condition.source(), "win");
                assert_eq!(cond.then.len(), 1);
            }
            _ => panic!("Expected conditional"),
        }
    }

    #[test]
    fn test_conditional_with_list() {
        let yaml = r#"
python:
  - if: unix
    then: ["3.9", "3.10"]
  - if: win
    then: ["3.8"]
"#;
        let config = parse_variant_str(yaml, None).unwrap();

        let python_items = &config.variants.get(&"python".into()).unwrap();
        assert_eq!(python_items.len(), 2);
    }

    #[test]
    fn test_template_value() {
        let yaml = r#"
target:
  - ${{ target_platform }}
"#;
        let config = parse_variant_str(yaml, None).unwrap();

        let target_items = &config.variants.get(&"target".into()).unwrap();
        assert_eq!(target_items.len(), 1);

        match target_items.into_iter().next().unwrap() {
            Item::Value(value) => {
                assert!(value.is_template());
            }
            _ => panic!("Expected value"),
        }
    }

    #[test]
    fn test_zip_keys() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
zip_keys:
  - [python, numpy]
"#;
        let config = parse_variant_str(yaml, None).unwrap();
        assert!(config.zip_keys.is_some());
        assert_eq!(config.zip_keys.unwrap().len(), 1);
    }

    #[test]
    fn test_mixed_values() {
        let yaml = r#"
mixed:
  - "plain_string"
  - ${{ template_var }}
  - if: win
    then: "conditional"
"#;
        let config = parse_variant_str(yaml, None).unwrap();

        let mixed_items = &config.variants.get(&"mixed".into()).unwrap();
        assert_eq!(mixed_items.len(), 3);
    }
}
