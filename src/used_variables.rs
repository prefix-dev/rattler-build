//! Find used variables on a Raw (YAML) recipe
//! This does an initial "prerender" step where we evaluate the Jinja
//! expressions globally based on the variables in the `context` section of the
//! recipe. This also evaluates any Jinja functions such as `compiler` and
//! `pin_subpackage` in a way that we can post-process them as "used variables"
//! more easily later.
//!
//! Step 1:
//!    - use only outer variables such as `target_platform`
//!    - extract all `if ... then ... else ` and `jinja` statements and find
//!      used variables
//!    - retrieve used variables from configuration and flatten selectors
//!    - extract all dependencies and add them to used variables to build full
//!      variant
use std::collections::{HashSet, VecDeque};

use marked_yaml::Span;
use minijinja::machinery::{
    WhitespaceConfig,
    ast::{self, CallArg, Expr, Stmt},
    parse_expr,
};

use crate::recipe::{
    ParsingError,
    custom_yaml::{self, HasSpan, Node, ScalarNode, SequenceNodeInternal},
    jinja::SYNTAX_CONFIG,
    parser::CollectErrors,
};
use crate::source_code::SourceCode;

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

fn parse<'source>(
    expr: &'source str,
    filename: &'source str,
) -> Result<ast::Stmt<'source>, minijinja::Error> {
    minijinja::machinery::parse(
        expr,
        filename,
        SYNTAX_CONFIG.clone(),
        WhitespaceConfig::default(),
    )
}

fn get_pos_expr<'a>(call_args: &'a [CallArg<'a>], idx: usize) -> Option<&'a Expr<'a>> {
    if idx < call_args.len() {
        match &call_args[idx] {
            CallArg::Pos(expr) => Some(expr),
            _ => None,
        }
    } else {
        None
    }
}

/// Extract all variables from a jinja expression (called from
/// [`extract_variables`])
fn extract_variable_from_expression(expr: &Expr, variables: &mut HashSet<String>) {
    match expr {
        Expr::Var(var) => {
            variables.insert(var.id.into());
        }
        Expr::Test(test) => {
            extract_variable_from_expression(&test.expr, variables);
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
            // for arg in &filter.args {
            //     extract_variable_from_expression(arg, variables);
            // }
        }
        Expr::Call(call) => {
            if let ast::CallType::Function(function) = call.identify_call() {
                let Some(arg) = get_pos_expr(&call.args, 0) else {
                    return;
                };
                if function == "compiler" {
                    if let Expr::Const(constant) = arg {
                        variables.insert(format!("{}_compiler", &constant.value));
                        variables.insert(format!("{}_compiler_version", &constant.value));
                    }
                } else if function == "stdlib" {
                    if let Expr::Const(constant) = arg {
                        variables.insert(format!("{}_stdlib", &constant.value));
                        variables.insert(format!("{}_stdlib_version", &constant.value));
                    }
                } else if function == "pin_subpackage" || function == "pin_compatible" {
                    if !call.args.is_empty() {
                        extract_variable_from_expression(arg, variables);
                    }
                } else if function == "cdt" {
                    variables.insert("cdt_name".into());
                    variables.insert("cdt_arch".into());
                } else if function == "match" {
                    extract_variable_from_expression(arg, variables);
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
fn find_jinja<S: SourceCode>(
    node: &Node,
    src: S,
    variables: &mut HashSet<String>,
) -> Result<(), Vec<ParsingError<S>>> {
    let mut errs = Vec::<ParsingError<S>>::new();
    let mut queue = VecDeque::from([node]);

    while let Some(node) = queue.pop_front() {
        match node {
            Node::Mapping(map) => {
                for (_, value) in map.iter() {
                    queue.push_back(value);
                }
            }
            Node::Sequence(seq) => {
                for item in seq.iter() {
                    match item {
                        SequenceNodeInternal::Simple(node) => queue.push_back(node),
                        SequenceNodeInternal::Conditional(if_sel) => {
                            match parse_expr(if_sel.cond().as_str()) {
                                Ok(expr) => {
                                    extract_variable_from_expression(&expr, variables);
                                    queue.push_back(if_sel.then());

                                    if let Some(otherwise) = if_sel.otherwise() {
                                        queue.push_back(otherwise);
                                    }
                                }
                                Err(err) => {
                                    let err = crate::recipe::ParsingError::from_partial(
                                        src.clone(),
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
                                src.clone(),
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

fn variables_from_raw_expr<S: SourceCode>(
    expr: &str,
    src: S,
    span: &Span,
) -> Result<HashSet<String>, ParsingError<S>> {
    let selector_tmpl = format!("${{{{ {} }}}}", expr);
    let ast = parse(&selector_tmpl, "selector.yaml").map_err(|e| {
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

fn variables_from_skip<S: SourceCode>(
    root: &custom_yaml::Node,
    src: S,
    variables: &mut HashSet<String>,
) -> Result<(), Vec<ParsingError<S>>> {
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
                    let vars = variables_from_raw_expr(scalar.as_str(), src.clone(), scalar.span());
                    match vars {
                        Ok(vars) => variables.extend(vars),
                        Err(err) => errors.push(err),
                    }
                }
            }
        }
        Some(custom_yaml::Node::Scalar(scalar)) => {
            let vars = variables_from_raw_expr(scalar.as_str(), src.clone(), scalar.span());
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
pub(crate) fn used_vars_from_expressions<S: SourceCode>(
    yaml_node: &Node,
    src: S,
) -> Result<HashSet<String>, Vec<ParsingError<S>>> {
    let mut selectors = HashSet::new();

    find_all_selectors(yaml_node, &mut selectors);

    let mut variables = HashSet::new();

    selectors
        .iter()
        .map(|selector| -> Result<(), ParsingError<S>> {
            let vars = variables_from_raw_expr(selector.as_str(), src.clone(), selector.span())?;
            variables.extend(vars);
            Ok(())
        })
        .collect_errors()?;

    // find all variables from skip conditions in the recipe
    variables_from_skip(yaml_node, src.clone(), &mut variables)?;

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
            - ${{ stdlib('c') }}
            - ${{ pin_subpackage(abcdef) }}
            - ${{ pin_subpackage("foobar") }}
            - ${{ pin_compatible(compatible) }}
            - ${{ pin_compatible(abc ~ def) }}
            - if: match(xpython, ">=3.7")
              then: numpy 100
            - ${{ testexprvar is string }}
        "#;

        let recipe_node = crate::recipe::custom_yaml::Node::parse_yaml(0, recipe).unwrap();
        let used_vars = used_vars_from_expressions(&recipe_node, recipe).unwrap();
        assert!(used_vars.contains("llvm_variant"));
        assert!(used_vars.contains("linux"));
        assert!(used_vars.contains("osx"));
        assert!(used_vars.contains("c_compiler"));
        assert!(used_vars.contains("c_compiler_version"));
        assert!(used_vars.contains("c_stdlib"));
        assert!(used_vars.contains("c_stdlib_version"));
        assert!(used_vars.contains("abcdef"));
        assert!(!used_vars.contains("foobar"));
        assert!(used_vars.contains("compatible"));
        assert!(used_vars.contains("abc"));
        assert!(used_vars.contains("def"));
        assert!(used_vars.contains("xpython"));
        assert!(used_vars.contains("testexprvar"));
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
