//! Parser for variant configuration files with conditional and Jinja support
//!
//! This module provides parsing for advanced variant configuration syntax including:
//! - if/then conditionals based on platform selectors
//! - Jinja template expressions for dynamic values

use std::collections::BTreeMap;
use std::path::Path;

use minijinja::{Environment, Value};
use serde_yaml;

use crate::{
    NormalizedKey, Variable, conda_build_config::SelectorContext, config::VariantConfig,
    error::VariantConfigError,
};

/// Parse a variant configuration file with full Jinja and conditional support
pub fn parse_variant_file(
    path: &Path,
    context: &SelectorContext,
) -> Result<VariantConfig, VariantConfigError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| VariantConfigError::IoError(path.to_path_buf(), e))?;

    parse_variant_str(&content, context)
}

/// Parse a variant configuration string with full Jinja and conditional support
pub fn parse_variant_str(
    yaml: &str,
    context: &SelectorContext,
) -> Result<VariantConfig, VariantConfigError> {
    // Parse YAML
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).map_err(|e| {
        VariantConfigError::ParseError(std::path::PathBuf::from("<string>"), e.to_string())
    })?;

    // Set up minijinja environment
    let env = Environment::new();
    let jinja_context = context.to_context();

    // Process the YAML value
    let config = process_yaml_value(value, &env, &jinja_context)?;

    Ok(config)
}

/// Process a YAML value and evaluate conditionals/Jinja expressions
fn process_yaml_value(
    value: serde_yaml::Value,
    env: &Environment,
    context: &BTreeMap<String, Value>,
) -> Result<VariantConfig, VariantConfigError> {
    let mapping = value.as_mapping().ok_or_else(|| {
        VariantConfigError::InvalidConfig("Variant config must be a YAML mapping".to_string())
    })?;

    let mut variants = BTreeMap::new();
    let mut zip_keys = None;

    for (key, value) in mapping {
        let key_str = key
            .as_str()
            .ok_or_else(|| VariantConfigError::InvalidConfig(format!("Invalid key: {:?}", key)))?;

        if key_str == "zip_keys" {
            zip_keys = Some(parse_zip_keys(value)?);
            continue;
        }

        if key_str == "pin_run_as_build" {
            tracing::warn!(
                "Found 'pin_run_as_build' in variant config - this is not supported and will be ignored"
            );
            continue;
        }

        // Parse variant values (which may contain conditionals or Jinja)
        let values = parse_variant_values(value, env, context)?;
        if !values.is_empty() {
            variants.insert(key_str.into(), values);
        }
    }

    Ok(VariantConfig { zip_keys, variants })
}

/// Parse zip_keys from YAML value
fn parse_zip_keys(
    value: &serde_yaml::Value,
) -> Result<Vec<Vec<NormalizedKey>>, VariantConfigError> {
    let sequences = value
        .as_sequence()
        .ok_or_else(|| VariantConfigError::InvalidConfig("zip_keys must be a list".to_string()))?;

    let mut result = Vec::new();
    for seq in sequences {
        let inner = seq.as_sequence().ok_or_else(|| {
            VariantConfigError::InvalidConfig("zip_keys must be a list of lists".to_string())
        })?;

        let keys: Vec<NormalizedKey> = inner
            .iter()
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| {
                        VariantConfigError::InvalidConfig(format!("Invalid zip key: {:?}", v))
                    })
                    .map(|s| s.into())
            })
            .collect::<Result<_, _>>()?;

        result.push(keys);
    }

    Ok(result)
}

/// Parse variant values, handling conditionals and Jinja expressions
fn parse_variant_values(
    value: &serde_yaml::Value,
    env: &Environment,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<Variable>, VariantConfigError> {
    let sequence = value.as_sequence().ok_or_else(|| {
        VariantConfigError::InvalidConfig("Variant values must be a list".to_string())
    })?;

    let mut result = Vec::new();

    for item in sequence {
        // Check if this is a conditional (has 'if' and 'then' keys)
        if let Some(mapping) = item.as_mapping() {
            if let Some(condition) = mapping.get(&serde_yaml::Value::String("if".to_string())) {
                // This is an if/then conditional
                let then_value = mapping
                    .get(&serde_yaml::Value::String("then".to_string()))
                    .ok_or_else(|| {
                        VariantConfigError::InvalidConfig(
                            "Conditional must have 'then' clause".to_string(),
                        )
                    })?;

                // Evaluate the condition
                let condition_str = condition.as_str().ok_or_else(|| {
                    VariantConfigError::InvalidConfig("Condition must be a string".to_string())
                })?;

                if evaluate_condition(condition_str, env, context)? {
                    // Condition is true, process the 'then' values
                    let then_values = parse_variant_values(then_value, env, context)?;
                    result.extend(then_values);
                }
                continue;
            }
        }

        // Check if this is a Jinja expression ${{ ... }}
        if let Some(s) = item.as_str() {
            if s.contains("${{") && s.contains("}}") {
                if let Some(evaluated) = evaluate_jinja_expression(s, env, context)? {
                    result.push(evaluated.into());
                }
                // If evaluated to None/null, skip adding it (filter null values)
                continue;
            }
        }

        // Regular value - convert to Variable
        result.push(yaml_to_variable(item)?);
    }

    Ok(result)
}

/// Evaluate a conditional expression
fn evaluate_condition(
    condition: &str,
    env: &Environment,
    context: &BTreeMap<String, Value>,
) -> Result<bool, VariantConfigError> {
    let template_str = format!("{{% if {} %}}true{{% else %}}false{{% endif %}}", condition);
    let template = env.template_from_str(&template_str).map_err(|e| {
        VariantConfigError::InvalidConfig(format!("Invalid condition '{}': {}", condition, e))
    })?;

    let result = template.render(context).map_err(|e| {
        VariantConfigError::InvalidConfig(format!(
            "Failed to evaluate condition '{}': {}",
            condition, e
        ))
    })?;

    Ok(result == "true")
}

/// Evaluate a Jinja expression ${{ ... }}
/// Returns None if the expression evaluates to an empty string
fn evaluate_jinja_expression(
    expr: &str,
    env: &Environment,
    context: &BTreeMap<String, Value>,
) -> Result<Option<String>, VariantConfigError> {
    // Extract the expression from ${{ ... }}
    let start = expr.find("${{").ok_or_else(|| {
        VariantConfigError::InvalidConfig(format!("Invalid Jinja expression: {}", expr))
    })?;
    let end = expr.find("}}").ok_or_else(|| {
        VariantConfigError::InvalidConfig(format!("Invalid Jinja expression: {}", expr))
    })?;

    let jinja_expr = &expr[start + 3..end].trim();

    // Evaluate using Jinja
    let template_str = format!("{{{{ {} }}}}", jinja_expr);
    let template = env.template_from_str(&template_str).map_err(|e| {
        VariantConfigError::InvalidConfig(format!(
            "Invalid Jinja expression '{}': {}",
            jinja_expr, e
        ))
    })?;

    let result = template.render(context).map_err(|e| {
        VariantConfigError::InvalidConfig(format!(
            "Failed to evaluate Jinja expression '{}': {}",
            jinja_expr, e
        ))
    })?;

    // Filter out empty strings (which represent null/false values from Jinja)
    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

/// Convert a YAML value to a Variable
fn yaml_to_variable(value: &serde_yaml::Value) -> Result<Variable, VariantConfigError> {
    match value {
        serde_yaml::Value::String(s) => Ok(s.as_str().into()),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into())
            } else {
                // Convert to string for floats
                Ok(n.to_string().into())
            }
        }
        serde_yaml::Value::Bool(b) => Ok((*b).into()),
        _ => Err(VariantConfigError::InvalidConfig(format!(
            "Unsupported YAML value type: {:?}",
            value
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::Platform;

    #[test]
    fn test_simple_parsing() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
"#;
        let context = SelectorContext::new(Platform::Linux64);
        let config = parse_variant_str(yaml, &context).unwrap();

        assert_eq!(config.variants.len(), 1);
        assert_eq!(config.get(&"python".into()).unwrap().len(), 2);
    }

    #[test]
    fn test_conditional_unix() {
        let yaml = r#"
python:
  - if: unix
    then: ["3.14", "3.15"]
  - if: win
    then: ["3.14"]
"#;
        let context = SelectorContext::new(Platform::Linux64);
        let config = parse_variant_str(yaml, &context).unwrap();

        let python_vals = config.get(&"python".into()).unwrap();
        assert_eq!(python_vals.len(), 2);
        assert_eq!(python_vals[0].to_string(), "3.14");
        assert_eq!(python_vals[1].to_string(), "3.15");
    }

    #[test]
    fn test_conditional_win() {
        let yaml = r#"
python:
  - if: unix
    then: ["3.14", "3.15"]
  - if: win
    then: ["3.14"]
"#;
        let context = SelectorContext::new(Platform::Win64);
        let config = parse_variant_str(yaml, &context).unwrap();

        let python_vals = config.get(&"python".into()).unwrap();
        assert_eq!(python_vals.len(), 1);
        assert_eq!(python_vals[0].to_string(), "3.14");
    }

    #[test]
    fn test_jinja_expression() {
        let yaml = r#"
foobar:
  - ${{ "unknown" if unix else "known" }}
"#;
        let context = SelectorContext::new(Platform::Linux64);
        let config = parse_variant_str(yaml, &context).unwrap();

        let vals = config.get(&"foobar".into()).unwrap();
        assert_eq!(vals[0].to_string(), "unknown");
    }

    #[test]
    fn test_jinja_variable() {
        let yaml = r#"
target:
  - ${{ target_platform }}
"#;
        let context = SelectorContext::new(Platform::Linux64);
        let config = parse_variant_str(yaml, &context).unwrap();

        let vals = config.get(&"target".into()).unwrap();
        assert_eq!(vals[0].to_string(), "linux-64");
    }

    #[test]
    fn test_null_filtering() {
        let yaml = r#"
filter:
  - ${{ "appears" if true }}
  - ${{ "disappears" if false }}
"#;
        let context = SelectorContext::new(Platform::Linux64);
        let config = parse_variant_str(yaml, &context).unwrap();

        let vals = config.get(&"filter".into()).unwrap();
        // Only "appears" should be in the result, "disappears" is filtered out
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].to_string(), "appears");
    }
}
