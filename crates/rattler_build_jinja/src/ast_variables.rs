//! AST traversal for extracting variables from Jinja templates
//!
//! This module provides functionality to parse Jinja templates and expressions,
//! traverse their AST, and extract all variable names used in them.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Display;

use minijinja::machinery::WhitespaceConfig;
use minijinja::machinery::ast::{CallArg, Expr, Stmt};
use minijinja::machinery::parse_expr;
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
                // compiler('c') expands to c_compiler, c_compiler_version,
                // and CONDA_BUILD_SYSROOT (matching conda-build behavior where
                // different sysroot paths produce different package hashes)
                if let Some(lang) = extract_first_string_arg(&call.args) {
                    variables.insert(format!("{}_compiler", lang));
                    variables.insert(format!("{}_compiler_version", lang));
                    variables.insert("CONDA_BUILD_SYSROOT".to_string());
                } else {
                    // If we can't extract the argument, collect the arguments as variables
                    for arg in &call.args {
                        collect_variables_from_call_arg(arg, variables);
                    }
                }
            }
            "stdlib" => {
                // stdlib('c') expands to c_stdlib, c_stdlib_version,
                // and CONDA_BUILD_SYSROOT (matching conda-build behavior)
                if let Some(lang) = extract_first_string_arg(&call.args) {
                    variables.insert(format!("{}_stdlib", lang));
                    variables.insert(format!("{}_stdlib_version", lang));
                    variables.insert("CONDA_BUILD_SYSROOT".to_string());
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

/// Extract the root variable name from an expression by following filter chains.
///
/// For `foo | replace("a", "b") | lower`, this returns `Some("foo")`.
/// For non-variable expressions (e.g., string literals), returns `None`.
fn extract_root_variable(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Var(var) => Some(var.id.to_string()),
        Expr::Filter(filter) => filter.expr.as_ref().and_then(extract_root_variable),
        _ => None,
    }
}

/// Collect both guarded and unguarded variable names from an expression in a single pass.
///
/// A variable is "guarded" if it is the root input to a `default`/`d` filter.
/// A variable is "unguarded" if it appears anywhere outside such a guarded position.
fn collect_default_guard_info_from_expr(
    expr: &Expr,
    guarded: &mut HashSet<String>,
    unguarded: &mut HashSet<String>,
) {
    let recurse = collect_default_guard_info_from_expr;
    match expr {
        Expr::Filter(filter) if filter.name == "default" || filter.name == "d" => {
            // The primary expression is guarded by `default`
            if let Some(inner) = &filter.expr
                && let Some(root_var) = extract_root_variable(inner)
            {
                guarded.insert(root_var);
            }
            // Recurse into filter args — nested defaults make their inputs guarded too,
            // while non-default args contain unguarded variables
            for arg in &filter.args {
                if let CallArg::Pos(arg_expr) = arg {
                    recurse(arg_expr, guarded, unguarded);
                }
            }
        }
        Expr::Var(var) => {
            unguarded.insert(var.id.to_string());
        }
        Expr::Filter(filter) => {
            if let Some(inner) = &filter.expr {
                recurse(inner, guarded, unguarded);
            }
            for arg in &filter.args {
                if let CallArg::Pos(arg_expr) = arg {
                    recurse(arg_expr, guarded, unguarded);
                }
            }
        }
        Expr::BinOp(binop) => {
            recurse(&binop.left, guarded, unguarded);
            recurse(&binop.right, guarded, unguarded);
        }
        Expr::UnaryOp(unary) => {
            recurse(&unary.expr, guarded, unguarded);
        }
        Expr::IfExpr(if_expr) => {
            recurse(&if_expr.test_expr, guarded, unguarded);
            recurse(&if_expr.true_expr, guarded, unguarded);
            if let Some(false_expr) = &if_expr.false_expr {
                recurse(false_expr, guarded, unguarded);
            }
        }
        Expr::Call(call) => {
            recurse(&call.expr, guarded, unguarded);
            for arg in &call.args {
                if let CallArg::Pos(arg_expr) = arg {
                    recurse(arg_expr, guarded, unguarded);
                }
            }
        }
        Expr::Test(test) => {
            recurse(&test.expr, guarded, unguarded);
            for arg in &test.args {
                if let CallArg::Pos(arg_expr) = arg {
                    recurse(arg_expr, guarded, unguarded);
                }
            }
        }
        Expr::List(list) => {
            for item in &list.items {
                recurse(item, guarded, unguarded);
            }
        }
        Expr::GetAttr(get_attr) => {
            recurse(&get_attr.expr, guarded, unguarded);
        }
        Expr::GetItem(get_item) => {
            recurse(&get_item.expr, guarded, unguarded);
            recurse(&get_item.subscript_expr, guarded, unguarded);
        }
        Expr::Slice(slice) => {
            recurse(&slice.expr, guarded, unguarded);
            if let Some(start) = &slice.start {
                recurse(start, guarded, unguarded);
            }
            if let Some(stop) = &slice.stop {
                recurse(stop, guarded, unguarded);
            }
            if let Some(step) = &slice.step {
                recurse(step, guarded, unguarded);
            }
        }
        Expr::Map(map) => {
            for key in &map.keys {
                recurse(key, guarded, unguarded);
            }
        }
        Expr::Const(_) => {}
    }
}

/// Collect both guarded and unguarded variable names from a statement AST in a single pass.
fn collect_default_guard_info_from_ast(
    node: &Stmt,
    guarded: &mut HashSet<String>,
    unguarded: &mut HashSet<String>,
) {
    let recurse_expr = collect_default_guard_info_from_expr;
    let recurse_ast = collect_default_guard_info_from_ast;
    match node {
        Stmt::Template(template) => {
            for child in &template.children {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::EmitExpr(emit) => {
            recurse_expr(&emit.expr, guarded, unguarded);
        }
        Stmt::ForLoop(for_loop) => {
            recurse_expr(&for_loop.iter, guarded, unguarded);
            for child in &for_loop.body {
                recurse_ast(child, guarded, unguarded);
            }
            for child in &for_loop.else_body {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::IfCond(if_cond) => {
            recurse_expr(&if_cond.expr, guarded, unguarded);
            for child in &if_cond.true_body {
                recurse_ast(child, guarded, unguarded);
            }
            for child in &if_cond.false_body {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::WithBlock(with_block) => {
            for (_target, expr) in &with_block.assignments {
                recurse_expr(expr, guarded, unguarded);
            }
            for child in &with_block.body {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::Set(set) => {
            recurse_expr(&set.expr, guarded, unguarded);
        }
        Stmt::AutoEscape(auto_escape) => {
            recurse_expr(&auto_escape.enabled, guarded, unguarded);
            for child in &auto_escape.body {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::FilterBlock(filter_block) => {
            recurse_expr(&filter_block.filter, guarded, unguarded);
            for child in &filter_block.body {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::Block(block) => {
            for child in &block.body {
                recurse_ast(child, guarded, unguarded);
            }
        }
        Stmt::Do(do_stmt) => {
            recurse_expr(&do_stmt.call.expr, guarded, unguarded);
        }
        _ => {}
    }
}

/// Extract variable names that are *exclusively* guarded by a `default` (or `d`) filter
/// from a Jinja expression (without `${{ }}` delimiters).
///
/// A variable is only considered exclusively guarded if it appears as an input to a
/// `default`/`d` filter AND does not also appear in any unguarded position.
/// For example, in `bar | default + bar`, `bar` is NOT exclusively guarded because
/// it also appears unguarded on the right side of `+`.
pub fn extract_default_guarded_variables_from_expression(expr: &str) -> HashSet<String> {
    let mut guarded = HashSet::new();
    let mut unguarded = HashSet::new();
    if let Ok(ast) = parse_expr(expr) {
        collect_default_guard_info_from_expr(&ast, &mut guarded, &mut unguarded);
    }
    // Only exclude variables that appear exclusively in guarded positions
    &guarded - &unguarded
}

/// Extract variable names that are *exclusively* guarded by a `default` (or `d`) filter
/// from a Jinja template string (with `${{ }}` delimiters).
///
/// A variable is only considered exclusively guarded if it appears as an input to a
/// `default`/`d` filter AND does not also appear in any unguarded position.
pub fn extract_default_guarded_variables_from_template(template: &str) -> HashSet<String> {
    let mut guarded = HashSet::new();
    let mut unguarded = HashSet::new();
    if let Ok(ast) = minijinja::machinery::parse(
        template,
        "template",
        Default::default(),
        WhitespaceConfig::default(),
    ) {
        collect_default_guard_info_from_ast(&ast, &mut guarded, &mut unguarded);
    }
    &guarded - &unguarded
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
        // compiler('c') expands to CONDA_BUILD_SYSROOT, c_compiler, and c_compiler_version
        assert_eq!(
            vars,
            vec!["CONDA_BUILD_SYSROOT", "c_compiler", "c_compiler_version"]
        );
    }

    #[test]
    fn test_stdlib_function_expands_variants() {
        let template = JinjaTemplate::new("${{ stdlib('c') }}".to_string()).unwrap();
        let mut vars = template.used_variables().to_vec();
        vars.sort();
        // stdlib('c') expands to CONDA_BUILD_SYSROOT, c_stdlib, and c_stdlib_version
        assert_eq!(
            vars,
            vec!["CONDA_BUILD_SYSROOT", "c_stdlib", "c_stdlib_version"]
        );
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

    #[test]
    fn test_extract_default_guarded_simple() {
        // `foo | default("fallback")` — `foo` is guarded
        let guarded =
            extract_default_guarded_variables_from_expression(r#"foo | default("fallback")"#);
        assert_eq!(guarded, HashSet::from(["foo".to_string()]));
    }

    #[test]
    fn test_extract_default_guarded_with_d_alias() {
        // `foo | d("fallback")` — `d` is an alias for `default`
        let guarded = extract_default_guarded_variables_from_expression(r#"foo | d("fallback")"#);
        assert_eq!(guarded, HashSet::from(["foo".to_string()]));
    }

    #[test]
    fn test_extract_default_guarded_with_filter_chain() {
        // `foo | replace("a", "b") | default("fallback")` — `foo` is the root variable
        let guarded = extract_default_guarded_variables_from_expression(
            r#"foo | replace("a", "b") | default("fallback")"#,
        );
        assert_eq!(guarded, HashSet::from(["foo".to_string()]));
    }

    #[test]
    fn test_extract_default_guarded_nested() {
        // `foo | default(bar | default("fallback"))` — both `foo` and `bar` are guarded
        let guarded = extract_default_guarded_variables_from_expression(
            r#"foo | default(bar | default("fallback"))"#,
        );
        assert_eq!(
            guarded,
            HashSet::from(["foo".to_string(), "bar".to_string()])
        );
    }

    #[test]
    fn test_extract_default_guarded_complex_fallback() {
        // `cxx_compiler_version | default(cxx_compiler | replace("vs", ""))`
        // — `cxx_compiler_version` is guarded, `cxx_compiler` is NOT (it's used in the fallback)
        let guarded = extract_default_guarded_variables_from_expression(
            r#"cxx_compiler_version | default(cxx_compiler | replace("vs", ""))"#,
        );
        assert_eq!(guarded, HashSet::from(["cxx_compiler_version".to_string()]));
    }

    #[test]
    fn test_extract_default_guarded_no_default() {
        // No `default` filter — nothing is guarded
        let guarded = extract_default_guarded_variables_from_expression("foo | lower | upper");
        assert!(guarded.is_empty());
    }

    #[test]
    fn test_extract_default_guarded_var_also_used_unguarded() {
        // `bar | default("x") + bar` — `bar` is guarded on the left but also used
        // unguarded on the right, so it should NOT be exclusively guarded
        let guarded =
            extract_default_guarded_variables_from_expression(r#"bar | default("x") + bar"#);
        assert!(
            guarded.is_empty(),
            "bar appears in both guarded and unguarded positions, so it should not be excluded"
        );
    }

    #[test]
    fn test_extract_default_guarded_from_template() {
        let guarded =
            extract_default_guarded_variables_from_template(r#"${{ foo | default("bar") }}"#);
        assert_eq!(guarded, HashSet::from(["foo".to_string()]));
    }
}
