//! YAML parser for variant configuration using the shared rattler_build_yaml_parser
//!
//! This parser uses the shared parser with Variable type specialization.

use crate::error::VariantConfigError;
use crate::stage0_types::{Conditional, ConditionalList, Item, ListOrItem, Value};
use marked_yaml::{Node, Span};
use rattler_build_jinja::{JinjaExpression, JinjaTemplate, Variable};
use rattler_build_types::NormalizedKey;
use rattler_build_yaml_parser::ParseError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// Parse a variant configuration file using marked_yaml
pub fn parse_variant_file(path: &Path) -> Result<Stage0VariantConfig, VariantConfigError> {
    let content = fs_err::read_to_string(path)
        .map_err(|e| VariantConfigError::IoError(path.to_path_buf(), e))?;

    parse_variant_str(&content, Some(path.to_path_buf()))
}

/// Parse a variant configuration string using marked_yaml
pub fn parse_variant_str(
    yaml: &str,
    path: Option<PathBuf>,
) -> Result<Stage0VariantConfig, VariantConfigError> {
    let node = marked_yaml::parse_yaml(0, yaml).map_err(|e| VariantConfigError::ParseError {
        path: path.clone().unwrap_or_default(),
        source: ParseError::generic(e.to_string(), Span::new_blank()),
    })?;

    parse_node(&node, path)
}

/// Parse a marked_yaml Node into a Stage0VariantConfig
fn parse_node(
    node: &Node,
    path: Option<PathBuf>,
) -> Result<Stage0VariantConfig, VariantConfigError> {
    let mapping = node
        .as_mapping()
        .ok_or_else(|| VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::expected_type("mapping", "other", *node.span()),
        })?;

    let mut zip_keys = None;
    let mut variants = BTreeMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key_str = key_node.as_str();

        if key_str == "zip_keys" {
            zip_keys = Some(parse_zip_keys(value_node, &path)?);
            continue;
        }

        if key_str == "pin_run_as_build" {
            tracing::warn!(
                "Found 'pin_run_as_build' in variant config - this is not supported and will be ignored"
            );
            continue;
        }

        // Parse variant values (which may contain conditionals or templates)
        let values = parse_variant_values(value_node, key_str, &path)?;
        if !values.is_empty() {
            variants.insert(key_str.into(), values);
        }
    }

    Ok(Stage0VariantConfig {
        zip_keys,
        variants,
        path,
    })
}

/// Parse zip_keys from a marked_yaml Node
fn parse_zip_keys(
    node: &Node,
    path: &Option<PathBuf>,
) -> Result<Vec<Vec<NormalizedKey>>, VariantConfigError> {
    let sequences = node
        .as_sequence()
        .ok_or_else(|| VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::expected_type("list", "other", *node.span()),
        })?;

    let mut result = Vec::new();
    for seq_node in sequences.iter() {
        let inner = seq_node
            .as_sequence()
            .ok_or_else(|| VariantConfigError::ParseError {
                path: path.clone().unwrap_or_default(),
                source: ParseError::generic("zip_keys must be a list of lists", *seq_node.span()),
            })?;

        let keys: Vec<NormalizedKey> = inner
            .iter()
            .map(|v| {
                let key_str = v
                    .as_scalar()
                    .ok_or_else(|| VariantConfigError::ParseError {
                        path: path.clone().unwrap_or_default(),
                        source: ParseError::generic("Invalid zip key", *v.span()),
                    })?;
                Ok(key_str.as_str().into())
            })
            .collect::<Result<_, VariantConfigError>>()?;

        result.push(keys);
    }

    Ok(result)
}

/// Parse variant values from a marked_yaml Node, handling conditionals and templates
fn parse_variant_values(
    node: &Node,
    key: &str,
    path: &Option<PathBuf>,
) -> Result<ConditionalList, VariantConfigError> {
    let sequence = node
        .as_sequence()
        .ok_or_else(|| VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::generic(
                format!("Variant values for '{}' must be a list", key),
                *node.span(),
            ),
        })?;

    let mut items = Vec::new();

    for item_node in sequence.iter() {
        // Check if this is a conditional (has 'if' and 'then' keys)
        if let Some(mapping) = item_node.as_mapping() {
            // Check if this has an 'if' key
            let if_key = mapping.iter().find(|(k, _)| k.as_str() == "if");

            if let Some((_, _condition_node)) = if_key {
                // This is a conditional
                let conditional = parse_conditional(item_node, key, path)?;
                items.push(Item::Conditional(conditional));
                continue;
            }
        }

        // Regular value - convert to Item::Value
        let value = parse_value(item_node, path)?;
        items.push(Item::Value(value));
    }

    Ok(ConditionalList::new(items))
}

/// Parse a conditional from a marked_yaml Node
fn parse_conditional(
    node: &Node,
    key: &str,
    path: &Option<PathBuf>,
) -> Result<Conditional, VariantConfigError> {
    let mapping = node
        .as_mapping()
        .ok_or_else(|| VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::expected_type("mapping", "other", *node.span()),
        })?;

    // Extract 'if' condition
    let (_, condition_node) = mapping
        .iter()
        .find(|(k, _)| k.as_str() == "if")
        .ok_or_else(|| VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::generic(
                format!("Conditional for '{}' must have 'if' key", key),
                *node.span(),
            ),
        })?;

    let condition_scalar =
        condition_node
            .as_scalar()
            .ok_or_else(|| VariantConfigError::ParseError {
                path: path.clone().unwrap_or_default(),
                source: ParseError::expected_type("string", "other", *condition_node.span()),
            })?;

    let condition_str = condition_scalar.as_str();

    let condition = JinjaExpression::new(condition_str.to_string()).map_err(|e| {
        VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::jinja_error(
                format!("Invalid condition '{}': {}", condition_str, e),
                *condition_node.span(),
            ),
        }
    })?;

    // Extract 'then' values
    let (_, then_node) = mapping
        .iter()
        .find(|(k, _)| k.as_str() == "then")
        .ok_or_else(|| VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::generic(
                format!("Conditional for '{}' must have 'then' key", key),
                *node.span(),
            ),
        })?;

    let then_values = parse_then_else_values(then_node, path)?;

    // Extract optional 'else' values
    let else_values = mapping
        .iter()
        .find(|(k, _)| k.as_str() == "else")
        .map(|(_, else_node)| parse_then_else_values(else_node, path))
        .transpose()?;

    Ok(Conditional {
        condition,
        then: then_values,
        else_value: else_values,
    })
}

/// Parse then/else values - can be a single value or a list
fn parse_then_else_values(
    node: &Node,
    path: &Option<PathBuf>,
) -> Result<ListOrItem<Value>, VariantConfigError> {
    if let Some(sequence) = node.as_sequence() {
        // It's a list
        let mut values = Vec::new();
        for item_node in sequence.iter() {
            values.push(parse_value(item_node, path)?);
        }
        Ok(ListOrItem::new(values))
    } else {
        // It's a single value
        let value = parse_value(node, path)?;
        Ok(ListOrItem::single(value))
    }
}

/// Parse a single value from a marked_yaml Node
fn parse_value(node: &Node, path: &Option<PathBuf>) -> Result<Value, VariantConfigError> {
    let span = *node.span();

    // Check if it's a scalar that might be a template
    if let Some(scalar) = node.as_scalar() {
        let s = scalar.as_str();
        if s.contains("${{") && s.contains("}}") {
            // It's a template
            let template =
                JinjaTemplate::new(s.to_string()).map_err(|e| VariantConfigError::ParseError {
                    path: path.clone().unwrap_or_default(),
                    source: ParseError::jinja_error(
                        format!("Invalid Jinja template '{}': {}", s, e),
                        span,
                    ),
                })?;
            return Ok(Value::new_template(template, Some(span)));
        }
    }

    // Convert to Variable based on the node type
    let variable = node_to_variable(node, path)?;
    Ok(Value::new_concrete(variable, Some(span)))
}

/// Convert a marked_yaml Node to a Variable
fn node_to_variable(node: &Node, path: &Option<PathBuf>) -> Result<Variable, VariantConfigError> {
    if let Some(scalar) = node.as_scalar() {
        // Get the string representation
        let s = scalar.as_str();

        // Check if the scalar may coerce to a non-string type (i.e., it's unquoted)
        // may_coerce() returns true for unquoted values that could be numbers/booleans
        let may_coerce = scalar.may_coerce();

        if !may_coerce {
            // Quoted string - use the same method as From<String> impl but this uses from_safe_string
            // which parses the string. We need to check if this is actually a problem in practice.
            // The from_safe_string method creates arc'd strings which is what we want.
            Ok(Variable::from(s.to_string()))
        } else {
            // Try to parse as bool
            if s == "true" || s == "false" {
                Ok(Variable::from(s == "true"))
            } else if let Ok(i) = s.parse::<i64>() {
                // Parse as integer
                Ok(Variable::from(i))
            } else if s.parse::<f64>().is_ok() {
                // Float - convert to string to preserve version numbers
                Ok(Variable::from(minijinja::Value::from(s.to_string())))
            } else {
                // Fallback to string
                Ok(Variable::from(minijinja::Value::from(s.to_string())))
            }
        }
    } else {
        Err(VariantConfigError::ParseError {
            path: path.clone().unwrap_or_default(),
            source: ParseError::generic("Unsupported variant value type", *node.span()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
