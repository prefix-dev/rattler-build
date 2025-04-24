//! Module to load deprecated `conda_build_config.yaml` files and apply the selector_config to it
use minijinja::{Environment, Value};
use std::path::PathBuf;
use std::{collections::BTreeMap, path::Path};

use crate::{selectors::SelectorConfig, variant_config::VariantConfig};

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
    let template = env.template_from_str(&template_str).unwrap();

    template.render(context).unwrap() == "true"
}

/// An error that can occur when parsing a `conda_build_config.yaml` file
#[derive(Debug, thiserror::Error)]
pub enum ParseConfigBuildConfigError {
    /// An IO error occurred while trying to read the file
    #[error("Could not open file ({0}): {1}")]
    IOError(PathBuf, std::io::Error),

    /// An error occurred while parsing the file
    #[error("Could not parse variant config file ({0}): {1}")]
    ParseError(PathBuf, serde_yaml::Error),
}

/// Load an old-school conda_build_config.yaml file, and apply the selector_config to it
/// The parser supports only a small subset of (potential) conda_build_config.yaml features.
/// Especially, only `os.environ.get(...)` is supported.
pub fn load_conda_build_config(
    path: &Path,
    selector_config: &SelectorConfig,
) -> Result<VariantConfig, ParseConfigBuildConfigError> {
    // load the text, parse it and load as VariantConfig using serde_yaml
    let mut input = fs_err::read_to_string(path)
        .map_err(|e| ParseConfigBuildConfigError::IOError(path.to_path_buf(), e))?;

    let mut context = selector_config.clone().into_context();

    let short_target_platform = selector_config.target_platform.to_string().replace("-", "");
    context.insert(short_target_platform, Value::from(true));

    let mut env = Environment::new();

    env.add_function(
        "environ_get",
        move |name: String, default: Option<String>| {
            let value = std::env::var(name).unwrap_or_else(|_| default.unwrap_or_default());
            Ok(Value::from(value))
        },
    );

    // replace all `os.environ.get` calls with `environ_get`
    input = input.replace("os.environ.get", "environ_get");
    // replace calls to `.startswith` with `is startingwith`
    input = input.replace(".startswith", " is startingwith");

    let mut lines = Vec::new();
    for line in input.lines() {
        let parsed = ParsedLine::from_str(line);
        let mut line_content = if let Some(condition) = &parsed.condition {
            if evaluate_condition(condition, &env, &context) {
                parsed.content.to_string()
            } else {
                continue;
            }
        } else {
            parsed.content.to_string()
        };

        let trimmed = line_content.trim();
        if let Some(node) = trimmed.strip_prefix("- ") {
            let s = node.trim();
            // try to parse as a float or integer
            if s.parse::<f64>().is_ok() || s.parse::<i64>().is_ok() {
                line_content = line_content.replace(s, &format!("\"{}\"", s));
            }
        }

        lines.push(line_content);
    }

    let out = lines.join("\n");

    // We need to filter "unit keys" from the YAML because our config expects a list (not None)
    let value: serde_yaml::Value = serde_yaml::from_str(&out).map_err(|e| {
        ParseConfigBuildConfigError::IOError(
            path.to_path_buf(),
            std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        )
    })?;

    if value.is_null() {
        return Ok(VariantConfig::default());
    }

    // filter all empty maps
    let value = value
        .as_mapping()
        .ok_or_else(|| {
            ParseConfigBuildConfigError::IOError(
                path.to_path_buf(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Expected `conda_build_config.yaml` to be a mapping",
                ),
            )
        })?
        .clone()
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .collect::<serde_yaml::Mapping>();

    let config: VariantConfig =
        serde_yaml::from_value(serde_yaml::Value::Mapping(value)).map_err(|e| {
            ParseConfigBuildConfigError::IOError(
                path.to_path_buf(),
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use rattler_conda_types::Platform;
    use std::path::PathBuf;

    use rstest::rstest;
    use serial_test::serial;

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

        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(true));
        let env = Environment::new();
        assert!(!evaluate_condition("not py3k", &env, &context));

        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(false));
        let env = Environment::new();
        assert!(evaluate_condition("not py3k", &env, &context));
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
        let selector_config = SelectorConfig {
            target_platform: Platform::OsxArm64,
            host_platform: Platform::OsxArm64,
            build_platform: Platform::OsxArm64,
            ..Default::default()
        };

        if let Some(cuda) = cuda {
            unsafe {
                std::env::set_var("TEST_CF_CUDA_ENABLED", if cuda { "True" } else { "False" })
            };
        }

        let config = load_conda_build_config(&path, &selector_config).unwrap();
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
