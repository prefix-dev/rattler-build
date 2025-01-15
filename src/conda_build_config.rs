//! Module to load deprecated `conda_build_config.yaml` files and apply the selector_config to it
use std::{collections::BTreeMap, io::Read};

use miette::IntoDiagnostic;
use minijinja::{Environment, Value};

use crate::{selectors::SelectorConfig, variant_config::VariantConfig};

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
pub fn load_conda_build_config(
    r: impl Read,
    selector_config: &SelectorConfig,
) -> miette::Result<VariantConfig> {
    // load the text, parse it and load as VariantConfig using serde_yaml
    let mut config = VariantConfig::default();

    let mut input = String::new();
    std::io::BufReader::new(r)
        .read_to_string(&mut input)
        .unwrap();
    let context = selector_config.clone().into_context();
    let mut env = Environment::new();
    // env.set_context(context);
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let parsed = parse_line(&line);
        if let Some(condition) = &parsed.condition {
            if evaluate_condition(condition, &env, &context) {
                out.push_str(&parsed.content);
            }
        } else {
            out.push_str(&parsed.content);
        }
        out.push('\n');
    }

    config = serde_yaml::from_str(&out).into_diagnostic()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

    #[test]
    fn test_conda_forge() {
        let path = test_data_dir().join("conda_build_config/test_1.yaml");
        let file = std::fs::File::open(path).unwrap();
        let selector_config = SelectorConfig::default();
        let config = load_conda_build_config(file, &selector_config).unwrap();
        insta::assert_yaml_snapshot!(config);
    }

    fn test_data_dir() -> PathBuf {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/");
        return test_data_dir;
    }
}
