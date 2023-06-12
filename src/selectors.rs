use std::collections::{BTreeMap, HashMap};

use crate::render::jinja::jinja_environment;
use minijinja::value::Value;
use rattler_conda_types::Platform;
use serde_yaml::Value as YamlValue;

#[derive(Clone, Debug)]
pub struct SelectorConfig {
    pub target_platform: Platform,
    pub build_platform: Platform,
    pub variant: BTreeMap<String, String>,
}

impl SelectorConfig {
    pub fn into_context(self) -> HashMap<String, Value> {
        let mut context = HashMap::<String, Value>::new();

        context.insert(
            "target_platform".to_string(),
            Value::from_safe_string(self.target_platform.to_string()),
        );
        context.insert(
            "unix".to_string(),
            Value::from(self.target_platform.is_unix()),
        );
        context.insert(
            "win".to_string(),
            Value::from(self.target_platform.is_windows()),
        );
        context.insert(
            "osx".to_string(),
            Value::from(self.target_platform.is_osx()),
        );
        context.insert(
            "linux".to_string(),
            Value::from(self.target_platform.is_linux()),
        );
        let arch = self
            .target_platform
            .to_string()
            .split('-')
            .last()
            .unwrap()
            .to_string();

        let arch = match arch.as_str() {
            "64" => "x86_64",
            "32" => "x86",
            _ => &arch,
        };

        context.insert(arch.to_string(), Value::from(true));

        context.insert(
            "build_platform".to_string(),
            Value::from_safe_string(self.build_platform.to_string()),
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
    let env = jinja_environment();

    let selector = selector.into();

    // strip the sel() wrapper
    let expr = selector
        .strip_prefix("sel(")
        .and_then(|selector| selector.strip_suffix(')'))
        .expect("Could not strip sel( ... ). Check your brackets.");

    let expr = env.compile_expression(expr).unwrap();
    let ctx = selector_config.clone().into_context();
    let result = expr.eval(ctx).unwrap();
    result.is_true()
}

/// Flatten a YAML value, returning a new value with selectors evaluated and removed.
/// This is used in recipes to selectively include or exclude sections of the recipe.
/// For example, the following YAML:
///
/// ```yaml
/// requirements:
///   build:
///   - sel(unix): pkg-config
///   - sel(win): m2-pkg-config
/// ```
///
/// will be flattened to (if the selector config is `unix`):
///
/// ```yaml
/// requirements:
///  build:
///   - pkg-config
/// ```
///
/// Nested lists are supported as well, so the following YAML:
///
/// ```yaml
/// requirements:
///   build:
///   - sel(unix):
///     - pkg-config
///     - libtool
///   - sel(win):
///     - m2-pkg-config
/// ```
///
/// will be flattened to (if the selector config is `unix`):
///
/// ```yaml
/// requirements:
///   build:
///   - pkg-config
///   - libtool
/// ```
pub fn flatten_selectors(
    val: &mut YamlValue,
    selector_config: &SelectorConfig,
) -> Option<YamlValue> {
    if val.is_string() || val.is_number() || val.is_bool() {
        return Some(val.clone());
    }

    if val.is_mapping() {
        let only_selectors = val.as_mapping().unwrap().iter().all(|(k, _)| {
            if let YamlValue::String(key) = k {
                key.starts_with("sel(")
            } else {
                false
            }
        });

        if only_selectors {
            for (k, v) in val.as_mapping_mut().unwrap().iter_mut() {
                if let YamlValue::String(key) = k {
                    if eval_selector(key, selector_config) {
                        return flatten_selectors(v, selector_config);
                    }
                }
            }
            return None;
        }

        for (k, v) in val.as_mapping_mut().unwrap().iter_mut() {
            if let YamlValue::String(key) = k {
                if key.starts_with("sel(") {
                    panic!(
                        "Cannot mix selector dictionary with other keys in: {:?}",
                        val
                    );
                }
            }
            let res = flatten_selectors(v, selector_config);
            *v = res.unwrap_or_else(|| YamlValue::Null);
        }
    }

    if val.is_sequence() {
        let new_val: Vec<YamlValue> = val
            .as_sequence_mut()
            .unwrap()
            .iter_mut()
            .filter_map(|el| flatten_selectors(el, selector_config))
            .collect();

        // This does not yet work for lists of list with selectors (it flattens them)
        // This is relevant for zip_keys, which is a list of lists of strings.
        if new_val.iter().ne(val.as_sequence().unwrap().iter()) {
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

            return Some(serde_yaml::to_value(new_val).unwrap());
        }

        return Some(val.clone());
    }

    Some(val.clone())
}

/// Flatten a YAML top-level mapping, returning a new mapping with all selectors.
/// This is particularly useful for variant configuration files. For example,
/// the following YAML:
///
/// ```yaml
/// sel(unix):
///  compiler: gcc
///  compiler_version: 7.5.0
/// sel(win):
///  compiler: msvc
///  compiler_version: 14.2
/// ```
///
/// will be flattened to (if the selector config is `unix`):
///
/// ```yaml
/// compiler: gcc
/// compiler_version: 7.5.0
/// ```
pub fn flatten_toplevel(
    val: &mut YamlValue,
    selector_config: &SelectorConfig,
) -> Option<YamlValue> {
    if val.is_mapping() {
        let mut new_val = BTreeMap::<String, YamlValue>::new();
        for (k, v) in val.as_mapping_mut().unwrap().iter_mut() {
            if let YamlValue::String(key) = k {
                if key.starts_with("sel(") {
                    if eval_selector(key, selector_config) {
                        if let Some(inner_map) = flatten_selectors(v, selector_config) {
                            for (k, v) in inner_map.as_mapping().unwrap().iter() {
                                new_val.insert(k.as_str().unwrap().to_string(), v.clone());
                            }
                        }
                    }
                } else {
                    new_val.insert(key.clone(), flatten_selectors(v, selector_config).unwrap());
                }
            } else {
                tracing::error!("Variant config key is not a string: {:?}. Ignoring", k);
            }
        }
        Some(serde_yaml::to_value(new_val).unwrap())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn test_eval_selector() {
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant: Default::default(),
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
        assert!(eval_selector("sel(linux and x86_64)", &selector_config));
        assert!(!eval_selector("sel(linux and aarch64)", &selector_config));
    }

    #[test]
    fn test_cmp() {
        let mut variant = BTreeMap::new();
        variant.insert("python".to_string(), "3.7".to_string());
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant,
        };

        assert!(eval_selector("sel(cmp(python, '==3.7'))", &selector_config));
        assert!(eval_selector("sel(cmp(python, '>=3.7'))", &selector_config));
        assert!(eval_selector(
            "sel(cmp(python, '>=3.7,<3.9'))",
            &selector_config
        ));

        assert!(!eval_selector(
            "sel(cmp(python, '!=3.7'))",
            &selector_config
        ));
        assert!(!eval_selector("sel(cmp(python, '<3.7'))", &selector_config));
        assert!(!eval_selector(
            "sel(cmp(python, '>3.5,<3.7'))",
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
    #[case("selectors/flatten_3.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();
        let selector_config = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant: Default::default(),
        };

        let res = flatten_selectors(&mut yaml, &selector_config);
        set_snapshot_suffix!("{}", filename.replace('/', "_"));
        insta::assert_yaml_snapshot!(res);
    }
}
