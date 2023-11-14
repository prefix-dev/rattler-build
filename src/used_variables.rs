//! find used variables on a Raw (YAML) recipe
//! This does an initial "prerender" step where we evaluate the Jinja expressions globally
//! based on the variables in the `context` section of the recipe.
//! This also evaluates any Jinja functions such as `compiler` and `pin_subpackage` in a way
//! that we can post-process them as "used variables" more easily later.
//!
//! Step 1:
//!    - use only outer variables such as `target_platform`
//!    - extract all `if ... then ... else ` and `jinja` statements and find used variables
//!    - retrieve used variables from configuration and flatten selectors
//!    - extract all dependencies and add them to used variables to build full variant
use std::collections::HashSet;

use minijinja::machinery::{
    ast::{self, Expr, Stmt},
    parse,
};

use crate::recipe::custom_yaml::Node;

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
                } else if function == "cdt" {
                    variables.insert("cdt_name".into());
                    variables.insert("cdt_arch".into());
                } else if function == "cmp" {
                    if let Expr::Const(constant) = &call.args[0] {
                        variables.insert(constant.value.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

/// This recursively finds all `if/then/else` expressions in a YAML node
fn find_all_selectors(node: &Node, selectors: &mut HashSet<String>) {
    use crate::recipe::custom_yaml::SequenceNodeInternal;

    match node {
        Node::Mapping(map) => {
            for (_, value) in map.iter() {
                find_all_selectors(value, selectors);
            }
        }
        Node::Sequence(seq) => {
            for item in seq.iter() {
                match item {
                    SequenceNodeInternal::Simple(node) => find_all_selectors(node, selectors),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        selectors.insert(if_sel.cond().as_str().to_owned());
                        find_all_selectors(if_sel.then(), selectors);
                        if let Some(otherwise) = if_sel.otherwise() {
                            find_all_selectors(otherwise, selectors);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

// find all scalar nodes and Jinja expressions
fn find_jinja(node: &Node, variables: &mut HashSet<String>) {
    use crate::recipe::custom_yaml::SequenceNodeInternal;

    match node {
        Node::Mapping(map) => {
            for (_, value) in map.iter() {
                find_jinja(value, variables);
            }
        }
        Node::Sequence(seq) => {
            for item in seq.iter() {
                match item {
                    SequenceNodeInternal::Simple(node) => find_jinja(node, variables),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        if if_sel.cond().contains("${{") {
                            let ast = parse(if_sel.cond(), "jinja.yaml").unwrap();
                            extract_variables(&ast, variables);
                        }

                        find_jinja(if_sel.then(), variables);
                        if let Some(otherwise) = if_sel.otherwise() {
                            find_jinja(otherwise, variables);
                        }
                    }
                }
            }
        }
        Node::Scalar(scalar) => {
            if scalar.contains("${{") {
                let ast = parse(scalar, "jinja.yaml").unwrap();
                extract_variables(&ast, variables);
            }
        }
        _ => {}
    }
}

/// This finds all variables used in jinja or `if/then/else` expressions
pub(crate) fn used_vars_from_expressions(yaml_node: &Node) -> HashSet<String> {
    let mut selectors = HashSet::new();

    find_all_selectors(yaml_node, &mut selectors);

    let mut variables = HashSet::new();

    for selector in selectors {
        let selector_tmpl = format!("{{{{ {} }}}}", selector);
        let ast = parse(&selector_tmpl, "selector.yaml").unwrap();
        extract_variables(&ast, &mut variables);
    }

    // parse recipe into AST
    find_jinja(yaml_node, &mut variables);

    variables
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_used_vars_from_expressions() {
        let recipe = r#"build:
            - if: llvm_variant > 10
              then: llvm >= 10
            - if: linux
              then: linux-gcc
            - if: osx
              then: osx-clang
            - ${{ compiler('c') }}
            - ${{ pin_subpackage('abcdef') }}
        "#;

        let recipe = crate::recipe::custom_yaml::Node::parse_yaml(0, recipe).unwrap();
        let used_vars = used_vars_from_expressions(&recipe);
        assert!(used_vars.contains("llvm_variant"));
        assert!(used_vars.contains("linux"));
        assert!(used_vars.contains("osx"));
        assert!(used_vars.contains("c_compiler"));
        assert!(used_vars.contains("c_compiler_version"));
        assert!(used_vars.contains("abcdef"));
    }
}
