use std::collections::{BTreeMap, HashMap};

use minijinja::value::Value;
use minijinja::Environment;
use serde_yaml::Value as YamlValue;

#[derive(Clone, Debug)]
pub struct SelectorConfig {
    pub target_platform: String,
    pub build_platform: String,
    pub variant: BTreeMap<String, String>,
}

impl SelectorConfig {
    fn is_unix(&self) -> bool {
        !self.target_platform.starts_with("win-")
    }

    fn is_win(&self) -> bool {
        self.target_platform.starts_with("win-")
    }

    fn is_osx(&self) -> bool {
        self.target_platform.starts_with("osx-")
    }

    fn is_linux(&self) -> bool {
        self.target_platform.starts_with("linux-")
    }
    pub fn into_context(self) -> HashMap<String, Value> {
        let mut context = HashMap::<String, Value>::new();
        context.insert("unix".to_string(), Value::from(self.is_unix()));
        context.insert("win".to_string(), Value::from(self.is_win()));
        context.insert("osx".to_string(), Value::from(self.is_osx()));
        context.insert("linux".to_string(), Value::from(self.is_linux()));

        context.insert(
            "arch".to_string(),
            Value::from_safe_string(
                self.target_platform
                    .split('-')
                    .last()
                    .unwrap_or("unknown")
                    .to_string(),
            ),
        );
        context.insert(
            "target_platform".to_string(),
            Value::from_safe_string(self.target_platform),
        );
        context.insert(
            "build_platform".to_string(),
            Value::from_safe_string(self.build_platform),
        );
        // for (key, v) in std::env::vars() {
        //     context.insert(key, Value::from_safe_string(v));
        // }

        for (key, v) in self.variant {
            context.insert(key, Value::from_safe_string(v));
        }

        context
    }
}

pub fn eval_selector<S: Into<String>>(selector: S, selector_config: &SelectorConfig) -> bool {
    let env = Environment::new();

    let selector = selector.into();

    // strip the sel() wrapper
    let selector = selector
        .strip_prefix("sel(")
        .and_then(|selector| selector.strip_suffix(')'))
        .expect("Could not strip sel( ... ). Check your brackets.");

    let expr = env.compile_expression(selector).unwrap();
    let ctx = selector_config.clone().into_context();
    let result = expr.eval(ctx).unwrap();
    result.is_true()
}

pub fn flatten_selectors(
    val: &mut YamlValue,
    selector_config: &SelectorConfig,
) -> Option<YamlValue> {
    if val.is_string() || val.is_number() || val.is_bool() {
        return Some(val.clone());
    }

    if val.is_mapping() {
        for (k, v) in val.as_mapping_mut().unwrap().iter_mut() {
            if let YamlValue::String(key) = k {
                if key.starts_with("sel(") {
                    if eval_selector(key, selector_config) {
                        return flatten_selectors(v, selector_config);
                    } else {
                        return None;
                    }
                } else {
                    *v = flatten_selectors(v, selector_config).unwrap();
                }
            }
        }
    }

    if val.is_sequence() {
        let new_val = val
            .as_sequence_mut()
            .unwrap()
            .iter_mut()
            .filter_map(|el| flatten_selectors(el, selector_config));

        // flatten down list of lists
        let new_val = new_val
            .into_iter()
            .flat_map(|el| {
                if el.is_sequence() {
                    el.as_sequence().unwrap().clone()
                } else {
                    vec![el]
                }
            })
            .collect::<Vec<_>>();

        return Some(serde_yaml::to_value(&new_val).unwrap());
    }

    Some(val.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn test_eval_selector() {
        let selector_config = SelectorConfig {
            target_platform: "linux-64".into(),
            build_platform: "linux-64".into(),
            variant: vec![("python_version".into(), "3.8.5".into())]
                .into_iter()
                .collect(),
        };
        assert!(eval_selector("sel(unix)", &selector_config));
        assert!(!eval_selector("sel(win)", &selector_config));
        assert!(!eval_selector("sel(osx)", &selector_config));
        assert!(eval_selector("sel(unix and not win)", &selector_config));
        assert!(!eval_selector("sel(unix and not linux)", &selector_config));
        assert!(eval_selector(
            "sel((unix and not osx) or win)",
            &selector_config
        ));
        assert!(eval_selector(
            "sel((unix and not osx) or win or osx)",
            &selector_config
        ));
    }

    macro_rules! set_snapshot_suffix {
        ($($expr:expr),*) => {
            let mut settings = insta::Settings::clone_current();
            settings.set_snapshot_suffix(format!($($expr,)*));
            let _guard = settings.bind_to_scope();
        }
    }

    #[rstest]
    #[case("selectors/flatten_1.yaml")]
    #[case("selectors/flatten_2.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();
        let selector_config = SelectorConfig {
            target_platform: "linux-64".into(),
            build_platform: "linux-64".into(),
            variant: vec![("python_version".into(), "3.8.5".into())]
                .into_iter()
                .collect(),
        };

        let res = flatten_selectors(&mut yaml, &selector_config);
        set_snapshot_suffix!("{}", filename.replace('/', "_"));
        insta::assert_yaml_snapshot!(res);
    }

    // #[test]
    // fn test_config_selectors() {
    //     let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
    //     let yaml_file =
    //         std::fs::read_to_string(test_data_dir.join("selectors/config_1.yaml")).unwrap();

    //     let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();
    //     let selector_config = SelectorConfig {
    //         target_platform: "linux-64".into(),
    //         build_platform: "win-64".into(),
    //         variant: vec![("python_version".into(), "3.8.5".into())]
    //             .into_iter()
    //             .collect(),
    //     };

    //     let res = flatten_selectors(&mut yaml, &selector_config);
    //     // set_snapshot_suffix!("{}", filename.replace('/', "_"));
    //     insta::assert_yaml_snapshot!(res);
    // }
}
