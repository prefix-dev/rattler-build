//! Module to load legacy `conda_build_config.yaml` files
//!
//! This module provides support for the older conda-build configuration format,
//! which supports conditional lines using `# [selector]` syntax.

use minijinja::Value;
use rattler_build_jinja::{Jinja, JinjaConfig};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::{config::VariantConfig, error::VariantConfigError};

/// Look up an environment variable by name, returning `None` on WASM.
fn lookup_env_var(name: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var(name).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        None
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

fn evaluate_condition(condition: &str, jinja: &Jinja) -> bool {
    if condition.is_empty() {
        return true;
    }

    let template_str = format!("{{% if {} %}}true{{% else %}}false{{% endif %}}", condition);

    let jinja_res = jinja.render_str(&template_str);
    jinja_res.map(|v| v == "true").unwrap_or(false)
}

/// Obtain a Jinja instance for conda_build_config parsing purposes
fn conda_build_config_jinja(jinja_config: &JinjaConfig) -> Jinja {
    let mut jinja = Jinja::new(jinja_config.clone());

    // Add platform shorthands to jinja context
    let short_target_platform = jinja_config.target_platform.to_string().replace("-", "");
    jinja
        .context_mut()
        .insert(short_target_platform, Value::from(true));

    // Add environ_get function for environment variable access
    jinja.env_mut().add_function(
        "environ_get",
        move |name: String, default: Option<String>| {
            let value = lookup_env_var(&name).unwrap_or_else(|| default.unwrap_or_default());
            Ok(Value::from(value))
        },
    );

    jinja
}

/// Parse a `conda_build_config.yaml` string with selector support
///
/// This is the string-based version of [`load_conda_build_config`] that works on
/// in-memory strings instead of files. It supports the same selector syntax:
/// - Conditional lines using `# [selector]` syntax
/// - `os.environ.get(...)` for environment variables
/// - Platform selectors (unix, linux, osx, win)
pub fn parse_conda_build_config(
    input: &str,
    config: &JinjaConfig,
) -> Result<VariantConfig, VariantConfigError> {
    let jinja = conda_build_config_jinja(config);

    // Replace Python-style calls with Jinja-compatible ones
    let mut input = input.replace("os.environ.get", "environ_get");
    input = input.replace(".startswith", " is startingwith");

    // Process lines with selectors
    let mut lines = Vec::new();
    for line in input.lines() {
        let parsed = ParsedLine::from_str(line);
        let mut line_content = if let Some(condition) = &parsed.condition {
            if evaluate_condition(condition, &jinja) {
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
    let value: serde_yaml::Value =
        serde_yaml::from_str(&out).map_err(|e| VariantConfigError::InvalidConfig(e.to_string()))?;

    if value.is_null() {
        return Ok(VariantConfig::default());
    }

    // Filter empty/null entries
    let value = value
        .as_mapping()
        .ok_or_else(|| {
            VariantConfigError::InvalidConfig(
                "Expected conda_build_config.yaml to be a mapping".to_string(),
            )
        })?
        .clone()
        .into_iter()
        .filter(|(k, v)| {
            // Emit warning for pin_run_as_build
            if let Some(key_str) = k.as_str()
                && key_str == "pin_run_as_build"
            {
                tracing::warn!("Found 'pin_run_as_build' in conda_build_config.yaml - this is currently not supported and will be ignored");
                return false;
            }
            !v.is_null()
        })
        .collect::<serde_yaml::Mapping>();

    let config: VariantConfig = serde_yaml::from_value(serde_yaml::Value::Mapping(value))
        .map_err(|e| VariantConfigError::InvalidConfig(e.to_string()))?;

    Ok(config)
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
#[cfg(not(target_arch = "wasm32"))]
pub fn load_conda_build_config(
    path: &Path,
    config: &JinjaConfig,
) -> Result<VariantConfig, VariantConfigError> {
    let input = fs_err::read_to_string(path)
        .map_err(|e| VariantConfigError::IoError(path.to_path_buf(), e))?;
    parse_conda_build_config(&input, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::Platform;
    use rstest::rstest;
    use serial_test::serial;
    use std::path::PathBuf;

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
        let mut jinja = Jinja::new(JinjaConfig::default());
        jinja
            .context_mut()
            .insert("py3k".to_string(), Value::from(true));
        assert!(evaluate_condition("py3k", &jinja));

        jinja
            .context_mut()
            .insert("py3k".to_string(), Value::from(false));
        assert!(!evaluate_condition("py3k", &jinja));
    }

    #[test]
    fn test_selector_context() {
        let config = JinjaConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            ..Default::default()
        };
        let jinja = conda_build_config_jinja(&config);
        let ctx = jinja.context();

        assert!(ctx.get("unix").unwrap().is_true());
        assert!(ctx.get("linux").unwrap().is_true());
        assert!(!ctx.get("win").unwrap().is_true());
        assert!(ctx.get("linux64").unwrap().is_true());
    }

    #[rstest]
    #[case("conda_build_config/test_1.yaml", None)]
    #[case("conda_build_config/all_filtered.yaml", None)]
    #[case("conda_build_config/conda_forge_subset.yaml", Some(false))]
    #[case("conda_build_config/conda_forge_subset.yaml", Some(true))]
    #[case("conda_build_config/conda_forge_subset.yaml", None)]
    #[serial]
    fn test_conda_forge(#[case] config_path: &str, #[case] cuda: Option<bool>) {
        let path = test_data_dir().join(config_path);

        // fix the platform for the snapshots
        let jinja_config = JinjaConfig {
            target_platform: Platform::OsxArm64,
            host_platform: Platform::OsxArm64,
            ..Default::default()
        };

        if let Some(cuda) = cuda {
            unsafe {
                std::env::set_var("TEST_CF_CUDA_ENABLED", if cuda { "True" } else { "False" })
            };
        }

        let config = load_conda_build_config(&path, &jinja_config).unwrap();
        insta::assert_yaml_snapshot!(
            format!(
                "{}_{}",
                config_path,
                cuda.map(|o| o.to_string()).unwrap_or("none".to_string())
            ),
            config
        );

        if let Some(cuda) = cuda {
            if cuda {
                assert_eq!(
                    config.variants[&"environment_var".into()],
                    vec!["CF_CUDA_ENABLED".into()]
                );
            } else {
                assert_eq!(
                    config.variants[&"environment_var".into()],
                    vec!["CF_CUDA_DISABLED".into()]
                );
            }
            unsafe {
                std::env::remove_var("TEST_CF_CUDA_ENABLED");
            }
        }
    }

    fn test_data_dir() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/")
    }
}
