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
use std::collections::{HashSet, VecDeque};

use marked_yaml::Span;
use minijinja::machinery::{
    ast::{self, Expr, Stmt},
    parse,
};

use crate::recipe::{
    custom_yaml::{self, HasSpan, Node, ScalarNode, SequenceNodeInternal},
    parser::CollectErrors,
    ParsingError,
};

/// Extract all variables from a jinja statement
fn extract_variables(node: &Stmt, variables: &mut HashSet<String>) {
    match node {
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
                    if let Expr::Var(var) = &call.args[0] {
                        variables.insert(var.id.to_string());
                    }
                }
            }
        }
        Expr::IfExpr(ifexpr) => {
            extract_variable_from_expression(&ifexpr.test_expr, variables);
            extract_variable_from_expression(&ifexpr.true_expr, variables);
            if let Some(false_expr) = &ifexpr.false_expr {
                extract_variable_from_expression(false_expr, variables);
            }
        }
        _ => {}
    }
}

/// This recursively finds all `if/then/else` expressions in a YAML node
fn find_all_selectors<'a>(node: &'a Node, selectors: &mut HashSet<&'a ScalarNode>) {
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
                        selectors.insert(if_sel.cond());
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
fn find_jinja(
    node: &Node,
    src: &str,
    variables: &mut HashSet<String>,
) -> Result<(), Vec<ParsingError>> {
    let mut errs = Vec::<ParsingError>::new();
    let mut queue = VecDeque::from([(node, src)]);
    while let Some((node, src)) = queue.pop_front() {
        match node {
            Node::Mapping(map) => {
                for (_, value) in map.iter() {
                    queue.push_back((value, src));
                    // find_jinja(value, src, variables)?;
                }
            }
            Node::Sequence(seq) => {
                for item in seq.iter() {
                    match item {
                        SequenceNodeInternal::Simple(node) => queue.push_back((node, src)),
                        SequenceNodeInternal::Conditional(if_sel) => {
                            // we need to convert the if condition to a Jinja expression to parse it
                            let as_jinja_expr = format!("${{{{ {} }}}}", if_sel.cond().as_str());
                            match parse(&as_jinja_expr, "jinja.yaml") {
                                Ok(ast) => {
                                    extract_variables(&ast, variables);
                                    queue.push_back((if_sel.then(), src));
                                    // find_jinja(if_sel.then(), src, variables)?;
                                    if let Some(otherwise) = if_sel.otherwise() {
                                        queue.push_back((otherwise, src));
                                        // find_jinja(otherwise, src, variables)?;
                                    }
                                }
                                Err(err) => {
                                    let err = crate::recipe::ParsingError::from_partial(
                                        src,
                                        crate::_partialerror!(
                                            *if_sel.span(),
                                            crate::recipe::error::ErrorKind::from(err),
                                            label = "failed to parse as jinja expression"
                                        ),
                                    );
                                    errs.push(err);
                                }
                            }
                        }
                    }
                }
            }
            Node::Scalar(scalar) => {
                if scalar.contains("${{") {
                    match parse(scalar, "jinja.yaml") {
                        Ok(ast) => extract_variables(&ast, variables),
                        Err(err) => {
                            let err = crate::recipe::ParsingError::from_partial(
                                src,
                                crate::_partialerror!(
                                    *scalar.span(),
                                    crate::recipe::error::ErrorKind::from(err),
                                    label = "failed to parse as jinja expression"
                                ),
                            );
                            errs.push(err);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    Ok(())
}

fn variables_from_raw_expr(
    expr: &str,
    src: &str,
    span: &Span,
) -> Result<HashSet<String>, ParsingError> {
    let selector_tmpl = format!("${{{{ {} }}}}", expr);
    let ast = parse(&selector_tmpl, "selector.yaml").map_err(|e| -> ParsingError {
        ParsingError::from_partial(
            src,
            crate::_partialerror!(
                *span,
                crate::recipe::error::ErrorKind::from(e),
                label = "failed to parse as jinja expression"
            ),
        )
    })?;
    let mut variables = HashSet::new();
    extract_variables(&ast, &mut variables);
    Ok(variables)
}

fn variables_from_skip(
    root: &custom_yaml::Node,
    src: &str,
    variables: &mut HashSet<String>,
) -> Result<(), Vec<ParsingError>> {
    // find all variables from skip conditions in the recipe
    let skip = root
        .as_mapping()
        .and_then(|m| m.get("build"))
        .and_then(|m| m.as_mapping())
        .and_then(|m| m.get("skip"));

    let mut errors = Vec::new();
    match skip {
        Some(custom_yaml::Node::Sequence(node)) => {
            for item in node.iter() {
                if let SequenceNodeInternal::Simple(custom_yaml::Node::Scalar(scalar)) = item {
                    let vars = variables_from_raw_expr(scalar.as_str(), src, scalar.span());
                    match vars {
                        Ok(vars) => variables.extend(vars),
                        Err(err) => errors.push(err),
                    }
                }
            }
        }
        Some(custom_yaml::Node::Scalar(scalar)) => {
            let vars = variables_from_raw_expr(scalar.as_str(), src, scalar.span());
            match vars {
                Ok(vars) => variables.extend(vars),
                Err(err) => errors.push(err),
            }
        }
        _ => {}
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(())
}

/// This finds all variables used in jinja or `if/then/else` expressions
pub(crate) fn used_vars_from_expressions(
    yaml_node: &Node,
    src: &str,
) -> Result<HashSet<String>, Vec<ParsingError>> {
    let mut selectors = HashSet::new();

    find_all_selectors(yaml_node, &mut selectors);

    let mut variables = HashSet::new();

    selectors
        .iter()
        .map(|selector| -> Result<(), ParsingError> {
            let vars = variables_from_raw_expr(selector.as_str(), src, selector.span())?;
            variables.extend(vars);
            Ok(())
        })
        .collect_errors()?;

    // find all variables from skip conditions in the recipe
    variables_from_skip(yaml_node, src, &mut variables)?;

    // parse recipe into AST
    find_jinja(yaml_node, src, &mut variables)?;

    Ok(variables)
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

        let recipe_node = crate::recipe::custom_yaml::Node::parse_yaml(0, recipe).unwrap();
        let used_vars = used_vars_from_expressions(&recipe_node, recipe).unwrap();
        assert!(used_vars.contains("llvm_variant"));
        assert!(used_vars.contains("linux"));
        assert!(used_vars.contains("osx"));
        assert!(used_vars.contains("c_compiler"));
        assert!(used_vars.contains("c_compiler_version"));
        assert!(used_vars.contains("abcdef"));
    }

    #[test]
    fn test_conditional_compiler() {
        let recipe = r#"build:
            - ${{ compiler('c') if linux }}
            - ${{ bla if linux else foo }}
        "#;

        let recipe_node = crate::recipe::custom_yaml::Node::parse_yaml(0, recipe).unwrap();
        let used_vars = used_vars_from_expressions(&recipe_node, recipe).unwrap();
        assert!(used_vars.contains("c_compiler"));
        assert!(used_vars.contains("c_compiler_version"));
        assert!(used_vars.contains("bla"));
        assert!(used_vars.contains("foo"));
    }

    #[test]
    fn test_used_vars_from_expressions_with_skip() {
        let recipe = r#"build:
            skip:
              - llvm_variant > 10
              - linux
              - cuda
        "#;

        let recipe_node = crate::recipe::custom_yaml::Node::parse_yaml(0, recipe).unwrap();
        let used_vars = used_vars_from_expressions(&recipe_node, recipe).unwrap();
        assert!(used_vars.contains("llvm_variant"));
        assert!(used_vars.contains("cuda"));
        assert!(used_vars.contains("linux"));
        assert!(!used_vars.contains("osx"));
    }
}
