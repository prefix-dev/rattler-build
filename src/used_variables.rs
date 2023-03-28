use minijinja::machinery::{
    ast::{Expr, Stmt},
    parse,
};
use serde_yaml::Value as YamlValue;
/// find used variabels on a Raw (YAML) recipe

/// This does an initial "prerender" step where we evaluate the Jinja expressions globally
/// based on the variables in the `context` section of the recipe.
/// This also evaluates any Jinja functions such as `compiler` and `pin_subpackage` in a way
/// that we can post-process them as "used variables" more easily later.
///
/// Step 1:
///    - use only outer variables such as `target_platform`
///    - extract all sel( ... ) and `jinja` statements and find used variables
///    - retrieve used variabels from configuration and flatten selectors
///    - extract all dependencies and add them to used variables to build full variant
use std::collections::HashSet;

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
        _ => {
            //println!("Received {:?}", expr)
        }
    }
}

pub fn find_all_selectors(node: &YamlValue, selectors: &mut HashSet<String>) {
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
pub fn used_vars_from_jinja(recipe: &str) -> HashSet<String> {
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

// /// This finds all used variables in any dependency declarations, build, host, and run sections.
// /// As well as any used variables from Jinja functions
// fn find_used_variables(recipe: &YamlValue) -> HashSet<String> {
//     let mut used_variables = HashSet::new();

//     used_variables
// }
