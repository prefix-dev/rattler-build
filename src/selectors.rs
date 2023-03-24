use std::collections::HashMap;
use std::panic;

use serde_yaml::Value as YamlValue;

use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::Value;

#[derive(Clone, Debug)]
pub struct SelectorConfig {
    pub python_version: String,
    pub target_platform: String,
    pub build_platform: String,
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

    pub fn into_globals(self) -> GlobalsBuilder {
        let mut globals = GlobalsBuilder::standard();
        globals.set("unix", self.is_unix());
        globals.set("win", self.is_win());
        globals.set("osx", self.is_osx());
        globals.set("linux", self.is_linux());

        globals.set("arch", self.target_platform.split('-').last().unwrap_or("unknown"));
        globals.set("python_version", self.python_version);
        globals.set("target_platform", self.target_platform);
        globals.set("build_platform", self.build_platform);
        for (key, v) in std::env::vars() {
            globals.set(&key, v);
        }
        globals
    }
}

pub fn eval_selector<S: Into<String>>(selector: S, selector_config: &SelectorConfig) -> bool {
    let selector = selector.into();

    // strip the sel() wrapper
    let selector = selector
        .strip_prefix("sel(")
        .and_then(|selector| selector.strip_suffix(')'))
        .expect("Could not strip sel( ... ). Check your brackets.")
        .into();


    // We first parse the content, giving a filename and the Starlark
    // `Dialect` we'd like to use (we pick standard).
    let ast: AstModule = AstModule::parse("selector.star", selector, &Dialect::Standard).unwrap();

    // We create a `Globals`, defining the standard library functions available.
    // The `standard` function uses those defined in the Starlark specification.
    let globals = selector_config.clone().into_globals().build();

    // We create a `Module`, which stores the global variables for our calculation.
    let module: Module = Module::new();

    // We create an evaluator, which controls how evaluation occurs.
    let mut eval: Evaluator = Evaluator::new(&module);

    // And finally we evaluate the code using the evaluator.
    let res: Value = eval.eval_module(ast, &globals).expect("huuuuh?");
    res.unpack_bool().unwrap_or(false)
}

pub fn flatten_selectors(val: &mut YamlValue, selector_config: &SelectorConfig) -> Option<YamlValue> {
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
        let new_val = val.as_sequence_mut().unwrap()
            .iter_mut()
            .map(|el| flatten_selectors(el, selector_config))
            .filter(|el| el.is_some())
            .map(|el| el.unwrap())
            .collect::<Vec<YamlValue>>();

        // flatten down list of lists
        let new_val =  new_val
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

    return Some(val.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn test_eval_selector() {
        let selector_config = SelectorConfig {
            python_version: "3.8.5".into(),
            target_platform: "linux-64".into(),
            build_platform: "linux-64".into(),
        };
        assert!(eval_selector("sel(unix)", &selector_config));
        assert!(!eval_selector("sel(win)", &selector_config));
        assert!(!eval_selector("sel(osx)", &selector_config));
        assert!(eval_selector("sel(unix and not win)", &selector_config));
        assert!(!eval_selector("sel(unix and not linux)", &selector_config));
        assert!(eval_selector("sel((unix and not osx) or win)", &selector_config));
        assert!(eval_selector("sel((unix and not osx) or win or osx)", &selector_config));
    }

    #[rstest]
    #[case("selectors/flatten_1.yaml")]
    #[case("selectors/flatten_2.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();
        let selector_config = SelectorConfig {
            python_version: "3.8.5".into(),
            target_platform: "linux-64".into(),
            build_platform: "linux-64".into(),
        };

        let res = flatten_selectors(&mut yaml, &selector_config);

        insta::assert_yaml_snapshot!(res);
    }
}