//! Module to load legacy `conda_build_config.yaml` files
//!
//! This module provides support for the older conda-build configuration format,
//! which supports conditional lines using `# [selector]` syntax.

use minijinja::{Environment, Value};
use rattler_conda_types::Platform;
use std::{collections::BTreeMap, path::Path};

use crate::{config::VariantConfig, error::VariantConfigError};

/// Context for evaluating selectors in conda_build_config files
#[derive(Debug, Clone)]
pub struct SelectorContext {
    /// Target platform (e.g., linux-64, osx-arm64)
    pub target_platform: Platform,
    /// Build platform (usually same as target)
    pub build_platform: Platform,
    /// Additional context variables
    pub variables: BTreeMap<String, Value>,
}

impl SelectorContext {
    /// Create a new selector context
    pub fn new(target_platform: Platform) -> Self {
        Self {
            target_platform,
            build_platform: target_platform,
            variables: BTreeMap::new(),
        }
    }

    /// Convert to minijinja context
    pub fn to_context(&self) -> BTreeMap<String, Value> {
        let mut context = self.variables.clone();

        // Add platform info
        context.insert(
            "target_platform".to_string(),
            Value::from(self.target_platform.to_string()),
        );
        context.insert(
            "build_platform".to_string(),
            Value::from(self.build_platform.to_string()),
        );

        // Add boolean platform shortcuts
        let platform_str = self.target_platform.to_string();
        context.insert(
            "unix".to_string(),
            Value::from(!platform_str.starts_with("win")),
        );
        context.insert(
            "linux".to_string(),
            Value::from(platform_str.starts_with("linux")),
        );
        context.insert(
            "osx".to_string(),
            Value::from(platform_str.starts_with("osx")),
        );
        context.insert(
            "win".to_string(),
            Value::from(platform_str.starts_with("win")),
        );

        // Add short platform name without dash (e.g., linux64, osxarm64)
        let short_platform = platform_str.replace("-", "");
        context.insert(short_platform, Value::from(true));

        context
    }
}

impl Default for SelectorContext {
    fn default() -> Self {
        Self::new(Platform::current())
    }
}

#[derive(Debug)]
struct ParsedLine<'a> {
    content: &'a str,
    condition: Option<&'a str>,
}

impl<'a> ParsedLine<'a> {
    pub fn from_str(s: &'a str) -> ParsedLine<'a> {
        match s.split_once('#') {
            Some((content, cond)) => ParsedLine {
                content: content.trim_end(),
                condition: cond
                    .trim()
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .map(str::trim),
            },
            None => ParsedLine {
                content: s.trim_end(),
                condition: None,
            },
        }
    }
}

fn evaluate_condition(
    condition: &str,
    env: &Environment,
    context: &BTreeMap<String, Value>,
) -> bool {
    if condition.is_empty() {
        return true;
    }

    let template_str = format!("{{% if {} %}}true{{% else %}}false{{% endif %}}", condition);
    let template = match env.template_from_str(&template_str) {
        Ok(t) => t,
        Err(_) => return false,
    };

    template
        .render(context)
        .unwrap_or_else(|_| "false".to_string())
        == "true"
}

/// Load a `conda_build_config.yaml` file with selector support
///
/// The parser supports:
/// - Conditional lines using `# [selector]` syntax
/// - `os.environ.get(...)` for environment variables
/// - Platform selectors (unix, linux, osx, win)
///
/// # Example
///
/// ```yaml
/// python:
///   - 3.9
///   - 3.10  # [unix]
///   - 3.11  # [osx]
/// ```
pub fn load_conda_build_config(
    path: &Path,
    context: &SelectorContext,
) -> Result<VariantConfig, VariantConfigError> {
    let mut input = fs_err::read_to_string(path)
        .map_err(|e| VariantConfigError::IoError(path.to_path_buf(), e))?;

    let selector_context = context.to_context();
    let mut env = Environment::new();

    // Add environ_get function for environment variable access
    env.add_function(
        "environ_get",
        move |name: String, default: Option<String>| {
            let value = std::env::var(name).unwrap_or_else(|_| default.unwrap_or_default());
            Ok(Value::from(value))
        },
    );

    // Replace Python-style calls with Jinja-compatible ones
    input = input.replace("os.environ.get", "environ_get");
    input = input.replace(".startswith", " is startingwith");

    // Process lines with selectors
    let mut lines = Vec::new();
    for line in input.lines() {
        let parsed = ParsedLine::from_str(line);
        let mut line_content = if let Some(condition) = &parsed.condition {
            if evaluate_condition(condition, &env, &selector_context) {
                parsed.content.to_string()
            } else {
                continue; // Skip lines that don't match selector
            }
        } else {
            parsed.content.to_string()
        };

        // Quote numeric values in lists to preserve them as strings
        let trimmed = line_content.trim();
        if let Some(node) = trimmed.strip_prefix("- ") {
            let s = node.trim();
            if s.parse::<f64>().is_ok() || s.parse::<i64>().is_ok() {
                line_content = line_content.replace(s, &format!("\"{}\"", s));
            }
        }

        lines.push(line_content);
    }

    let out = lines.join("\n");

    // Parse as YAML and filter null values
    let value: serde_yaml::Value = serde_yaml::from_str(&out)
        .map_err(|e| VariantConfigError::ParseError(path.to_path_buf(), e.to_string()))?;

    if value.is_null() {
        return Ok(VariantConfig::default());
    }

    // Filter empty/null entries
    let value = value
        .as_mapping()
        .ok_or_else(|| {
            VariantConfigError::InvalidConfig(
                "Expected conda_build_config.yaml to be a mapping".to_string()
            )
        })?
        .clone()
        .into_iter()
        .filter(|(k, v)| {
            // Emit warning for pin_run_as_build
            if let Some(key_str) = k.as_str() {
                if key_str == "pin_run_as_build" {
                    tracing::warn!("Found 'pin_run_as_build' in conda_build_config.yaml - this is currently not supported and will be ignored");
                    return false;
                }
            }
            !v.is_null()
        })
        .collect::<serde_yaml::Mapping>();

    let config: VariantConfig = serde_yaml::from_value(serde_yaml::Value::Mapping(value))
        .map_err(|e| VariantConfigError::ParseError(path.to_path_buf(), e.to_string()))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line() {
        let parsed = ParsedLine::from_str("  - python\n");
        assert_eq!(parsed.content, "  - python");
        assert_eq!(parsed.condition, None);

        let parsed = ParsedLine::from_str("  - python # [py3k]\n");
        assert_eq!(parsed.content, "  - python");
        assert_eq!(parsed.condition, Some("py3k"));
    }

    #[test]
    fn test_evaluate_condition() {
        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(true));
        let env = Environment::new();
        assert!(evaluate_condition("py3k", &env, &context));

        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(false));
        let env = Environment::new();
        assert!(!evaluate_condition("py3k", &env, &context));
    }

    #[test]
    fn test_selector_context() {
        let context = SelectorContext::new(Platform::Linux64);
        let ctx = context.to_context();

        assert!(ctx.get("unix").unwrap().is_true());
        assert!(ctx.get("linux").unwrap().is_true());
        assert!(!ctx.get("win").unwrap().is_true());
        assert!(ctx.get("linux64").unwrap().is_true());
    }
}
