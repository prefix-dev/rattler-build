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
use std::collections::HashSet;

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
        _ => {}
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
                    }
                } else if function == "pin_subpackage" {
                    if let Expr::Const(constant) = &call.args[0] {
                        variables.insert(format!("{}", &constant.value));
                    }
                }
            }
        }
        _ => {}
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
pub(crate) fn used_vars_from_expressions(recipe: &str) -> HashSet<String> {
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

pub(crate) fn extract_dependencies(recipe: &YamlValue) -> HashSet<String> {
    // we do this in simple mode for now, but could later also do intersections
    // with the real matchspec (e.g. build variants for python 3.1-3.10, but recipe
    // says >=3.7 and then we only do 3.7-3.10)
    let mut dependencies = HashSet::<String>::new();

    if let Some(requirements) = recipe.get("requirements") {
        ["build", "host", "run", "constrains"]
            .iter()
            .for_each(|section| {
                if let Some(YamlValue::Sequence(section)) = requirements.get(section) {
                    for item in section {
                        if let YamlValue::String(item) = item {
                            if item.starts_with("{{") {
                                continue;
                            }
                            dependencies.insert(item.to_string());
                        }
                    }
                }
            });
    }

    dependencies
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_used_vars_from_expressions() {
        let recipe = r#"
        - sel(llvm_variant > 10): llvm >= 10
        - sel(linux): linux-gcc
        - sel(osx): osx-clang
        - "{{ compiler('c') }}"
        - "{{ pin_subpackage('abcdef') }}"
        "#;

        let used_vars = used_vars_from_expressions(recipe);
        assert!(used_vars.contains("llvm_variant"));
        assert!(used_vars.contains("linux"));
        assert!(used_vars.contains("osx"));
        assert!(used_vars.contains("c_compiler"));
        assert!(used_vars.contains("c_compiler_version"));
        assert!(used_vars.contains("abcdef"));
    }
}
