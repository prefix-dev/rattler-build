//! AST traversal for extracting variables from Jinja templates
//!
//! This module provides functionality to parse Jinja templates and expressions,
//! traverse their AST, and extract all variable names used in them.

use std::collections::BTreeSet;
use std::fmt::Display;

use minijinja::machinery::ast::Expr;
use serde::{Deserialize, Serialize};

/// A validated Jinja2 template with pre-computed variable dependencies
/// Templates include the `${{ }}` delimiters, e.g., `${{ name }}-${{ version }}`
#[derive(Debug, Clone, PartialEq)]
pub struct JinjaTemplate {
    source: String,
    variables: Vec<String>,
}

impl JinjaTemplate {
    /// Create a new JinjaTemplate, validating that it parses correctly
    pub fn new(source: String) -> Result<Self, String> {
        let variables = extract_variables_from_template(&source)
            .map_err(|e| format!("Failed to parse Jinja template: {}", e))?;
        Ok(Self { source, variables })
    }

    /// Get the raw template source
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the raw template source (alias for consistency)
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Get the pre-computed list of variables used in this template (cached)
    pub fn used_variables(&self) -> &[String] {
        &self.variables
    }
}

impl Display for JinjaTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl Serialize for JinjaTemplate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as just the source string
        self.source.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JinjaTemplate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        JinjaTemplate::new(source).map_err(serde::de::Error::custom)
    }
}

/// A validated Jinja2 expression (without `${{ }}` delimiters) used in conditionals
/// Examples: `linux`, `version >= '3.0'`, `target_platform == 'linux'`
#[derive(Debug, Clone, PartialEq)]
pub struct JinjaExpression {
    source: String,
    variables: Vec<String>,
}

impl JinjaExpression {
    /// Create a new JinjaExpression, validating that it parses correctly
    pub fn new(source: String) -> Result<Self, String> {
        let variables = extract_variables_from_expression_checked(&source)
            .map_err(|e| format!("Failed to parse Jinja expression: {}", e))?;
        Ok(Self { source, variables })
    }

    /// Get the raw expression source
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the pre-computed list of variables used in this expression (cached)
    pub fn used_variables(&self) -> &[String] {
        &self.variables
    }
}

impl Display for JinjaExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl Serialize for JinjaExpression {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as just the source string
        self.source.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JinjaExpression {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        JinjaExpression::new(source).map_err(serde::de::Error::custom)
    }
}

/// Extract variable names from a Jinja2 template string using minijinja's AST parsing
/// Returns Result for validation during construction of JinjaTemplate
fn extract_variables_from_template(template: &str) -> Result<Vec<String>, minijinja::Error> {
    use minijinja::machinery::{WhitespaceConfig, parse};

    let mut variables = BTreeSet::new();

    // Parse the template directly
    let ast = parse(
        template,
        "template",
        Default::default(),
        WhitespaceConfig::default(),
    )?;

    // Walk the AST and collect variable names
    collect_variables_from_ast(&ast, &mut variables);

    Ok(variables.into_iter().collect())
}

/// Extract variable names from a Jinja2 expression (without ${{ }} delimiters)
/// Returns Result for validation during construction of JinjaExpression
fn extract_variables_from_expression_checked(expr: &str) -> Result<Vec<String>, minijinja::Error> {
    use minijinja::machinery::parse_expr;

    let mut variables = BTreeSet::new();

    // Parse just the expression
    let ast = parse_expr(expr)?;
    collect_variables_from_expr(&ast, &mut variables);

    Ok(variables.into_iter().collect())
}

/// Recursively collect variable names from the minijinja AST
fn collect_variables_from_ast(
    node: &minijinja::machinery::ast::Stmt,
    variables: &mut BTreeSet<String>,
) {
    use minijinja::machinery::ast::Stmt;

    match node {
        Stmt::Template(template) => {
            for child in &template.children {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::EmitExpr(emit) => {
            collect_variables_from_expr(&emit.expr, variables);
        }
        Stmt::ForLoop(for_loop) => {
            collect_variables_from_expr(&for_loop.iter, variables);
            for child in &for_loop.body {
                collect_variables_from_ast(child, variables);
            }
            for child in &for_loop.else_body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::IfCond(if_cond) => {
            collect_variables_from_expr(&if_cond.expr, variables);
            for child in &if_cond.true_body {
                collect_variables_from_ast(child, variables);
            }
            for child in &if_cond.false_body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::WithBlock(with_block) => {
            for (_target, expr) in &with_block.assignments {
                collect_variables_from_expr(expr, variables);
            }
            for child in &with_block.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::Set(set) => {
            collect_variables_from_expr(&set.expr, variables);
        }
        Stmt::SetBlock(_) => {
            // SetBlock doesn't contain variable references we care about
        }
        Stmt::AutoEscape(auto_escape) => {
            collect_variables_from_expr(&auto_escape.enabled, variables);
            for child in &auto_escape.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::FilterBlock(filter_block) => {
            collect_variables_from_expr(&filter_block.filter, variables);
            for child in &filter_block.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::Block(block) => {
            for child in &block.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::Extends(_) | Stmt::Include(_) => {
            // These don't contain variable references we care about for now
        }
        Stmt::Import(_) | Stmt::FromImport(_) => {
            // Import statements don't contain variable references we care about
        }
        Stmt::Do(do_stmt) => {
            collect_variables_from_call(&do_stmt.call, variables);
        }
        Stmt::EmitRaw(_) => {
            // Raw text doesn't contain variables
        }
        Stmt::Macro(_) => {
            // Macro definitions don't contain variable references we care about
        }
        Stmt::CallBlock(_) => {
            // CallBlock doesn't contain variable references we care about
        }
    }
}

/// Collect variables from a Call expression
/// Special handling for rattler-build Jinja functions that expand to variant variables
fn collect_variables_from_call(
    call: &minijinja::machinery::ast::Call,
    variables: &mut BTreeSet<String>,
) {
    // Check if this is a special function call that we should expand
    if let Expr::Var(var) = &call.expr {
        let function_name = var.id.to_string();

        match function_name.as_str() {
            "compiler" => {
                // compiler('c') expands to c_compiler and c_compiler_version
                if let Some(lang) = extract_first_string_arg(&call.args) {
                    variables.insert(format!("{}_compiler", lang));
                    variables.insert(format!("{}_compiler_version", lang));
                } else {
                    // If we can't extract the argument, collect the arguments as variables
                    for arg in &call.args {
                        collect_variables_from_call_arg(arg, variables);
                    }
                }
            }
            "stdlib" => {
                // stdlib('c') expands to c_stdlib and c_stdlib_version
                if let Some(lang) = extract_first_string_arg(&call.args) {
                    variables.insert(format!("{}_stdlib", lang));
                    variables.insert(format!("{}_stdlib_version", lang));
                } else {
                    for arg in &call.args {
                        collect_variables_from_call_arg(arg, variables);
                    }
                }
            }
            "pin_subpackage" => {
                // pin_subpackage(name) expands to the name variable (unless it's a string literal)
                // pin_subpackage("literal") doesn't expand
                for arg in &call.args {
                    if let minijinja::machinery::ast::CallArg::Pos(expr) = arg {
                        // Don't add string literals as variables
                        if !matches!(expr, Expr::Const(_)) {
                            collect_variables_from_expr(expr, variables);
                        }
                    }
                }
            }
            "pin_compatible" => {
                // pin_compatible(name) expands to the name variable (unless it's a string literal)
                for arg in &call.args {
                    if let minijinja::machinery::ast::CallArg::Pos(expr) = arg
                        && !matches!(expr, Expr::Const(_))
                    {
                        collect_variables_from_expr(expr, variables);
                    }
                }
            }
            "match" => {
                // match(var, spec) - first arg is a variable, second is a string
                for arg in &call.args {
                    if let minijinja::machinery::ast::CallArg::Pos(expr) = arg {
                        // Both arguments could be variables
                        collect_variables_from_expr(expr, variables);
                    }
                }
            }
            "cdt" => {
                // Add cdt_name to variables
                variables.insert("cdt_name".to_string());
            }
            _ => {
                // For other functions, collect the function name and arguments normally
                collect_variables_from_expr(&call.expr, variables);
                for arg in &call.args {
                    collect_variables_from_call_arg(arg, variables);
                }
            }
        }
    } else {
        // Not a simple function call, collect normally
        collect_variables_from_expr(&call.expr, variables);
        for arg in &call.args {
            collect_variables_from_call_arg(arg, variables);
        }
    }
}

/// Extract the first string literal argument from a function call
/// Returns None if the first argument is not a string literal
fn extract_first_string_arg(args: &[minijinja::machinery::ast::CallArg]) -> Option<String> {
    use minijinja::machinery::ast::{CallArg, Expr};

    if let Some(CallArg::Pos(Expr::Const(c))) = args.first()
        && let Some(s) = c.value.as_str()
    {
        return Some(s.to_string());
    }
    None
}

/// Collect variables from a CallArg
fn collect_variables_from_call_arg(
    arg: &minijinja::machinery::ast::CallArg,
    variables: &mut BTreeSet<String>,
) {
    use minijinja::machinery::ast::CallArg;

    // Match based on the CallArg variants available
    match arg {
        CallArg::Pos(expr) => {
            collect_variables_from_expr(expr, variables);
        }
        _ => {
            // Handle other CallArg variants if they exist
        }
    }
}

/// Recursively collect variable names from expressions
fn collect_variables_from_expr(
    expr: &minijinja::machinery::ast::Expr,
    variables: &mut BTreeSet<String>,
) {
    use minijinja::machinery::ast::Expr;

    match expr {
        Expr::Var(var) => {
            variables.insert(var.id.to_string());
        }
        Expr::Const(_) => {
            // Constants don't contain variables
        }
        Expr::UnaryOp(unary) => {
            collect_variables_from_expr(&unary.expr, variables);
        }
        Expr::BinOp(binop) => {
            collect_variables_from_expr(&binop.left, variables);
            collect_variables_from_expr(&binop.right, variables);
        }
        Expr::IfExpr(if_expr) => {
            collect_variables_from_expr(&if_expr.test_expr, variables);
            collect_variables_from_expr(&if_expr.true_expr, variables);
            if let Some(false_expr) = &if_expr.false_expr {
                collect_variables_from_expr(false_expr, variables);
            }
        }
        Expr::Filter(filter) => {
            if let Some(expr) = &filter.expr {
                collect_variables_from_expr(expr, variables);
            }
            for arg in &filter.args {
                collect_variables_from_call_arg(arg, variables);
            }
        }
        Expr::Test(test) => {
            collect_variables_from_expr(&test.expr, variables);
            for arg in &test.args {
                collect_variables_from_call_arg(arg, variables);
            }
        }
        Expr::GetAttr(get_attr) => {
            collect_variables_from_expr(&get_attr.expr, variables);
        }
        Expr::GetItem(get_item) => {
            collect_variables_from_expr(&get_item.expr, variables);
            collect_variables_from_expr(&get_item.subscript_expr, variables);
        }
        Expr::Slice(slice) => {
            collect_variables_from_expr(&slice.expr, variables);
            if let Some(start) = &slice.start {
                collect_variables_from_expr(start, variables);
            }
            if let Some(stop) = &slice.stop {
                collect_variables_from_expr(stop, variables);
            }
            if let Some(step) = &slice.step {
                collect_variables_from_expr(step, variables);
            }
        }
        Expr::Call(call) => {
            collect_variables_from_call(call, variables);
        }
        Expr::List(list) => {
            for item in &list.items {
                collect_variables_from_expr(item, variables);
            }
        }
        Expr::Map(map) => {
            for expr in &map.keys {
                collect_variables_from_expr(expr, variables);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_simple_variable() {
        let template = JinjaTemplate::new("${{ name }}".to_string()).unwrap();
        assert_eq!(template.used_variables(), &["name"]);
    }

    #[test]
    fn test_template_multiple_variables() {
        let template = JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        assert_eq!(vars, vec!["name", "version"]);
    }

    #[test]
    fn test_template_with_filter() {
        let template = JinjaTemplate::new("${{ name | lower }}".to_string()).unwrap();
        assert_eq!(template.used_variables(), &["name"]);
    }

    #[test]
    fn test_template_with_complex_expression() {
        let template =
            JinjaTemplate::new("${{ name ~ '-' ~ version if linux else name }}".to_string())
                .unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        assert_eq!(vars, vec!["linux", "name", "version"]);
    }

    #[test]
    fn test_expression_simple() {
        let expr = JinjaExpression::new("linux".to_string()).unwrap();
        assert_eq!(expr.used_variables(), &["linux"]);
    }

    #[test]
    fn test_expression_complex() {
        let expr =
            JinjaExpression::new("target_platform == 'linux' and version >= '3.0'".to_string())
                .unwrap();
        let mut vars = expr.used_variables().to_vec();
        vars.sort();
        assert_eq!(vars, vec!["target_platform", "version"]);
    }

    #[test]
    fn test_compiler_function_expands_variants() {
        let template = JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        // compiler('c') expands to c_compiler and c_compiler_version
        assert_eq!(vars, vec!["c_compiler", "c_compiler_version"]);
    }

    #[test]
    fn test_stdlib_function_expands_variants() {
        let template = JinjaTemplate::new("${{ stdlib('c') }}".to_string()).unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        // stdlib('c') expands to c_stdlib and c_stdlib_version
        assert_eq!(vars, vec!["c_stdlib", "c_stdlib_version"]);
    }

    #[test]
    fn test_template_with_binary_operators() {
        let template = JinjaTemplate::new("${{ x + y * z }}".to_string()).unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        assert_eq!(vars, vec!["x", "y", "z"]);
    }

    #[test]
    fn test_template_with_comparison() {
        let template = JinjaTemplate::new(
            "${{ version >= min_version and version < max_version }}".to_string(),
        )
        .unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        assert_eq!(vars, vec!["max_version", "min_version", "version"]);
    }

    #[test]
    fn test_template_with_attribute_access() {
        let template = JinjaTemplate::new("${{ build.number }}".to_string()).unwrap();
        assert_eq!(template.used_variables(), &["build"]);
    }

    #[test]
    fn test_template_with_list() {
        let template = JinjaTemplate::new("${{ [a, b, c] }}".to_string()).unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        assert_eq!(vars, vec!["a", "b", "c"]);
    }
}
