use serde_yaml::Value as YamlValue;

use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::Value;

pub fn eval_selector<S: Into<String>>(selector: S) -> bool {
    let selector = selector.into();

    let selector = selector
        .strip_prefix("sel(")
        .and_then(|selector| selector.strip_suffix(')'))
        .expect("Could not strip sel( ... ). Check your brackets.")
        .into();

    // We first parse the content, giving a filename and the Starlark
    // `Dialect` we'd like to use (we pick standard).
    let ast: AstModule =
        AstModule::parse("hello_world.star", selector, &Dialect::Standard).unwrap();

    // We create a `Globals`, defining the standard library functions available.
    // The `standard` function uses those defined in the Starlark specification.
    let mut globals = GlobalsBuilder::standard();
    globals.set("unix", true);
    globals.set("win", false);
    globals.set("osx", false);
    for (key, v) in std::env::vars() {
        globals.set(&key, v);
    }
    let globals = globals.build();
    // We create a `Module`, which stores the global variables for our calculation.
    let module: Module = Module::new();

    // We create an evaluator, which controls how evaluation occurs.
    let mut eval: Evaluator = Evaluator::new(&module);

    // And finally we evaluate the code using the evaluator.
    let res: Value = eval.eval_module(ast, &globals).expect("huuuuh?");
    return res.unpack_bool().unwrap_or(false);
}

pub fn flatten_selectors(val: &YamlValue) -> Option<YamlValue> {
    println!("Flattening {:?}", val);
    if val.is_string() || val.is_number() || val.is_bool() {
        println!("Flattening string stuff");
        return Some(val.clone());
    }
    if val.is_mapping() {
        println!("Flattening Map stuff");
        for (k, v) in val.as_mapping()? {
            if let YamlValue::String(key) = k {
                if key.starts_with("sel(") {
                    if eval_selector(key) {
                        flatten_selectors(v);
                    } else {
                        return None;
                    }
                } else {
                    flatten_selectors(v);
                }
            } else {
                flatten_selectors(v);
                // v = flatten_selectors(val).unwrap();
            }
        }
    }
    if val.is_sequence() {
        println!("Flattening List stuff");
        let mut to_delete: Vec<usize> = Vec::new();
        let seq = val.as_sequence().unwrap();
        for (idx, el) in seq.iter().enumerate() {
            let res = flatten_selectors(el);
            if res.is_some() {
                // seq[idx] = res.expect("Some");
            } else {
                to_delete.push(idx);
            }
        }
        for el in to_delete.into_iter().rev() {
            println!("Would like to delete {}", el);
            // seq.remove(el);
        }
        // val = seq;
    }
    return None;
}
