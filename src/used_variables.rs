//! find used variabels on a Raw (YAML) recipe
//! This does an initial "prerender" step where we evaluate the Jinja expressions globally
//! based on the variables in the `context` section of the recipe.
//! This also evaluates any Jinja functions such as `compiler` and `pin_subpackage` in a way
//! that we can post-process them as "used variables" more easily later.
//!
//! Step 1:
//!    - use only outer variables such as `target_platform`
//!    - extract all sel( ... ) and `jinja` statements and find used variables
//!    - retrieve used variabels from configuration and flatten selectors
//!    - extract all dependencies and add them to used variables to build full variant

use minijinja::machinery::{
    ast::{self, Expr, Stmt},
    parse,
};
use serde_yaml::Value as YamlValue;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::selectors::{flatten_selectors, SelectorConfig};

/// Extract all variables from a jinja statement
fn extract_variables(node: &Stmt, variables: &mut HashSet<String>) {
    match node {
        Stmt::IfCond(stmt) => {
            extract_variable_from_expression(&stmt.expr, variables);
        }
        Stmt::Template(stmt) => {
            stmt.children.iter().for_each(|child| {
                extract_variables(child, variables);
            });
        }
        Stmt::EmitExpr(expr) => {
            extract_variable_from_expression(&expr.expr, variables);
        }
        _ => {
            // println!("Received {:?}", node);
        }
    }
}

/// Extract all variables from a jinja expression (called from [`extract_variables`])
fn extract_variable_from_expression(expr: &Expr, variables: &mut HashSet<String>) {
    match expr {
        Expr::Var(var) => {
            variables.insert(var.id.into());
        }
        Expr::BinOp(binop) => {
            extract_variable_from_expression(&binop.left, variables);
            extract_variable_from_expression(&binop.right, variables);
        }
        Expr::UnaryOp(unaryop) => {
            extract_variable_from_expression(&unaryop.expr, variables);
        }
        Expr::Filter(filter) => {
            if let Some(expr) = &filter.expr {
                extract_variable_from_expression(expr, variables);
            }
            for arg in &filter.args {
                extract_variable_from_expression(arg, variables);
            }
        }
        Expr::Call(call) => {
            if let ast::CallType::Function(function) = call.identify_call() {
                if function == "compiler" {
                    if let Expr::Const(constant) = &call.args[0] {
                        variables.insert(format!("{}_compiler", &constant.value));
                        variables.insert(format!("{}_compiler_version", &constant.value));
                    } else {
                    }
                }
            }
        }
        _ => {
            // println!("Received {:?}", expr)
        }
    }
}

/// This recursively finds all `sel(...)` expressions in a YAML node
fn find_all_selectors(node: &YamlValue, selectors: &mut HashSet<String>) {
    match node {
        YamlValue::Mapping(map) => {
            for (key, value) in map {
                if let YamlValue::String(key) = key {
                    if key.starts_with("sel(") {
                        selectors.insert(key[4..key.len() - 1].to_string());
                    }
                }
                find_all_selectors(value, selectors);
            }
        }
        YamlValue::Sequence(seq) => {
            for item in seq {
                find_all_selectors(item, selectors);
            }
        }
        _ => {}
    }
}

/// This finds all variables used in jinja or `sel(...)` expressions
fn used_vars_from_jinja(recipe: &str) -> HashSet<String> {
    // regex replace all `sel(...)` with `{{ ... }}` to turn them into jinja expressions
    let mut selectors = HashSet::new();
    let yaml_node = serde_yaml::from_str(recipe).unwrap();
    find_all_selectors(&yaml_node, &mut selectors);

    let mut variables = HashSet::new();

    for selector in selectors {
        let selector_tmpl = format!("{{{{ {} }}}}", selector);
        let ast = parse(&selector_tmpl, "selector.yaml").unwrap();
        extract_variables(&ast, &mut variables);
    }

    // parse recipe into AST
    let template_ast = parse(recipe, "recipe.yaml").unwrap();

    // extract all variables from the AST
    extract_variables(&template_ast, &mut variables);

    variables
}

fn extract_dependencies(recipe: &YamlValue) -> HashSet<String> {
    // we do this in simple mode for now, but could later also do intersections
    // with the real matchspec (e.g. build variants for python 3.1-3.10, but recipe
    // says >=3.7 and then we only do 3.7-3.10)
    let mut dependencies = HashSet::<String>::new();

    if let Some(requirements) = recipe.get("requirements") {
        ["build", "host", "run"].iter().for_each(|section| {
            if let Some(YamlValue::Sequence(section)) = requirements.get(section) {
                for item in section {
                    if let YamlValue::String(item) = item {
                        dependencies.insert(item.to_string());
                    }
                }
            }
        });
    }

    dependencies
}

fn find_combinations(
    map: &HashMap<String, Vec<String>>,
    keys: &[String],
    index: usize,
    current: Vec<(String, String)>,
    result: &mut Vec<BTreeMap<String, String>>,
) {
    if index == keys.len() {
        result.push(current.into_iter().collect());
        return;
    }

    let key = &keys[index];
    let values = map.get(key).unwrap();

    for value in values {
        let mut next = current.clone();
        next.push((key.clone(), value.clone()));
        find_combinations(map, keys, index + 1, next, result);
    }
}

/// This finds all used variables in any dependency declarations, build, host, and run sections.
/// As well as any used variables from Jinja functions to calculate the variants of this recipe.
pub fn find_variants(
    recipe: &str,
    config: &BTreeMap<String, Vec<String>>,
    selector_config: &SelectorConfig,
) -> Vec<BTreeMap<String, String>> {
    let used_variables = used_vars_from_jinja(recipe);

    // now render all selectors with the used variables
    let mut variants = HashMap::new();

    for var in &used_variables {
        if let Some(value) = config.get(var) {
            variants.insert(var.clone(), value.clone());
        }
    }

    let mut combinations = Vec::new();
    let keys: Vec<String> = variants.keys().cloned().collect();
    find_combinations(&variants, &keys, 0, vec![], &mut combinations);

    let recipe_parsed: YamlValue = serde_yaml::from_str(recipe).unwrap();
    for _variant in combinations {
        let mut val = recipe_parsed.clone();
        if let Some(flattened_recipe) = flatten_selectors(&mut val, selector_config) {
            // extract all dependencies from the flattened recipe
            let dependencies = extract_dependencies(&flattened_recipe);
            for dependency in dependencies {
                if let Some(value) = config.get(&dependency) {
                    variants.insert(dependency, value.clone());
                }
            }
        };
    }

    let mut combinations = Vec::new();
    find_combinations(&variants, &keys, 0, vec![], &mut combinations);
    combinations
}
