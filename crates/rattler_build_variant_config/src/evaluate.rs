//! Evaluation logic for variant configuration - converts Stage0 (with templates) to Stage1 (concrete values)

use crate::config::VariantConfig;
use crate::error::VariantConfigError;
use crate::stage0_types::{ConditionalList, Item, Value};
use crate::yaml_parser::Stage0VariantConfig;
use rattler_build_jinja::{Jinja, JinjaConfig, Variable};
use rattler_build_types::NormalizedKey;
use std::collections::BTreeMap;

/// Evaluate a Stage0VariantConfig using a JinjaConfig to produce a concrete VariantConfig
pub fn evaluate_variant_config(
    stage0: &Stage0VariantConfig,
    jinja_config: &JinjaConfig,
) -> Result<VariantConfig, VariantConfigError> {
    let jinja = Jinja::new(jinja_config.clone());

    let mut variants = BTreeMap::new();

    for (key, conditional_list) in &stage0.variants {
        let evaluated_values = evaluate_conditional_list(conditional_list, &jinja, key)?;
        if !evaluated_values.is_empty() {
            variants.insert(key.clone(), evaluated_values);
        }
    }

    Ok(VariantConfig {
        zip_keys: stage0.zip_keys.clone(),
        variants,
    })
}

/// Evaluate a ConditionalList to produce a Vec<Variable>
fn evaluate_conditional_list(
    list: &ConditionalList<Variable>,
    jinja: &Jinja,
    key: &NormalizedKey,
) -> Result<Vec<Variable>, VariantConfigError> {
    let mut result = Vec::new();

    for item in list.iter() {
        evaluate_item(item, jinja, key, &mut result)?;
    }

    Ok(result)
}

/// Evaluate a single item (value or conditional) and push results to the result vector
fn evaluate_item(
    item: &Item<Variable>,
    jinja: &Jinja,
    key: &NormalizedKey,
    result: &mut Vec<Variable>,
) -> Result<(), VariantConfigError> {
    match item {
        Item::Value(value) => {
            if let Some(evaluated) = evaluate_value(value, jinja, key)? {
                result.push(evaluated);
            }
        }
        Item::Conditional(conditional) => {
            // Evaluate the condition
            let condition_result = jinja
                .eval(conditional.condition.source())
                .map_err(|e| {
                    VariantConfigError::InvalidConfig(format!(
                        "Failed to evaluate condition '{}' for variant key '{:?}': {}",
                        conditional.condition.source(),
                        key,
                        e
                    ))
                })?
                .is_true();

            // Choose the appropriate branch
            let branch = if condition_result {
                &conditional.then
            } else if let Some(else_branch) = &conditional.else_value {
                else_branch
            } else {
                // No else branch and condition is false - skip
                return Ok(());
            };

            // Recursively evaluate all items in the branch (supports nested conditionals)
            for nested_item in branch.iter() {
                evaluate_item(nested_item, jinja, key, result)?;
            }
        }
    }

    Ok(())
}

/// Evaluate a single Value<Variable> to produce an optional Variable
/// Returns None if the value evaluates to null/empty (for filtering)
fn evaluate_value(
    value: &Value<Variable>,
    jinja: &Jinja,
    key: &NormalizedKey,
) -> Result<Option<Variable>, VariantConfigError> {
    if let Some(concrete) = value.as_concrete() {
        Ok(Some(concrete.clone()))
    } else if let Some(template) = value.as_template() {
        let rendered = jinja.render_str(template.as_str()).map_err(|e| {
            VariantConfigError::InvalidConfig(format!(
                "Failed to render template '{}' for variant key '{:?}': {}",
                template.as_str(),
                key,
                e
            ))
        })?;

        // Filter out empty strings (which represent null/false values from Jinja)
        if rendered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Variable::from_string(&rendered)))
        }
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml_parser::parse_variant_str;
    use rattler_conda_types::Platform;

    #[test]
    fn test_evaluate_simple() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();
        let jinja_config = JinjaConfig::default();
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        assert_eq!(config.variants.len(), 1);
        let python_vals = config.get(&"python".into()).unwrap();
        assert_eq!(python_vals.len(), 2);
        assert_eq!(python_vals[0].to_string(), "3.9");
        assert_eq!(python_vals[1].to_string(), "3.10");
    }

    #[test]
    fn test_evaluate_conditional_unix() {
        let yaml = r#"
vc:
  - if: unix
    then: "16"
  - if: win
    then: "14"
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();

        // Test with Linux platform (unix = true)
        let jinja_config = JinjaConfig {
            target_platform: Platform::Linux64,
            ..Default::default()
        };
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        let vc_vals = config.get(&"vc".into()).unwrap();
        assert_eq!(vc_vals.len(), 1);
        assert_eq!(vc_vals[0].to_string(), "16");
    }

    #[test]
    fn test_evaluate_conditional_win() {
        let yaml = r#"
vc:
  - if: unix
    then: "16"
  - if: win
    then: "14"
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();

        // Test with Windows platform (win = true)
        let jinja_config = JinjaConfig {
            target_platform: Platform::Win64,
            ..Default::default()
        };
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        let vc_vals = config.get(&"vc".into()).unwrap();
        assert_eq!(vc_vals.len(), 1);
        assert_eq!(vc_vals[0].to_string(), "14");
    }

    #[test]
    fn test_evaluate_template() {
        let yaml = r#"
target:
  - ${{ target_platform }}
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();

        let jinja_config = JinjaConfig {
            target_platform: Platform::Linux64,
            ..Default::default()
        };
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        let target_vals = config.get(&"target".into()).unwrap();
        assert_eq!(target_vals.len(), 1);
        assert_eq!(target_vals[0].to_string(), "linux-64");
    }

    #[test]
    fn test_evaluate_mixed() {
        let yaml = r#"
mixed:
  - "plain"
  - ${{ target_platform }}
  - if: unix
    then: ["unix-val"]
  - if: win
    then: ["win-val"]
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();

        let jinja_config = JinjaConfig {
            target_platform: Platform::Linux64,
            ..Default::default()
        };
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        let mixed_vals = config.get(&"mixed".into()).unwrap();
        assert_eq!(mixed_vals.len(), 3); // "plain", "linux-64", "unix-val"
        assert_eq!(mixed_vals[0].to_string(), "plain");
        assert_eq!(mixed_vals[1].to_string(), "linux-64");
        assert_eq!(mixed_vals[2].to_string(), "unix-val");
    }

    #[test]
    fn test_evaluate_conditional_with_list() {
        let yaml = r#"
python:
  - if: unix
    then: ["3.9", "3.10", "3.11"]
  - if: win
    then: ["3.8"]
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();

        let jinja_config = JinjaConfig {
            target_platform: Platform::Linux64,
            ..Default::default()
        };
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        let python_vals = config.get(&"python".into()).unwrap();
        assert_eq!(python_vals.len(), 3);
        assert_eq!(python_vals[0].to_string(), "3.9");
        assert_eq!(python_vals[1].to_string(), "3.10");
        assert_eq!(python_vals[2].to_string(), "3.11");
    }

    #[test]
    fn test_filter_null_values() {
        let yaml = r#"
filter:
  - ${{ "appears" if true }}
  - ${{ "disappears" if false }}
"#;
        let stage0 = parse_variant_str(yaml, None).unwrap();

        let jinja_config = JinjaConfig::default();
        let config = evaluate_variant_config(&stage0, &jinja_config).unwrap();

        let filter_vals = config.get(&"filter".into()).unwrap();
        // Only "appears" should be in the result
        assert_eq!(filter_vals.len(), 1);
        assert_eq!(filter_vals[0].to_string(), "appears");
    }
}
