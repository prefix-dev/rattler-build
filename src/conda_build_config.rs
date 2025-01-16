//! Module to load deprecated `conda_build_config.yaml` files and apply the selector_config to it
use std::{collections::BTreeMap, path::Path};

use minijinja::{Environment, Value};

use crate::{
    selectors::SelectorConfig,
    variant_config::{VariantConfig, VariantConfigError},
};

#[derive(Debug)]
struct ParsedLine {
    content: String,
    condition: Option<String>,
}

fn parse_line(line: &str) -> ParsedLine {
    let parts: Vec<&str> = line.splitn(2, '#').collect();
    let content = parts[0].trim().to_string();
    let condition = parts.get(1).and_then(|c| {
        if c.trim().starts_with('[') && c.trim().ends_with(']') {
            Some(c.trim()[1..c.trim().len() - 1].trim().to_string())
        } else {
            None
        }
    });

    ParsedLine { content, condition }
}

fn evaluate_condition(
    condition: &str,
    env: &Environment,
    context: &BTreeMap<String, Value>,
) -> bool {
    if condition.is_empty() {
        return true;
    }
    println!("Evaluate condition: {}", condition);
    let template_str = format!("{{% if {} %}}true{{% else %}}false{{% endif %}}", condition);
    let template = env.template_from_str(&template_str).unwrap();

    template.render(context).unwrap() == "true"
}

/// Load an old-school conda_build_config.yaml file, and apply the selector_config to it
/// The parser supports only a small subset of (potential) conda_build_config.yaml features.
/// Especially, only `os.environ.get(...)` is supported.
pub fn load_conda_build_config(
    path: &Path,
    selector_config: &SelectorConfig,
) -> Result<VariantConfig, VariantConfigError> {
    // load the text, parse it and load as VariantConfig using serde_yaml

    let mut input = fs_err::read_to_string(path)
        .map_err(|e| VariantConfigError::IOError(path.to_path_buf(), e))?;

    let context = selector_config.clone().into_context();

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

    let mut lines = Vec::new();
    for line in input.lines() {
        let parsed = parse_line(line);
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
        VariantConfigError::IOError(
            path.to_path_buf(),
            std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        )
    })?;

    // filter all empty maps
    let value = value
        .as_mapping()
        .unwrap()
        .clone()
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .collect::<serde_yaml::Mapping>();

    let config: VariantConfig =
        serde_yaml::from_value(serde_yaml::Value::Mapping(value)).map_err(|e| {
            VariantConfigError::IOError(
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
        let parsed = parse_line("  - python\n");
        assert_eq!(parsed.content, "- python");
        assert_eq!(parsed.condition, None);

        let parsed = parse_line("  - python # [py3k]\n");
        assert_eq!(parsed.content, "- python");
        assert_eq!(parsed.condition, Some("py3k".to_string()));
    }

    #[test]
    fn test_evaluate_condition() {
        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(true));
        let env = Environment::new();
        assert_eq!(evaluate_condition("py3k", &env, &context), true);

        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(false));
        let env = Environment::new();
        assert_eq!(evaluate_condition("py3k", &env, &context), false);

        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(true));
        let env = Environment::new();
        assert_eq!(evaluate_condition("not py3k", &env, &context), false);

        let mut context = BTreeMap::new();
        context.insert("py3k".to_string(), Value::from(false));
        let env = Environment::new();
        assert_eq!(evaluate_condition("not py3k", &env, &context), true);
    }

    #[rstest]
    #[case("conda_build_config/test_1.yaml", None)]
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
            std::env::set_var("TEST_CF_CUDA_ENABLED", if cuda { "True" } else { "False" });
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
                    vec!["CF_CUDA_ENABLED".to_string()]
                );
            } else {
                assert_eq!(
                    config.variants[&"environment_var".into()],
                    vec!["CF_CUDA_DISABLED".to_string()]
                );
            }
            std::env::remove_var("TEST_CF_CUDA_ENABLED");
        }
    }

    fn test_data_dir() -> PathBuf {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/");
        return test_data_dir;
    }
}
