//! Evaluation of stage0 types into stage1 types
//!
//! This module implements the `Evaluate` trait for stage0 types,
//! converting them into their stage1 equivalents by:
//! - Rendering Jinja templates
//! - Flattening conditionals based on the evaluation context
//! - Validating the results
//!
//! ## Build String Deferred Evaluation
//!
//! The build string is NOT evaluated during stage0 → stage1 conversion because it may
//! depend on a special `hash` variable that must be computed from the actual variant
//! (the subset of variant variables that were actually used during evaluation).
//!
//! The evaluation flow is:
//! 1. Extract the build.string template source without evaluating it
//! 2. Track all accessed variables during recipe evaluation
//! 3. Determine the actual variant from accessed variables
//! 4. Compute the hash from the actual variant
//! 5. Call `Build::render_build_string_with_hash()` to finalize the build string

use std::{
    collections::{BTreeMap, HashSet},
    ops::Not,
    path::PathBuf,
    str::FromStr,
};

use indexmap::IndexMap;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{
    MatchSpec, NoArchType, PackageName, PackageNameMatcher, ParseStrictness, VersionWithSource,
};
use rattler_digest::{Md5Hash, Sha256Hash};

use crate::{
    ParseError, Span,
    stage0::{
        self, About as Stage0About, Build as Stage0Build, Extra as Stage0Extra, License,
        Package as Stage0Package, Requirements as Stage0Requirements, Source as Stage0Source,
        Stage0Recipe, TestType as Stage0TestType,
        build::{
            BinaryRelocation as Stage0BinaryRelocation, DynamicLinking as Stage0DynamicLinking,
            ForceFileType as Stage0ForceFileType, PostProcess as Stage0PostProcess,
            PrefixDetection as Stage0PrefixDetection, PrefixIgnore as Stage0PrefixIgnore,
            PythonBuild as Stage0PythonBuild, VariantKeyUsage as Stage0VariantKeyUsage,
        },
        requirements::{
            IgnoreRunExports as Stage0IgnoreRunExports, RunExports as Stage0RunExports,
        },
        source::{
            AttestationConfig as Stage0AttestationConfig, GitRev as Stage0GitRev,
            GitSource as Stage0GitSource, PathSource as Stage0PathSource,
            UrlSource as Stage0UrlSource,
        },
        tests::{
            CommandsTest as Stage0CommandsTest, CommandsTestFiles as Stage0CommandsTestFiles,
            CommandsTestRequirements as Stage0CommandsTestRequirements,
            DownstreamTest as Stage0DownstreamTest,
            PackageContentsCheckFiles as Stage0PackageContentsCheckFiles,
            PackageContentsTest as Stage0PackageContentsTest, PerlTest as Stage0PerlTest,
            PythonTest as Stage0PythonTest, PythonVersion as Stage0PythonVersion,
            RTest as Stage0RTest, RubyTest as Stage0RubyTest,
        },
        types::{ConditionalList, Item, JinjaExpression, Value},
    },
    stage1::{
        self, About as Stage1About, AllOrGlobVec, Dependency, Evaluate, EvaluationContext,
        Extra as Stage1Extra, GlobVec, Package as Stage1Package, Recipe as Stage1Recipe,
        Requirements as Stage1Requirements, Rpaths,
        build::{
            Build as Stage1Build, BuildString, DynamicLinking as Stage1DynamicLinking,
            ForceFileType as Stage1ForceFileType, PostProcess as Stage1PostProcess,
            PrefixDetection as Stage1PrefixDetection, PythonBuild as Stage1PythonBuild,
            VariantKeyUsage as Stage1VariantKeyUsage,
        },
        requirements::{
            IgnoreRunExports as Stage1IgnoreRunExports, RunExports as Stage1RunExports,
        },
        source::{
            AttestationConfig as Stage1AttestationConfig, GitRev as Stage1GitRev,
            GitSource as Stage1GitSource, GitUrl as Stage1GitUrl, PathSource as Stage1PathSource,
            Source as Stage1Source, UrlSource as Stage1UrlSource, parse_publisher_string,
        },
        tests::{
            CommandsTest as Stage1CommandsTest, CommandsTestFiles as Stage1CommandsTestFiles,
            CommandsTestRequirements as Stage1CommandsTestRequirements,
            DownstreamTest as Stage1DownstreamTest,
            PackageContentsCheckFiles as Stage1PackageContentsCheckFiles,
            PackageContentsTest as Stage1PackageContentsTest, PerlTest as Stage1PerlTest,
            PythonTest as Stage1PythonTest, PythonVersion as Stage1PythonVersion,
            RTest as Stage1RTest, RubyTest as Stage1RubyTest, TestType as Stage1TestType,
        },
    },
};
use rattler_build_jinja::{
    Variable, extract_default_guarded_variables_from_expression,
    extract_default_guarded_variables_from_template,
};

/// Variables that are always included in variant combinations
pub const ALWAYS_INCLUDED_VARS: &[&str] =
    &["target_platform", "channel_targets", "channel_sources"];

/// Helper to render a Jinja template to a Variable (preserving type information)
fn render_template_to_variable(
    template: &str,
    context: &EvaluationContext,
    span: Option<&Span>,
) -> Result<Variable, ParseError> {
    let undefined_behavior = context.jinja_config().undefined_behavior;
    let jinja = context.to_jinja();

    let trimmed = template.trim();

    // Check if it's a simple expression: starts with "${{", ends with "}}", and no additional "${{" inside
    // Simple examples: "${{ true }}", "${{ 42 }}", "${{ name }}"
    // Complex examples: "${{ a }}/${{ b }}", "prefix ${{ x }}", "${{ a }} ${{ b }}"
    let is_simple_expression = trimmed.starts_with("${{")
        && trimmed.ends_with("}}")
        && !trimmed[3..trimmed.len() - 2].contains("${{");

    let minijinja_value = if is_simple_expression {
        // Extract the expression between ${{ and }}
        let expression = trimmed[3..trimmed.len() - 2].trim();

        // Simple expression - use jinja.eval() for type-preserving evaluation with variable tracking
        match jinja.eval(expression) {
            Ok(val) => val,
            Err(e) => {
                // Transfer the tracked variables from Jinja to EvaluationContext
                for var in jinja.accessed_variables_excluding_functions() {
                    context.track_access(&var);
                }
                for var in jinja.undefined_variables() {
                    context.track_undefined(&var);
                }

                // Build error with suggestion based on undefined variables
                let undefined_vars: Vec<String> = jinja
                    .undefined_variables_excluding_functions()
                    .into_iter()
                    .collect();
                let mut error = ParseError::jinja_error(
                    format!("Failed to evaluate expression '{}': {}", expression, e),
                    span.cloned().unwrap_or(Span::new_blank()),
                );

                if !undefined_vars.is_empty() {
                    let suggestion = if undefined_vars.len() == 1 {
                        format!(
                            "Variable '{}' is not defined in the context",
                            undefined_vars[0]
                        )
                    } else {
                        format!(
                            "Variables {} are not defined in the context",
                            undefined_vars.join(", ")
                        )
                    };
                    error = error.with_suggestion(suggestion);
                }

                return Err(error);
            }
        }
    } else {
        // Complex template (e.g., "${{ base }}/${{ name }}") - render as string
        let rendered_str = match jinja.render_str(template) {
            Ok(s) => s,
            Err(e) => {
                // Transfer the tracked variables from Jinja to EvaluationContext
                for var in jinja.accessed_variables_excluding_functions() {
                    context.track_access(&var);
                }
                for var in jinja.undefined_variables() {
                    context.track_undefined(&var);
                }

                // Build error with suggestion based on undefined variables
                let undefined_vars: Vec<String> = jinja
                    .undefined_variables_excluding_functions()
                    .into_iter()
                    .collect();
                let mut error = ParseError::jinja_error(
                    format!("Failed to render template: {}", e),
                    span.cloned().unwrap_or(Span::new_blank()),
                );

                if !undefined_vars.is_empty() {
                    let suggestion = if undefined_vars.len() == 1 {
                        format!(
                            "Variable '{}' is not defined in the context",
                            undefined_vars[0]
                        )
                    } else {
                        format!(
                            "Variables {} are not defined in the context",
                            undefined_vars.join(", ")
                        )
                    };
                    error = error.with_suggestion(suggestion);
                }

                return Err(error);
            }
        };

        // Transfer tracked variables and check for undefined ones
        for var in jinja.accessed_variables_excluding_functions() {
            context.track_access(&var);
        }
        for var in jinja.undefined_variables() {
            context.track_undefined(&var);
        }

        // Check for undefined variables and error out (even if rendering succeeded)
        // Only error if we're not in Lenient mode
        // Exclude variables guarded by the `default` filter — they are intentionally undefined
        let default_guarded = extract_default_guarded_variables_from_template(template);
        let undefined_vars: Vec<String> = jinja
            .undefined_variables_excluding_functions()
            .into_iter()
            .filter(|v| default_guarded.contains(v).not())
            .collect();
        if !undefined_vars.is_empty()
            && !matches!(
                undefined_behavior,
                rattler_build_jinja::UndefinedBehavior::Lenient
            )
        {
            let mut error = ParseError::jinja_error(
                format!(
                    "Undefined variable(s) in template: {}",
                    undefined_vars
                        .iter()
                        .map(|s| format!("'{}'", s))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                span.cloned().unwrap_or(Span::new_blank()),
            );
            let suggestion = if undefined_vars.len() == 1 {
                format!(
                    "Variable '{}' is not defined in the context",
                    undefined_vars[0]
                )
            } else {
                format!(
                    "Variables {} are not defined in the context",
                    undefined_vars.join(", ")
                )
            };
            error = error.with_suggestion(suggestion);
            return Err(error);
        }

        // Parse the string to detect type (for simple values like "true" or "42")
        // This is a fallback - it won't preserve types for complex expressions
        return Ok(parse_rendered_value(&rendered_str));
    };

    // Transfer the tracked variables from Jinja to EvaluationContext
    for var in jinja.accessed_variables_excluding_functions() {
        context.track_access(&var);
    }
    for var in jinja.undefined_variables() {
        context.track_undefined(&var);
    }

    // Check for undefined variables and error out (even if evaluation succeeded)
    // This catches cases like "${{ 'foo' if undefined_var else 'bar' }}" in SemiStrict mode
    // Only error if we're not in Lenient mode
    // Exclude variables guarded by the `default` filter — they are intentionally undefined
    let expression = trimmed[3..trimmed.len() - 2].trim();
    let default_guarded = extract_default_guarded_variables_from_expression(expression);
    let undefined_vars: Vec<String> = jinja
        .undefined_variables_excluding_functions()
        .into_iter()
        .filter(|v| default_guarded.contains(v).not())
        .collect();
    if !undefined_vars.is_empty()
        && !matches!(
            undefined_behavior,
            rattler_build_jinja::UndefinedBehavior::Lenient
        )
    {
        let mut error = ParseError::jinja_error(
            format!(
                "Undefined variable(s) in expression: {}",
                undefined_vars
                    .iter()
                    .map(|s| format!("'{}'", s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            span.cloned().unwrap_or(Span::new_blank()),
        );
        let suggestion = if undefined_vars.len() == 1 {
            format!(
                "Variable '{}' is not defined in the context",
                undefined_vars[0]
            )
        } else {
            format!(
                "Variables {} are not defined in the context",
                undefined_vars.join(", ")
            )
        };
        error = error.with_suggestion(suggestion);
        return Err(error);
    }

    // Wrap the minijinja::Value in our Variable type
    // This preserves the type information (bool, int, string, etc.)
    Ok(Variable::from(minijinja_value))
}

/// Parse a rendered string value into a Variable with proper type detection
///
/// This function detects booleans and integers from their string representation
/// and creates properly-typed Variable instances.
fn parse_rendered_value(s: &str) -> Variable {
    let trimmed = s.trim();

    // Try to parse as boolean
    if trimmed.eq_ignore_ascii_case("true") {
        return Variable::from(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Variable::from(false);
    }

    // Try to parse as integer
    if let Ok(i) = trimmed.parse::<i64>() {
        return Variable::from(i);
    }

    // Default to string
    Variable::from(s.to_string())
}

#[cfg(test)]
mod variable_evaluation_tests {
    use super::*;
    use crate::stage1::EvaluationContext;

    #[test]
    fn test_evaluate_value_to_variable_preserves_bool() {
        let context = EvaluationContext::new();

        // Test template that evaluates to boolean
        let true_template =
            crate::stage0::types::JinjaTemplate::new("${{ true }}".to_string()).unwrap();
        let value = Value::new_template(true_template, None);

        let result = evaluate_value_to_variable(&value, &context).unwrap();
        // Check it's a boolean
        assert!(result.as_ref().is_true());
        assert_eq!(result.as_ref().to_string(), "true");
    }

    #[test]
    fn test_evaluate_value_to_variable_preserves_int() {
        let context = EvaluationContext::new();

        // Test template that evaluates to integer
        let int_template =
            crate::stage0::types::JinjaTemplate::new("${{ 42 }}".to_string()).unwrap();
        let value = Value::new_template(int_template, None);

        let result = evaluate_value_to_variable(&value, &context).unwrap();
        // Check it's a number
        assert!(result.as_ref().is_number());
        assert_eq!(result.as_ref().to_string(), "42");
    }

    #[test]
    fn test_evaluate_value_to_variable_string() {
        let context = EvaluationContext::new();

        // Test template that evaluates to string
        let str_template =
            crate::stage0::types::JinjaTemplate::new("${{ 'hello' }}".to_string()).unwrap();
        let value = Value::new_template(str_template, None);

        let result = evaluate_value_to_variable(&value, &context).unwrap();
        assert_eq!(result.as_ref().as_str(), Some("hello"));
    }

    #[test]
    fn test_evaluate_value_to_variable_concrete() {
        let context = EvaluationContext::new();

        // Test concrete Variable value
        let concrete_bool = Variable::from(true);
        let value = Value::new_concrete(concrete_bool.clone(), None);

        let result = evaluate_value_to_variable(&value, &context).unwrap();
        // Check it's a boolean
        assert!(result.as_ref().is_true());
    }
}

/// Helper to render a Jinja template with the evaluation context
fn render_template(
    template: &str,
    context: &EvaluationContext,
    span: Option<&Span>,
) -> Result<String, ParseError> {
    let undefined_behavior = context.jinja_config().undefined_behavior;
    let jinja = context.to_jinja();

    // The Jinja environment is already configured to use ${{ }} syntax
    // so we can pass the template as-is
    // The Jinja type now tracks accessed and undefined variables automatically
    match jinja.render_str(template) {
        Ok(result) => {
            // Transfer the tracked variables from Jinja to EvaluationContext
            for var in jinja.accessed_variables_excluding_functions() {
                context.track_access(&var);
            }
            for var in jinja.undefined_variables() {
                context.track_undefined(&var);
            }

            // Check for undefined variables and error out (even if rendering succeeded)
            // This catches cases like "${{ 'foo' if undefined_var else 'bar' }}" in SemiStrict mode
            // Only error if we're not in Lenient mode
            // Exclude variables guarded by the `default` filter — they are intentionally undefined
            let default_guarded = extract_default_guarded_variables_from_template(template);
            let undefined_vars: Vec<String> = jinja
                .undefined_variables_excluding_functions()
                .into_iter()
                .filter(|v| default_guarded.contains(v).not())
                .collect();
            if !undefined_vars.is_empty()
                && !matches!(
                    undefined_behavior,
                    rattler_build_jinja::UndefinedBehavior::Lenient
                )
            {
                let mut error = ParseError::jinja_error(
                    format!(
                        "Undefined variable(s) in template: {}",
                        undefined_vars
                            .iter()
                            .map(|s| format!("'{}'", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    span.map_or_else(Span::new_blank, |s| *s),
                );
                let suggestion = if undefined_vars.len() == 1 {
                    format!(
                        "Variable '{}' is not defined in the context",
                        undefined_vars[0]
                    )
                } else {
                    format!(
                        "Variables {} are not defined in the context",
                        undefined_vars.join(", ")
                    )
                };
                error = error.with_suggestion(suggestion);
                return Err(error);
            }

            Ok(result)
        }
        Err(e) => {
            // Transfer the tracked variables from Jinja to EvaluationContext
            for var in jinja.accessed_variables_excluding_functions() {
                context.track_access(&var);
            }
            for var in jinja.undefined_variables() {
                context.track_undefined(&var);
            }

            // Build error suggestion based on undefined variables
            let undefined_vars: Vec<String> = jinja
                .undefined_variables_excluding_functions()
                .into_iter()
                .collect();
            let mut error = ParseError::jinja_error(
                format!("Template rendering failed: {} (template: {})", e, template),
                span.map_or_else(Span::new_blank, |s| *s),
            );

            if !undefined_vars.is_empty() {
                let suggestion_text = if undefined_vars.len() == 1 {
                    format!(
                        "The variable '{}' is not defined in the evaluation context. \
                         Make sure it is provided or defined in the context section.",
                        undefined_vars[0]
                    )
                } else {
                    format!(
                        "The variables {} are not defined in the evaluation context. \
                         Make sure they are provided or defined in the context section.",
                        undefined_vars
                            .iter()
                            .map(|s| format!("'{}'", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                error = error.with_suggestion(suggestion_text);
            }

            Err(error)
        }
    }
}

/// Evaluate a simple conditional expression
fn evaluate_condition(
    expr: &JinjaExpression,
    context: &EvaluationContext,
    span: Option<&Span>,
) -> Result<bool, ParseError> {
    let jinja = context.to_jinja();

    // Evaluate the expression to get its value
    let value = jinja.eval(expr.source()).map_err(|e| {
        ParseError::jinja_error(
            format!("Failed to evaluate condition '{}': {}", expr.source(), e),
            span.cloned().unwrap_or(Span::new_blank()),
        )
    })?;

    // Transfer the tracked variables from Jinja to EvaluationContext
    for var in jinja.accessed_variables_excluding_functions() {
        context.track_access(&var);
    }

    // Check for undefined variables and error out
    // Track all undefined variables (including function names for completeness)
    for var in jinja.undefined_variables() {
        context.track_undefined(&var);
    }

    // But only error on actual undefined variables (not function names)
    // Exclude variables guarded by the `default` filter — they are intentionally undefined
    let default_guarded = extract_default_guarded_variables_from_expression(expr.source());
    let undefined_vars: Vec<String> = jinja
        .undefined_variables_excluding_functions()
        .into_iter()
        .filter(|v| default_guarded.contains(v).not())
        .collect();
    if !undefined_vars.is_empty() {
        return Err(
            ParseError::jinja_error(
                format!(
                    "Undefined variable(s) in condition '{}': {}",
                    expr.source(),
                    undefined_vars.iter().map(|s| format!("'{}'", s)).collect::<Vec<_>>().join(", ")
                ),
                span.cloned().unwrap_or(Span::new_blank()),
            )
            .with_suggestion("Make sure all variables used in conditions are defined in the variant config or context")
        );
    }

    // Convert the minijinja Value to a boolean using Jinja's truthiness rules
    Ok(value.is_true())
}

/// Evaluate a Value<String> into a String
pub fn evaluate_string_value(
    value: &Value<String>,
    context: &EvaluationContext,
) -> Result<String, ParseError> {
    if let Some(s) = value.as_concrete() {
        Ok(s.clone())
    } else if let Some(template) = value.as_template() {
        render_template(template.source(), context, value.span())
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Evaluate a Value<T: ToString> into a String
pub fn evaluate_value_to_string<T: ToString>(
    value: &Value<T>,
    context: &EvaluationContext,
) -> Result<String, ParseError> {
    if let Some(v) = value.as_concrete() {
        Ok(v.to_string())
    } else if let Some(template) = value.as_template() {
        render_template(template.source(), context, value.span())
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Evaluate a Value<Variable> into a Variable, preserving type information
///
/// This function properly handles templates that evaluate to booleans, integers, or strings,
/// rather than converting everything to strings.
pub fn evaluate_value_to_variable(
    value: &Value<Variable>,
    context: &EvaluationContext,
) -> Result<Variable, ParseError> {
    if let Some(var) = value.as_concrete() {
        Ok(var.clone())
    } else if let Some(template) = value.as_template() {
        let result = render_template_to_variable(template.source(), context, value.span())?;
        if value.force_string() {
            // The original YAML scalar was quoted or a block scalar, so the user
            // intended a string even if the jinja expression evaluates to a number
            // or boolean (e.g. `"${{ 123 }}"` should produce "123", not 123).
            Ok(Variable::from(result.as_ref().to_string()))
        } else {
            Ok(result)
        }
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Evaluate an optional Value<String> into an Option<String>
pub fn evaluate_optional_string_value(
    value: &Option<Value<String>>,
    context: &EvaluationContext,
) -> Result<Option<String>, ParseError> {
    match value {
        None => Ok(None),
        Some(v) => evaluate_string_value(v, context).map(|s| Some(s.trim().to_string())),
    }
}

/// Extract the template source from a Value<String> without evaluating it
/// This is used for deferred evaluation (e.g., build.string with hash variable)
fn extract_template_source(value: &Value<String>) -> Option<String> {
    if let Some(v) = value.as_concrete() {
        Some(v.clone())
    } else if let Some(template) = value.as_template() {
        Some(template.source().to_string())
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Track variables used in a template without evaluating it
/// This is used to track variable access for deferred evaluation (e.g., build.script with environment variables)
/// Returns the variables that are referenced in the template
fn track_template_variables(value: &Value<String>, context: &EvaluationContext) -> Vec<String> {
    if let Some(template) = value.as_template() {
        let vars = template.used_variables();
        for var in vars {
            context.track_access(var);
        }
        vars.to_vec()
    } else {
        Vec::new()
    }
}

/// Try to evaluate a template string, but preserve it if there are undefined variables
/// This is specifically for build script content where environment variables like ${{ PYTHON }}
/// are only available at build time.
/// Returns Ok(evaluated_string) if successful, or Ok(raw_template) if there were undefined variables.
/// Returns Err only for non-undefined-variable errors (syntax errors, etc.)
fn evaluate_or_preserve_template(
    value: &Value<String>,
    context: &EvaluationContext,
) -> Result<String, ParseError> {
    // First, track the variables
    track_template_variables(value, context);

    // Try to evaluate
    match evaluate_string_value(value, context) {
        Ok(result) => Ok(result),
        Err(e) => {
            // Check if this error was due to undefined variables
            // The error message will contain "undefined value" or we can check the tracked undefined vars
            let error_msg = e.to_string();
            if error_msg.contains("undefined value")
                || error_msg.contains("not defined in the evaluation context")
            {
                // This is an undefined variable error - preserve the template
                Ok(extract_template_source(value).unwrap_or_default())
            } else {
                // This is a different error (syntax error, etc.) - propagate it
                Err(e)
            }
        }
    }
}

/// Evaluate an optional Value<T: ToString> into an Option<T> by parsing the string result
pub fn evaluate_optional_value_to_type<T>(
    value: &Option<Value<T>>,
    context: &EvaluationContext,
) -> Result<Option<T>, ParseError>
where
    T: ToString + FromStr,
    T::Err: std::fmt::Display,
{
    match value {
        None => Ok(None),
        Some(v) => {
            let s = evaluate_value_to_string(v, context)?;
            T::from_str(&s).map(Some).map_err(|e| {
                ParseError::invalid_value(
                    "value",
                    format!("Failed to parse: {}", e),
                    Span::new_blank(),
                )
            })
        }
    }
}

/// Generic helper to evaluate a slice of Item<T> by processing each Value<T>
///
/// This abstracts the common pattern of iterating over a conditional list,
/// evaluating conditionals, and processing each value with a closure.
///
/// This helper significantly reduces code duplication across the evaluation functions
/// for different list types (strings, globs, dependencies, etc.).
///
/// Works with both `ConditionalList` and `ConditionalListOrItem` via `.as_slice()`.
/// Supports nested conditionals at any depth.
fn evaluate_conditional_list<T, R, F>(
    list: &[Item<T>],
    context: &EvaluationContext,
    process: F,
) -> Result<Vec<R>, ParseError>
where
    T: std::fmt::Debug,
    F: FnMut(&Value<T>, &EvaluationContext) -> Result<Option<R>, ParseError>,
{
    let mut results = Vec::new();
    evaluate_items_recursive(list, context, &mut results, process)?;
    Ok(results)
}

/// Recursively evaluate items, handling nested conditionals
fn evaluate_items_recursive<T, R, F>(
    items: &[Item<T>],
    context: &EvaluationContext,
    results: &mut Vec<R>,
    mut process: F,
) -> Result<F, ParseError>
where
    T: std::fmt::Debug,
    F: FnMut(&Value<T>, &EvaluationContext) -> Result<Option<R>, ParseError>,
{
    for item in items.iter() {
        match item {
            Item::Value(value) => {
                if let Some(result) = process(value, context)? {
                    results.push(result);
                }
            }
            Item::Conditional(cond) => {
                let condition_met =
                    evaluate_condition(&cond.condition, context, cond.condition_span.as_ref())?;

                let items_to_process = if condition_met {
                    &cond.then
                } else {
                    match &cond.else_value {
                        Some(else_items) => else_items,
                        None => continue,
                    }
                };

                // Recursively process nested items (supports nested conditionals)
                process = evaluate_items_recursive(
                    items_to_process.as_slice(),
                    context,
                    results,
                    process,
                )?;
            }
        }
    }

    Ok(process)
}

/// Evaluate a slice of Item<String> into Vec<String>
///
/// Empty strings are filtered out. This allows conditional list items like
/// `- ${{ "numpy" if unix }}` to be removed when the condition is false
/// (Jinja renders them as empty strings).
///
/// This function works with both `ConditionalList` and `ConditionalListOrItem` via `.as_slice()`.
pub fn evaluate_string_list_items(
    list: &[Item<String>],
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    evaluate_conditional_list(list, context, |value, ctx| {
        let s = evaluate_string_value(value, ctx)?;
        // Filter out empty strings from templates like `${{ "value" if condition }}`
        Ok(if s.is_empty() { None } else { Some(s) })
    })
}

/// Evaluate a ConditionalList<String> into Vec<String>
///
/// Empty strings are filtered out. This allows conditional list items like
/// `- ${{ "numpy" if unix }}` to be removed when the condition is false
/// (Jinja renders them as empty strings).
pub fn evaluate_string_list(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    evaluate_string_list_items(list.as_slice(), context)
}

/// Evaluate skip expressions as Jinja boolean expressions
///
/// Skip values are Jinja expressions like `is_abi3`, `not unix`, etc.
/// They are rendered through Jinja to track accessed variables
/// (important for variant hash computation), and then evaluated eagerly
/// to produce a boolean result. This matches the behavior of main where
/// skip conditions are resolved during parsing, before the variant gets
/// noarch overrides that would change `target_platform`.
///
/// Returns true if ANY skip condition evaluates to true (OR logic).
pub fn evaluate_skip_list(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<bool, ParseError> {
    let conditions: Vec<String> =
        evaluate_conditional_list(list.as_slice(), context, |value, ctx| {
            // Skip expressions are Jinja boolean expressions, not templates
            // We need to render them through Jinja to track accessed variables,
            // but we preserve the original expression string for later evaluation
            if let Some(expr_str) = value.as_concrete() {
                // Wrap in ${{ }} to render as Jinja expression (for variable tracking)
                let template = format!("${{{{ {} }}}}", expr_str);
                // Ignore the result - we just want to trigger variable tracking
                let _ = render_template(&template, ctx, value.span());

                // Always return the original expression string
                Ok(Some(expr_str.clone()))
            } else if let Some(template) = value.as_template() {
                // Already a template, render it
                let rendered = render_template(template.source(), ctx, value.span())?;
                // Return the rendered value (should evaluate to a skip condition)
                if rendered.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(rendered))
                }
            } else {
                unreachable!("Value must be either concrete or template")
            }
        })?;
    Ok(is_skipped(&conditions, context))
}

/// Check if any skip condition in the list evaluates to true
///
/// Skip conditions are Jinja boolean expressions (e.g., "win", "unix", "platform == 'osx-64'").
/// Returns true if ANY condition evaluates to true (OR logic).
pub fn is_skipped(skip_conditions: &[String], context: &EvaluationContext) -> bool {
    for condition in skip_conditions {
        let jinja = context.to_jinja();

        // Evaluate the skip condition as a boolean expression
        match jinja.eval(condition) {
            Ok(value) => {
                if value.is_true() {
                    return true;
                }
            }
            Err(_) => {
                // If evaluation fails, we can't determine if it's skipped
                // Continue to check other conditions
            }
        }
    }
    false
}

/// Evaluate a string list, preserving templates with undefined variables
/// This is used for script content where build-time environment variables might be referenced
pub fn evaluate_string_list_lenient(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    evaluate_conditional_list(list.as_slice(), context, |value, ctx| {
        let s = evaluate_or_preserve_template(value, ctx)?;
        // Filter out empty strings from templates like `${{ "value" if condition }}`
        Ok(if s.is_empty() { None } else { Some(s) })
    })
}

/// Preserve a string list with lazy evaluation - conditionals are evaluated but string content is kept as-is
///
/// This is used for script content where we want to:
/// 1. Evaluate if/then/else conditionals (e.g., `- if: unix`) to decide which items to include
/// 2. Keep the string content as-is, preserving any Jinja templates for later evaluation at build time
///
/// This allows templates like `${{ "$CMAKE_ARGS" if unix else "%CMAKE_ARGS%" }}` to remain
/// unevaluated until build time when the actual environment variables are available.
///
/// IMPORTANT: Variable tracking is done for ALL branches (both taken and not-taken) because
/// script variables affect variant computation. For example, `if: win then: echo ${{ win_var }}`
/// should track `win_var` even on unix, since it's part of the recipe's variant dependencies.
pub fn preserve_string_list(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    // First, track variables in ALL branches (including non-taken ones)
    // This ensures variant computation includes all script variables regardless of platform
    track_all_template_variables(list.as_slice(), context);

    evaluate_conditional_list(list.as_slice(), context, |value, ctx| {
        // Track variables in templates for used_variant calculation
        // This ensures that variables like ${{ python }} in scripts are tracked
        // even though we don't fully evaluate them at this stage
        track_template_variables(value, ctx);

        // Extract the original string value without evaluating templates
        let s = extract_template_source(value).unwrap_or_default();
        // Filter out empty strings
        Ok(if s.is_empty() { None } else { Some(s) })
    })
}

/// Recursively track template variables in ALL items, including both branches of conditionals.
///
/// This is used by `preserve_string_list` to ensure that variables in non-taken conditional
/// branches are still tracked for variant computation purposes.
fn track_all_template_variables(items: &[Item<String>], context: &EvaluationContext) {
    for item in items {
        match item {
            Item::Value(value) => {
                track_template_variables(value, context);
            }
            Item::Conditional(cond) => {
                // Track variables used in the condition expression itself
                for var in cond.condition.used_variables() {
                    context.track_access(var);
                }
                // Track variables in BOTH branches
                track_all_template_variables(cond.then.as_slice(), context);
                if let Some(else_items) = &cond.else_value {
                    track_all_template_variables(else_items.as_slice(), context);
                }
            }
        }
    }
}

/// Evaluate a ConditionalList<PackageName> into Vec<PackageName>
///
/// This evaluates templates in PackageName values and filters out empty results.
pub fn evaluate_package_name_list(
    list: &ConditionalList<PackageName>,
    context: &EvaluationContext,
) -> Result<Vec<PackageName>, ParseError> {
    evaluate_conditional_list(
        list.as_slice(),
        context,
        |value: &Value<PackageName>, ctx| {
            // Handle both concrete PackageName and template values
            if let Some(concrete) = value.as_concrete() {
                // For concrete values, just clone it
                Ok(Some(concrete.clone()))
            } else if let Some(template) = value.as_template() {
                // Render the template
                let s = render_template(template.source(), ctx, value.span())?;
                // Filter out empty strings from templates like `${{ "numpy" if unix }}`
                if s.is_empty() {
                    return Ok(None);
                }
                // Parse as a MatchSpec first to extract the package name.
                // Templates like `${{ compiler('cxx') }}` render to strings
                // such as `gxx_linux-64 =13` which are valid MatchSpecs but
                // not valid bare PackageNames.
                if let Ok(ms) = MatchSpec::from_str(&s, ParseStrictness::Lenient)
                    && let Some(PackageNameMatcher::Exact(name)) = ms.name
                {
                    return Ok(Some(name));
                }
                PackageName::from_str(&s).map(Some).map_err(|e| {
                    ParseError::invalid_value(
                        "package name",
                        format!("'{}' is not a valid package name: {}", s, e),
                        value.span().copied().unwrap_or_else(Span::new_blank),
                    )
                })
            } else {
                unreachable!("Value must be either concrete or template")
            }
        },
    )
}

/// Helper function to validate and evaluate glob patterns from a ConditionalList
///
/// Empty strings are filtered out. This allows conditional list items like
/// `- ${{ "loguru" if unix }}` to be removed when the condition is false
/// (Jinja renders them as empty strings).
fn evaluate_glob_patterns(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    evaluate_conditional_list(list.as_slice(), context, |value, ctx| {
        let pattern = evaluate_string_value(value, ctx)?;
        // Filter out empty strings from templates like `${{ "value" if condition }}`
        if pattern.is_empty() {
            return Ok(None);
        }
        // Validate the glob pattern immediately with proper error reporting
        match rattler_build_types::glob::validate_glob_pattern(&pattern) {
            Ok(_) => Ok(Some(pattern)),
            Err(e) => Err(
                ParseError::invalid_value(
                    "glob pattern",
                    format!("Invalid glob pattern '{}': {}", pattern, e),
                    value.span().copied().unwrap_or_else(Span::new_blank),
                )
                .with_suggestion("Check your glob pattern syntax. Common issues include unmatched braces or invalid escape sequences.")
            ),
        }
    })
}

/// Evaluate a ConditionalList<String> into a GlobVec (include-only)
///
/// This is a convenience wrapper around `evaluate_glob_vec` for simple include-only lists.
pub fn evaluate_glob_vec_simple(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<GlobVec, ParseError> {
    // Just delegate to evaluate_glob_vec with a simple List variant
    evaluate_glob_vec(
        &crate::stage0::types::IncludeExclude::List(list.clone()),
        context,
    )
}

/// Evaluate an IncludeExclude into a GlobVec, preserving span information for error reporting
pub fn evaluate_glob_vec(
    include_exclude: &crate::stage0::types::IncludeExclude<String>,
    context: &EvaluationContext,
) -> Result<GlobVec, ParseError> {
    let (include_list, exclude_list) = match include_exclude {
        crate::stage0::types::IncludeExclude::List(list) => (list, &ConditionalList::default()),
        crate::stage0::types::IncludeExclude::Mapping { include, exclude } => (include, exclude),
    };

    // Evaluate and validate include patterns
    let include_globs = evaluate_glob_patterns(include_list, context)?;

    // Evaluate and validate exclude patterns
    let exclude_globs = evaluate_glob_patterns(exclude_list, context)?;

    // Create the GlobVec - this should not fail since we've already validated
    GlobVec::from_strings(include_globs, exclude_globs).map_err(|e| {
        ParseError::invalid_value(
            "glob set",
            format!("Failed to build glob set: {}", e),
            Span::new_blank(),
        )
    })
}

/// Evaluate a ConditionalList<EntryPoint> into Vec<EntryPoint>
/// Entry points can be concrete values or templates that render to strings
pub fn evaluate_entry_point_list(
    list: &ConditionalList<rattler_conda_types::package::EntryPoint>,
    context: &EvaluationContext,
) -> Result<Vec<rattler_conda_types::package::EntryPoint>, ParseError> {
    evaluate_conditional_list(
        list.as_slice(),
        context,
        |val: &Value<rattler_conda_types::package::EntryPoint>, ctx| {
            if let Some(ep) = val.as_concrete() {
                Ok(Some(ep.clone()))
            } else if let Some(template) = val.as_template() {
                let s = render_template(template.source(), ctx, val.span())?;
                s.parse::<rattler_conda_types::package::EntryPoint>()
                    .map(Some)
                    .map_err(|e| {
                        ParseError::invalid_value(
                            "entry point",
                            format!("Invalid entry point '{}': {}", s, e),
                            val.span().copied().unwrap_or_else(Span::new_blank),
                        )
                        .with_suggestion(
                            "Entry points should be in the format 'command = module:function'",
                        )
                    })
            } else {
                unreachable!("Value must be either concrete or template")
            }
        },
    )
}

/// Check if a MatchSpec is a "free spec" (no version constraints).
///
/// A free spec is one that doesn't have any version constraints, build constraints,
/// or other specifications beyond the package name. These are used to determine
/// which variant variables should be included in the hash.
///
/// Examples of free specs:
/// - `python` (free)
/// - `numpy` (free)
///
/// Examples of NON-free specs:
/// - `python >=3.8` (has version constraint)
/// - `numpy 1.20.*` (has version constraint)
/// - `gcc_linux-64` (has build constraint via name)
pub(crate) fn is_free_matchspec(spec: &rattler_conda_types::MatchSpec) -> bool {
    let rattler_conda_types::MatchSpec {
        name,
        version,
        build,
        build_number,
        channel,
        subdir,
        namespace,
        md5,
        sha256,
        file_name,
        extras,
        url,
        license,
        condition,
        track_features,
    } = spec;

    name.is_some()
        && version.is_none()
        && build.is_none()
        && build_number.is_none()
        && channel.is_none()
        && subdir.is_none()
        && namespace.is_none()
        && md5.is_none()
        && sha256.is_none()
        && file_name.is_none()
        && extras.is_none()
        && url.is_none()
        && license.is_none()
        && condition.is_none()
        && track_features.is_none()
}
/// Evaluate a ConditionalList<SerializableMatchSpec> into Vec<Dependency>
///
/// This handles dependencies which may be:
/// - Concrete MatchSpecs (already validated at parse time)
/// - Templates that render to MatchSpecs or pin expressions
/// - Conditionals with MatchSpecs or templates in then/else branches
///
/// The template strings can be either:
/// - Regular match specs (e.g., "python >=3.8")
/// - Pin subpackage expressions (JSON like `{ pin_subpackage: { name: foo } }`)
/// - Pin compatible expressions (JSON like `{ pin_compatible: { name: bar } }`)
///
/// # Arguments
/// * `list` - The conditional list of dependencies to evaluate
/// * `context` - The evaluation context
///   are tracked as variant variables. This should be true for build/host dependencies
///   and false for run dependencies, since run dependencies don't affect the build variant.
pub fn evaluate_dependency_list(
    list: &crate::stage0::types::ConditionalList<crate::stage0::SerializableMatchSpec>,
    context: &EvaluationContext,
) -> Result<Vec<crate::stage1::Dependency>, ParseError> {
    evaluate_conditional_list(list.as_slice(), context, |value, ctx| {
        if let Some(match_spec) = value.as_concrete() {
            Ok(Some(Dependency::Spec(Box::new(match_spec.0.clone()))))
        } else if let Some(template) = value.as_template() {
            let s = render_template(template.source(), ctx, value.span())?;

            // Filter out empty strings from templates like `${{ "numpy" if unix }}`
            if s.is_empty() {
                return Ok(None);
            }

            let span_opt = value.span().copied();
            let dep = parse_dependency_string(&s, &span_opt)?;
            Ok(Some(dep))
        } else {
            unreachable!("Value must be either concrete or template")
        }
    })
}

/// Parse a dependency string into a Dependency
///
/// Handles both JSON pin expressions and regular MatchSpec strings
fn parse_dependency_string(s: &str, span: &Option<Span>) -> Result<Dependency, ParseError> {
    let span = (*span).unwrap_or_else(Span::new_blank);

    // Check if it's a JSON dictionary (pin_subpackage or pin_compatible)
    if s.trim().starts_with('{') {
        // Try to deserialize as Dependency (which handles pin types)
        serde_json::from_str(s).map_err(|e| {
            ParseError::invalid_value(
                "pin dependency",
                format!("Failed to parse pin dependency: {}", e),
                span,
            )
        })
    } else {
        // It's a regular MatchSpec string
        let spec = MatchSpec::from_str(s, ParseStrictness::Strict).map_err(|e| {
            ParseError::invalid_value(
                "match spec",
                format!("Invalid match spec '{}': {}", s, e),
                span,
            )
        })?;
        Ok(Dependency::Spec(Box::new(spec)))
    }
}

/// Evaluate script with variable tracking for content, allowing undefined environment variables
///
/// This function evaluates script metadata (interpreter, env, cwd, file) normally, but for
/// script content (inline commands), it tracks variables and preserves templates with undefined
/// variables. This allows build-time environment variables like ${{ PYTHON }} or ${{ PREFIX }}
/// to be used in scripts without failing during recipe evaluation.
pub fn evaluate_script(
    script: &crate::stage0::types::Script,
    context: &EvaluationContext,
) -> Result<rattler_build_script::Script, ParseError> {
    use rattler_build_script::{Script, ScriptContent as ScriptContentOutput};

    // If the script is default/empty, return default script
    if script.is_default() {
        return Ok(Script::default());
    }

    // Evaluate interpreter normally (it's typically simple and doesn't use env vars)
    let interpreter = if let Some(interp) = &script.interpreter {
        Some(evaluate_string_value(interp, context)?)
    } else {
        None
    };

    // Evaluate environment variables
    let mut env = indexmap::IndexMap::new();
    for (key, val) in &script.env {
        let evaluated_val = evaluate_string_value(val, context)?;
        if !evaluated_val.is_empty() {
            env.insert(key.clone(), evaluated_val);
        }
    }

    // Copy secrets as-is
    let secrets = script.secrets.clone();

    // Evaluate cwd normally (it's a path, not a script command)
    let cwd = if let Some(cwd_val) = &script.cwd {
        Some(PathBuf::from(evaluate_string_value(cwd_val, context)?))
    } else {
        None
    };

    // For content: evaluate conditionals but preserve templates with undefined vars
    let (content, inferred_interpreter) = if let Some(file_val) = &script.file {
        // File paths should be evaluated normally
        let file_str = evaluate_string_value(file_val, context)?;
        let file_path = PathBuf::from(&file_str);
        // Infer interpreter from file extension if not explicitly set
        let inferred = if interpreter.is_none() {
            rattler_build_script::determine_interpreter_from_path(&file_path)
        } else {
            None
        };
        (ScriptContentOutput::Path(file_path), inferred)
    } else if let Some(content_list) = &script.content {
        // Use lazy evaluation that preserves templates entirely
        // This evaluates if/then/else conditionals but keeps string content as-is
        // Templates like `${{ "$CMAKE_ARGS" if unix }}` will be evaluated at build time
        let commands = preserve_string_list(content_list, context)?;

        // Determine if script has additional options (env, interpreter, cwd, secrets)
        let has_additional_options =
            !env.is_empty() || interpreter.is_some() || cwd.is_some() || !secrets.is_empty();

        let content = if commands.is_empty() {
            ScriptContentOutput::Default
        } else if commands.len() == 1 && !has_additional_options && !script.content_explicit {
            // Single command with no additional options and NOT explicitly using content: field
            // This serializes as a simple string: `script: "cmd"`
            ScriptContentOutput::CommandOrPath(commands.into_iter().next().unwrap())
        } else if commands.len() == 1 && script.content_explicit {
            // Single command with explicit content: field
            // This serializes as `script: { content: "..." }`
            ScriptContentOutput::Command(commands.into_iter().next().unwrap())
        } else {
            // Multiple commands OR has additional options
            // This serializes with content field: `script: { content: [...] }`
            ScriptContentOutput::Commands(commands)
        };
        (content, None)
    } else {
        (ScriptContentOutput::Default, None)
    };

    // Use inferred interpreter if no explicit interpreter was set
    let final_interpreter = interpreter.or(inferred_interpreter);

    Ok(Script {
        interpreter: final_interpreter,
        env,
        secrets,
        content,
        cwd,
        content_explicit: script.content_explicit,
    })
}

/// Parse a boolean from a string (case-insensitive)
fn parse_bool_from_str(
    s: &str,
    field_name: &str,
    default_for_empty: bool,
) -> Result<bool, ParseError> {
    match s.to_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        // Empty string means the Jinja condition was false and there was no else branch
        // Use the provided default value
        "" => Ok(default_for_empty),
        _ => Err(ParseError::invalid_value(
            field_name,
            format!(
                "Invalid boolean value for '{}': '{}' (expected true/false or empty for default)",
                field_name, s
            ),
            Span::new_blank(),
        )),
    }
}

/// Evaluate a Value<bool> into a bool
///
/// When the template evaluates to an empty string (e.g., `${{ true if osx }}` on non-osx),
/// the `default_for_empty` value is used.
pub fn evaluate_bool_value(
    value: &Value<bool>,
    context: &EvaluationContext,
    field_name: &str,
    default_for_empty: bool,
) -> Result<bool, ParseError> {
    if let Some(b) = value.as_concrete() {
        Ok(*b)
    } else if let Some(template) = value.as_template() {
        let s = render_template(template.source(), context, value.span())?;
        parse_bool_from_str(&s, field_name, default_for_empty)
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Evaluate an optional field with a default value
/// This is a common pattern for Optional<T> that defaults to T::default() when None
pub fn evaluate_optional_with_default<S, T>(
    opt: &Option<S>,
    context: &EvaluationContext,
) -> Result<T, ParseError>
where
    S: Evaluate<Output = T>,
    T: Default,
{
    match opt {
        None => Ok(T::default()),
        Some(v) => v.evaluate(context),
    }
}

/// Generic helper to evaluate a Value<T> where T implements FromStr
/// This handles both concrete values and templates
pub fn evaluate_value<T>(
    value: &Value<T>,
    context: &EvaluationContext,
    type_name: &str,
) -> Result<T, ParseError>
where
    T: ToString + FromStr,
    T::Err: std::fmt::Display,
{
    if let Some(v) = value.as_concrete() {
        Ok(v.to_string().parse().map_err(|e| {
            ParseError::invalid_value(
                type_name,
                format!("Failed to parse {}: {}", type_name, e),
                Span::new_blank(),
            )
        })?)
    } else if let Some(template) = value.as_template() {
        let s = render_template(template.source(), context, value.span())?;
        s.parse().map_err(|e| {
            ParseError::invalid_value(
                type_name,
                format!("Invalid {} '{}': {}", type_name, s, e),
                value.span().copied().unwrap_or_else(Span::new_blank),
            )
        })
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Generic helper to evaluate an Option<Value<T>> where T implements FromStr
pub fn evaluate_optional_value<T>(
    value: &Option<Value<T>>,
    context: &EvaluationContext,
    type_name: &str,
) -> Result<Option<T>, ParseError>
where
    T: ToString + FromStr,
    T::Err: std::fmt::Display,
{
    match value {
        None => Ok(None),
        Some(v) => evaluate_value(v, context, type_name).map(Some),
    }
}

// Implement Evaluate for Value<T> where T is a foreign type
// This is allowed because Value is our local type (orphan rule doesn't apply)

impl Evaluate for Value<url::Url> {
    type Output = url::Url;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        if let Some(u) = self.as_concrete() {
            Ok(u.clone())
        } else if let Some(template) = self.as_template() {
            let s = render_template(template.source(), context, self.span())?;
            url::Url::parse(&s).map_err(|e| {
                ParseError::invalid_value(
                    "URL",
                    format!("Invalid URL '{}': {}", s, e),
                    self.span().copied().unwrap_or_else(Span::new_blank),
                )
            })
        } else {
            unreachable!("Value must be either concrete or template")
        }
    }
}

impl Evaluate for Option<Value<url::Url>> {
    type Output = Option<url::Url>;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        self.clone().map(|v| v.evaluate(context)).transpose()
    }
}

impl Evaluate for Value<License> {
    type Output = License;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        if let Some(license) = self.as_concrete() {
            Ok(license.clone())
        } else if let Some(template) = self.as_template() {
            let s = render_template(template.source(), context, self.span())?;
            s.parse::<License>().map_err(|e| {
                ParseError::invalid_value(
                    "SPDX license",
                    format!("Invalid SPDX license expression: {}", e),
                    self.span().copied().unwrap_or_else(Span::new_blank),
                )
            })
        } else {
            unreachable!("Value must be either concrete or template")
        }
    }
}

impl Evaluate for Value<PathBuf> {
    type Output = PathBuf;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        if let Some(p) = self.as_concrete() {
            Ok(p.clone())
        } else if let Some(template) = self.as_template() {
            let s = render_template(template.source(), context, self.span())?;
            Ok(PathBuf::from(s))
        } else {
            unreachable!("Value must be either concrete or template")
        }
    }
}

// Note: We can't implement Evaluate for Value<Sha256Hash> and Value<Md5Hash>
// because they are both type aliases to GenericArray<u8, N>, which would create
// conflicting implementations. We use helper functions instead.

/// Evaluate a Sha256Hash from a Value
fn evaluate_sha256(
    value: &Value<Sha256Hash>,
    context: &EvaluationContext,
) -> Result<Sha256Hash, ParseError> {
    if let Some(hash) = value.as_concrete() {
        Ok(*hash)
    } else if let Some(template) = value.as_template() {
        let s = render_template(template.source(), context, value.span())?;
        rattler_digest::parse_digest_from_hex::<rattler_digest::Sha256>(&s).ok_or_else(|| {
            ParseError::invalid_value(
                "SHA256 checksum",
                format!("Invalid SHA256 checksum: {}", s),
                value.span().copied().unwrap_or_else(Span::new_blank),
            )
        })
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Evaluate an Md5Hash from a Value
fn evaluate_md5(
    value: &Value<Md5Hash>,
    context: &EvaluationContext,
) -> Result<Md5Hash, ParseError> {
    if let Some(hash) = value.as_concrete() {
        Ok(*hash)
    } else if let Some(template) = value.as_template() {
        let s = render_template(template.source(), context, value.span())?;
        rattler_digest::parse_digest_from_hex::<rattler_digest::Md5>(&s).ok_or_else(|| {
            ParseError::invalid_value(
                "MD5 checksum",
                format!("Invalid MD5 checksum: {}", s),
                value.span().copied().unwrap_or_else(Span::new_blank),
            )
        })
    } else {
        unreachable!("Value must be either concrete or template")
    }
}

/// Macro to implement Evaluate for types that just evaluate string list fields
///
/// This handles the common pattern where a type contains one or more
/// ConditionalList<String> fields that need to be evaluated to Vec<String>.
///
/// # Single field example:
/// ```ignore
/// impl_evaluate_list_fields! {
///     Stage0PerlTest => Stage1PerlTest { uses }
/// }
/// ```
///
/// # Multiple fields example:
/// ```ignore
/// impl_evaluate_list_fields! {
///     Stage0RunExports => Stage1RunExports {
///         noarch,
///         strong,
///         weak
///     }
/// }
/// ```
macro_rules! impl_evaluate_list_fields {
    // Single field case
    ($stage0:ty => $stage1:ty { $field:ident }) => {
        impl Evaluate for $stage0 {
            type Output = $stage1;

            fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
                Ok(Self::Output {
                    $field: evaluate_string_list(&self.$field, context)?,
                })
            }
        }
    };

    // Multiple fields case
    ($stage0:ty => $stage1:ty { $($field:ident),+ $(,)? }) => {
        impl Evaluate for $stage0 {
            type Output = $stage1;

            fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
                Ok(Self::Output {
                    $(
                        $field: evaluate_string_list(&self.$field, context)?,
                    )+
                })
            }
        }
    };
}

impl Evaluate for Stage0Package {
    type Output = Stage1Package;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Evaluate name and version to strings
        let name_str = evaluate_value_to_string(&self.name, context)?;
        let version_str = evaluate_value_to_string(&self.version, context)?;

        // Parse into concrete types
        let name = PackageName::from_str(&name_str).map_err(|e| {
            ParseError::invalid_value(
                "name",
                format!(
                    "invalid value for name: '{}' is not a valid package name: {}",
                    name_str, e
                ),
                Span::new_blank(),
            )
        })?;

        let version = VersionWithSource::from_str(&version_str).map_err(|e| {
            ParseError::invalid_value(
                "version",
                format!(
                    "invalid value for version: '{}' is not a valid version: {}",
                    version_str, e
                ),
                Span::new_blank(),
            )
        })?;

        Ok(Stage1Package::new(name, version))
    }
}

impl Evaluate for Stage0About {
    type Output = Stage1About;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1About {
            homepage: self.homepage.evaluate(context)?,
            repository: self.repository.evaluate(context)?,
            documentation: self.documentation.evaluate(context)?,
            license: self
                .license
                .as_ref()
                .map(|v| v.evaluate(context))
                .transpose()?,
            license_file: self
                .license_file
                .as_ref()
                .map(|lf| evaluate_glob_vec_simple(lf, context))
                .transpose()?,
            license_family: evaluate_optional_string_value(&self.license_family, context)?,
            summary: evaluate_optional_string_value(&self.summary, context)?,
            description: evaluate_optional_string_value(&self.description, context)?,
        })
    }
}

// Evaluate RunExports with dependency parsing
impl Evaluate for Stage0RunExports {
    type Output = Stage1RunExports;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Run exports don't affect the build variant
        Ok(Stage1RunExports {
            noarch: evaluate_dependency_list(&self.noarch, context)?,
            strong: evaluate_dependency_list(&self.strong, context)?,
            strong_constraints: evaluate_dependency_list(&self.strong_constraints, context)?,
            weak: evaluate_dependency_list(&self.weak, context)?,
            weak_constraints: evaluate_dependency_list(&self.weak_constraints, context)?,
        })
    }
}

impl Evaluate for Stage0IgnoreRunExports {
    type Output = Stage1IgnoreRunExports;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1IgnoreRunExports {
            by_name: evaluate_package_name_list(&self.by_name, context)?,
            from_package: evaluate_package_name_list(&self.from_package, context)?,
        })
    }
}

impl Evaluate for Stage0Requirements {
    type Output = Stage1Requirements;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1Requirements {
            build: evaluate_dependency_list(&self.build, context)?,
            host: evaluate_dependency_list(&self.host, context)?,
            run: evaluate_dependency_list(&self.run, context)?,
            run_constraints: evaluate_dependency_list(&self.run_constraints, context)?,
            run_exports: self.run_exports.evaluate(context)?,
            ignore_run_exports: self.ignore_run_exports.evaluate(context)?,
        })
    }
}

// Evaluate Extra field with Jinja template support
impl Evaluate for Stage0Extra {
    type Output = Stage1Extra;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Recursively evaluate all values in the extra map, rendering any Jinja templates
        let extra = self
            .extra
            .iter()
            .map(|(key, value)| {
                let evaluated_value = evaluate_serde_value(value, context)?;
                Ok((key.clone(), evaluated_value))
            })
            .collect::<Result<indexmap::IndexMap<_, _>, ParseError>>()?;

        Ok(Stage1Extra { extra })
    }
}

/// Helper function to recursively evaluate serde_value::Value, rendering Jinja templates
fn evaluate_serde_value(
    value: &serde_value::Value,
    context: &EvaluationContext,
) -> Result<serde_value::Value, ParseError> {
    match value {
        serde_value::Value::String(s) => {
            // Check if this string contains a Jinja template
            if s.contains("${{") && s.contains("}}") {
                // Render the Jinja template using the render_template helper
                let rendered = render_template(s, context, None)?;
                Ok(serde_value::Value::String(rendered))
            } else {
                Ok(value.clone())
            }
        }
        serde_value::Value::Map(map) => {
            // Recursively evaluate all values in the map
            let evaluated_map = map
                .iter()
                .map(|(k, v)| {
                    let evaluated_value = evaluate_serde_value(v, context)?;
                    Ok((k.clone(), evaluated_value))
                })
                .collect::<Result<BTreeMap<_, _>, ParseError>>()?;
            Ok(serde_value::Value::Map(evaluated_map))
        }
        serde_value::Value::Seq(seq) => {
            // Recursively evaluate all items in the sequence
            let evaluated_seq = seq
                .iter()
                .map(|v| evaluate_serde_value(v, context))
                .collect::<Result<Vec<_>, ParseError>>()?;
            Ok(serde_value::Value::Seq(evaluated_seq))
        }
        // For other types (numbers, booleans, etc.), pass through as-is
        _ => Ok(value.clone()),
    }
}

impl Evaluate for Stage0PythonBuild {
    type Output = Stage1PythonBuild;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let skip_pyc_compilation = evaluate_glob_vec_simple(&self.skip_pyc_compilation, context)?;

        // Evaluate version_independent (supports Jinja templates)
        // Default is false - most packages are version-dependent
        let version_independent = match &self.version_independent {
            Some(val) => evaluate_bool_value(val, context, "version_independent", false)?,
            None => false,
        };

        // Evaluate use_python_app_entrypoint (supports Jinja templates)
        // Default is false - most packages don't use this
        let use_python_app_entrypoint = evaluate_bool_value(
            &self.use_python_app_entrypoint,
            context,
            "use_python_app_entrypoint",
            false,
        )?;

        Ok(Stage1PythonBuild {
            entry_points: evaluate_entry_point_list(&self.entry_points, context)?,
            skip_pyc_compilation,
            use_python_app_entrypoint,
            version_independent,
            site_packages_path: evaluate_optional_string_value(&self.site_packages_path, context)?,
        })
    }
}

impl Evaluate for Stage0VariantKeyUsage {
    type Output = Stage1VariantKeyUsage;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let down_prioritize_variant = match &self.down_prioritize_variant {
            None => None,
            Some(val) => {
                let s = evaluate_value_to_string(val, context)?;
                if s.is_empty() {
                    None
                } else {
                    Some(s.parse::<i32>().map_err(|_| {
                        ParseError::invalid_value(
                            "down_prioritize_variant",
                            format!("Invalid integer value for down_prioritize_variant: '{}'", s),
                            Span::new_blank(),
                        )
                    })?)
                }
            }
        };

        Ok(Stage1VariantKeyUsage {
            use_keys: evaluate_string_list(&self.use_keys, context)?,
            ignore_keys: evaluate_string_list(&self.ignore_keys, context)?,
            down_prioritize_variant,
        })
    }
}

impl Evaluate for Stage0ForceFileType {
    type Output = Stage1ForceFileType;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1ForceFileType {
            text: evaluate_glob_vec_simple(&self.text, context)?,
            binary: evaluate_glob_vec_simple(&self.binary, context)?,
        })
    }
}

impl Evaluate for Stage0PrefixDetection {
    type Output = Stage1PrefixDetection;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let ignore = match &self.ignore {
            Stage0PrefixIgnore::Boolean(val) => {
                let bool_val = if let Some(b) = val.as_concrete() {
                    *b
                } else if let Some(template) = val.as_template() {
                    let s = render_template(template.source(), context, val.span())?;
                    match s.as_str() {
                        "true" | "True" | "yes" | "Yes" => true,
                        "false" | "False" | "no" | "No" => false,
                        _ => {
                            return Err(ParseError::invalid_value(
                                "prefix_detection.ignore",
                                format!(
                                    "Invalid boolean value for prefix_detection.ignore: '{}'",
                                    s
                                ),
                                val.span().copied().unwrap_or_else(Span::new_blank),
                            ));
                        }
                    }
                } else {
                    unreachable!("Value must be either concrete or template")
                };
                AllOrGlobVec::All(bool_val)
            }
            Stage0PrefixIgnore::Patterns(list) => {
                AllOrGlobVec::SpecificPaths(evaluate_glob_vec_simple(list, context)?)
            }
        };

        // Evaluate ignore_binary_files (supports Jinja templates)
        // Default is false - binary files are checked for prefix by default
        let ignore_binary_files = evaluate_bool_value(
            &self.ignore_binary_files,
            context,
            "ignore_binary_files",
            false,
        )?;

        Ok(Stage1PrefixDetection {
            force_file_type: self.force_file_type.evaluate(context)?,
            ignore,
            ignore_binary_files,
        })
    }
}

impl Evaluate for Stage0PostProcess {
    type Output = Stage1PostProcess;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let regex_str = evaluate_string_value(&self.regex, context)?;
        let regex = regex::Regex::new(&regex_str).map_err(|e| {
            ParseError::invalid_value(
                "regex",
                format!("Invalid regular expression: {}", e),
                Span::new_blank(),
            )
            .with_suggestion("Check your regex syntax. Common issues include unescaped special characters or unbalanced brackets.")
        })?;

        Ok(Stage1PostProcess {
            files: evaluate_glob_vec_simple(&self.files, context)?,
            regex,
            replacement: evaluate_string_value(&self.replacement, context)?,
        })
    }
}

impl Evaluate for Stage0DynamicLinking {
    type Output = Stage1DynamicLinking;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        use crate::stage1::build::LinkingCheckBehavior;

        // Evaluate binary_relocation
        let binary_relocation = match &self.binary_relocation {
            Stage0BinaryRelocation::Boolean(val) => {
                let bool_val = if let Some(b) = val.as_concrete() {
                    *b
                } else if let Some(template) = val.as_template() {
                    let s = render_template(template.source(), context, val.span())?;
                    match s.as_str() {
                        "true" | "True" | "yes" | "Yes" => true,
                        "false" | "False" | "no" | "No" => false,
                        // Empty string means the condition was false and there was no else branch
                        // Use the default value (true = relocate all binaries)
                        "" => true,
                        _ => {
                            return Err(ParseError::invalid_value(
                                "binary_relocation",
                                format!(
                                    "Invalid boolean value for binary_relocation: '{}'. Expected true/false or empty (for default)",
                                    s
                                ),
                                val.span().copied().unwrap_or_else(Span::new_blank),
                            ));
                        }
                    }
                } else {
                    unreachable!("Value must be either concrete or template")
                };
                AllOrGlobVec::All(bool_val)
            }
            Stage0BinaryRelocation::Patterns(list) => {
                AllOrGlobVec::SpecificPaths(evaluate_glob_vec_simple(list, context)?)
            }
        };

        // Evaluate and validate glob patterns
        let missing_dso_allowlist = evaluate_glob_vec_simple(&self.missing_dso_allowlist, context)?;
        let rpath_allowlist = evaluate_glob_vec_simple(&self.rpath_allowlist, context)?;

        // Parse overdepending_behavior
        let overdepending_behavior = match &self.overdepending_behavior {
            None => LinkingCheckBehavior::Ignore,
            Some(v) => {
                let s = evaluate_value_to_string(v, context)?;
                match s.as_str() {
                    "ignore" => LinkingCheckBehavior::Ignore,
                    "error" => LinkingCheckBehavior::Error,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "overdepending_behavior",
                            format!(
                                "Invalid overdepending_behavior '{}'. Expected 'ignore' or 'error'",
                                s
                            ),
                            Span::new_blank(),
                        ));
                    }
                }
            }
        };

        // Parse overlinking_behavior
        let overlinking_behavior = match &self.overlinking_behavior {
            None => LinkingCheckBehavior::Ignore,
            Some(v) => {
                let s = evaluate_value_to_string(v, context)?;
                match s.as_str() {
                    "ignore" => LinkingCheckBehavior::Ignore,
                    "error" => LinkingCheckBehavior::Error,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "overlinking_behavior",
                            format!(
                                "Invalid overlinking_behavior '{}'. Expected 'ignore' or 'error'",
                                s
                            ),
                            Span::new_blank(),
                        ));
                    }
                }
            }
        };

        Ok(Stage1DynamicLinking {
            rpaths: Rpaths::new(evaluate_string_list(&self.rpaths, context)?),
            binary_relocation,
            missing_dso_allowlist,
            rpath_allowlist,
            overdepending_behavior,
            overlinking_behavior,
        })
    }
}

impl Evaluate for Stage0Build {
    type Output = Stage1Build;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // IMPORTANT: Do NOT fully evaluate build.string here - it may contain a "hash" variable
        // that needs to be deferred until after we know the actual variant and can compute the hash.
        // Store it as BuildString::Unresolved if it contains templates.
        // Track the variables used in the template to count towards "actually used" variables.
        let string = self
            .string
            .as_ref()
            .map_or(crate::stage1::build::BuildString::Default, |s| {
                // Track variables without failing on undefined
                track_template_variables(s, context);

                let template = extract_template_source(s).unwrap();
                crate::stage1::build::BuildString::unresolved(template, s.span().cloned())
            });

        // IMPORTANT: Do NOT fully evaluate build.script here - it may contain environment variables
        // like ${{ PYTHON }} or ${{ PREFIX }} that are only available at build time.
        // Track the variables used in the script to count towards "actually used" variables,
        // but defer actual evaluation until build time.
        let script = evaluate_script(&self.script, context)?;

        // Evaluate noarch
        let noarch = match &self.noarch {
            None => None,
            Some(v) => {
                // NoArchType is already validated during parsing, we just need to evaluate templates
                if let Some(value) = v.as_concrete() {
                    Some(*value)
                } else if let Some(template) = v.as_template() {
                    // If it's a template, we need to render it and parse as NoArchType
                    let s = render_template(template.source(), context, v.span())?;
                    // Parse the string as NoArchType using serde
                    serde_json::from_value::<NoArchType>(serde_json::Value::String(s.clone()))
                        .map(Some)
                        .map_err(|_| {
                            ParseError::invalid_value(
                                "noarch type",
                                format!(
                                    "Invalid noarch type '{}'. Expected 'python' or 'generic'",
                                    s
                                ),
                                v.span().copied().unwrap_or(Span::new_blank()),
                            )
                        })?
                } else {
                    unreachable!("Value must be either concrete or template")
                }
            }
        };

        // Evaluate skip conditions as Jinja boolean expressions
        // This tracks accessed variables for proper variant hash computation
        let skip = evaluate_skip_list(&self.skip, context)?;

        // Evaluate python configuration
        let python = self.python.evaluate(context)?;

        // Evaluate file lists and validate glob patterns
        let always_copy_files = evaluate_glob_vec_simple(&self.always_copy_files, context)?;
        let always_include_files = evaluate_glob_vec_simple(&self.always_include_files, context)?;

        // Evaluate files (handle both list and include/exclude variants)
        let files = evaluate_glob_vec(&self.files, context)?;

        // Evaluate dynamic linking
        let dynamic_linking = self.dynamic_linking.evaluate(context)?;

        // Evaluate variant
        let variant = self.variant.evaluate(context)?;

        // Evaluate prefix_detection
        let prefix_detection = self.prefix_detection.evaluate(context)?;

        // Evaluate post_process (supports conditional items with if/then/else)
        let mut post_process = Vec::new();
        for item in &self.post_process {
            post_process.extend(evaluate_post_process_item(item, context)?);
        }

        // Evaluate build number
        // None means inherit from top-level, Some(n) means use n (even if n is 0)
        let number = match &self.number {
            None => None,
            Some(value) => {
                let n = if let Some(n) = value.as_concrete() {
                    *n
                } else if let Some(template) = value.as_template() {
                    let s = render_template(template.source(), context, value.span())?;
                    // If we render to an empty string, treat it as 0
                    if s.is_empty() {
                        0
                    } else {
                        s.parse::<u64>().map_err(|_| {
                            ParseError::invalid_value(
                                "build number",
                                format!(
                                    "Invalid build number: '{}' is not a valid positive integer",
                                    s
                                ),
                                value.span().copied().unwrap_or_else(Span::new_blank),
                            )
                        })?
                    }
                } else {
                    unreachable!("Value must be either concrete or template")
                };
                Some(n)
            }
        };

        // Evaluate merge_build_and_host_envs (supports Jinja templates)
        // Default is false - environments are separate by default
        let merge_build_and_host_envs = evaluate_bool_value(
            &self.merge_build_and_host_envs,
            context,
            "merge_build_and_host_envs",
            false,
        )?;

        Ok(Stage1Build {
            number,
            string,
            script,
            noarch,
            python,
            skip,
            always_copy_files,
            always_include_files,
            merge_build_and_host_envs,
            files,
            dynamic_linking,
            variant,
            prefix_detection,
            post_process,
        })
    }
}

impl Evaluate for Stage0GitSource {
    type Output = Stage1GitSource;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Evaluate the Git URL
        let url_str = evaluate_string_value(&self.url.0, context)?;

        // Parse the URL into GitUrl enum (try URL first, then SSH, then path)
        let url = if let Ok(parsed_url) = url::Url::parse(&url_str) {
            Stage1GitUrl::Url(parsed_url)
        } else if url_str.contains('@') && url_str.contains(':') {
            // SSH-style URL (e.g., git@github.com:user/repo.git)
            Stage1GitUrl::Ssh(url_str)
        } else {
            // Assume it's a local path
            Stage1GitUrl::Path(PathBuf::from(url_str))
        };

        // Determine the revision (rev/tag/branch) - only one can be set
        let rev = if let Some(rev) = &self.rev {
            match rev {
                Stage0GitRev::Value(v) => {
                    let rev_str = evaluate_string_value(v, context)?;
                    Stage1GitRev::from_str(&rev_str).map_err(|e| {
                        ParseError::invalid_value(
                            "git revision",
                            format!("Invalid git revision: {}", e),
                            Span::new_blank(),
                        )
                    })?
                }
            }
        } else if let Some(tag) = &self.tag {
            match tag {
                Stage0GitRev::Value(v) => {
                    let tag_str = evaluate_string_value(v, context)?;
                    Stage1GitRev::Tag(tag_str)
                }
            }
        } else if let Some(branch) = &self.branch {
            match branch {
                Stage0GitRev::Value(v) => {
                    let branch_str = evaluate_string_value(v, context)?;
                    Stage1GitRev::Branch(branch_str)
                }
            }
        } else {
            Stage1GitRev::default()
        };

        Ok(Stage1GitSource {
            url,
            rev,
            depth: evaluate_optional_value_to_type(&self.depth, context)?,
            patches: evaluate_string_list(&self.patches, context)?
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            target_directory: self
                .target_directory
                .as_ref()
                .map(|v| v.evaluate(context))
                .transpose()?,
            // Default is false - LFS is opt-in
            lfs: self
                .lfs
                .as_ref()
                .map(|v| evaluate_bool_value(v, context, "lfs", false))
                .transpose()?
                .unwrap_or(false),
            // Default is true - submodules are updated by default
            submodules: self
                .submodules
                .as_ref()
                .map(|v| evaluate_bool_value(v, context, "submodules", true))
                .transpose()?
                .unwrap_or(true),
            expected_commit: self
                .expected_commit
                .as_ref()
                .map(|v| evaluate_string_value(v, context))
                .transpose()?,
        })
    }
}

impl Evaluate for Stage0UrlSource {
    type Output = Stage1UrlSource;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Evaluate URLs (they are Value<String>) and parse into url::Url
        let mut urls = Vec::new();
        for url_value in &self.url {
            let url_str = evaluate_string_value(url_value, context)?;
            let url = url::Url::parse(&url_str).map_err(|e| {
                ParseError::invalid_value(
                    "URL",
                    format!("Invalid URL '{}': {}", url_str, e),
                    Span::new_blank(),
                )
            })?;
            urls.push(url);
        }

        // Evaluate attestation config if present
        let attestation = self
            .attestation
            .as_ref()
            .map(|config| config.evaluate(context))
            .transpose()?;

        Ok(Stage1UrlSource {
            url: urls,
            sha256: self
                .sha256
                .as_ref()
                .map(|v| evaluate_sha256(v, context))
                .transpose()?,
            md5: self
                .md5
                .as_ref()
                .map(|v| evaluate_md5(v, context))
                .transpose()?,
            file_name: evaluate_optional_string_value(&self.file_name, context)?,
            patches: evaluate_string_list(&self.patches, context)?
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            target_directory: self
                .target_directory
                .as_ref()
                .map(|v| v.evaluate(context))
                .transpose()?,
            attestation,
        })
    }
}

impl Evaluate for Stage0AttestationConfig {
    type Output = Stage1AttestationConfig;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Evaluate bundle_url if present
        let bundle_url = self
            .bundle_url
            .as_ref()
            .map(|v| {
                let url_str = evaluate_string_value(v, context)?;
                url::Url::parse(&url_str).map_err(|e| {
                    ParseError::invalid_value(
                        "attestation.bundle_url",
                        format!("Invalid URL '{}': {}", url_str, e),
                        Span::new_blank(),
                    )
                })
            })
            .transpose()?;

        // Evaluate and parse publisher strings
        let mut publishers = Vec::new();
        for pub_value in &self.publishers {
            let pub_str = evaluate_string_value(pub_value, context)?;
            let publisher = parse_publisher_string(&pub_str).map_err(|e| {
                ParseError::invalid_value("attestation.publishers", &e, Span::new_blank())
            })?;
            publishers.push(publisher);
        }

        Ok(Stage1AttestationConfig {
            bundle_url,
            publishers,
        })
    }
}

impl Evaluate for Stage0PathSource {
    type Output = Stage1PathSource;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1PathSource {
            path: self.path.evaluate(context)?,
            sha256: self
                .sha256
                .as_ref()
                .map(|v| evaluate_sha256(v, context))
                .transpose()?,
            md5: self
                .md5
                .as_ref()
                .map(|v| evaluate_md5(v, context))
                .transpose()?,
            patches: evaluate_string_list(&self.patches, context)?
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            target_directory: self
                .target_directory
                .as_ref()
                .map(|v| v.evaluate(context))
                .transpose()?,
            file_name: self
                .file_name
                .as_ref()
                .map(|v| v.evaluate(context))
                .transpose()?,
            use_gitignore: self.use_gitignore,
            filter: evaluate_glob_vec(&self.filter, context)?,
        })
    }
}

impl Evaluate for Stage0Source {
    type Output = Stage1Source;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        match self {
            Stage0Source::Git(git) => Ok(Stage1Source::Git(git.evaluate(context)?)),
            Stage0Source::Url(url) => Ok(Stage1Source::Url(url.evaluate(context)?)),
            Stage0Source::Path(path) => Ok(Stage1Source::Path(path.evaluate(context)?)),
        }
    }
}

/// Evaluate a single source item (which can be a Value or Conditional), returning a vector
/// (conditionals expand to multiple sources)
/// Supports nested conditionals at any depth.
pub fn evaluate_source(
    source_item: &Item<Stage0Source>,
    context: &EvaluationContext,
) -> Result<Vec<Stage1Source>, ParseError> {
    match source_item {
        Item::Value(value) => {
            // Get the concrete Source from the Value
            let source = value.as_concrete().ok_or_else(|| {
                ParseError::invalid_value(
                    "source",
                    "source cannot be a template",
                    crate::Span::new_blank(),
                )
            })?;
            source.evaluate(context).map(|s| vec![s])
        }
        Item::Conditional(cond) => {
            // Evaluate the condition
            let condition_met =
                evaluate_condition(&cond.condition, context, cond.condition_span.as_ref())?;

            let sources_to_evaluate = if condition_met {
                &cond.then
            } else {
                match &cond.else_value {
                    Some(else_value) => else_value,
                    None => return Ok(Vec::new()), // No else branch and condition is false
                }
            };

            // Recursively evaluate the selected sources (supports nested conditionals)
            let mut results = Vec::new();
            for nested_item in sources_to_evaluate.iter() {
                results.extend(evaluate_source(nested_item, context)?);
            }
            Ok(results)
        }
    }
}

// Test type evaluations

impl Evaluate for Stage0PythonVersion {
    type Output = Stage1PythonVersion;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Helper to validate a python version spec by attempting to create a MatchSpec
        let validate_version = |version_str: &str, span: Option<&Span>| -> Result<(), ParseError> {
            let spec_str = format!("python={}", version_str);
            MatchSpec::from_str(&spec_str, ParseStrictness::Lenient).map_err(|e| {
                ParseError::invalid_value(
                    "python version spec",
                    format!(
                        "Invalid python version spec '{}': {}",
                        version_str, e
                    ),
                    span.cloned().unwrap_or(Span::new_blank()),
                )
                .with_suggestion(
                    "Python version must be a valid version constraint (e.g., '3.8', '>=3.7', '3.8.*')",
                )
            })?;
            Ok(())
        };

        match self {
            Stage0PythonVersion::Single(v) => {
                let evaluated = evaluate_string_value(v, context)?;
                validate_version(&evaluated, v.span())?;
                Ok(Stage1PythonVersion::Single(evaluated))
            }
            Stage0PythonVersion::Multiple(versions) => {
                // Evaluate the conditional list to get a flat list of version strings
                let evaluated_versions = evaluate_string_list(versions, context)?;

                // Validate each version
                for version_str in &evaluated_versions {
                    validate_version(version_str, None)?;
                }

                Ok(Stage1PythonVersion::Multiple(evaluated_versions))
            }
        }
    }
}

impl Evaluate for Stage0PythonTest {
    type Output = Stage1PythonTest;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let imports = evaluate_string_list_items(self.imports.as_slice(), context)?;

        let pip_check = match &self.pip_check {
            None => true, // default to true
            // Default to true for empty Jinja results - pip_check is on by default
            Some(v) => evaluate_bool_value(v, context, "pip_check", true)?,
        };

        let python_version = match &self.python_version {
            None => Stage1PythonVersion::None,
            Some(v) => v.evaluate(context)?,
        };

        Ok(Stage1PythonTest {
            imports,
            pip_check,
            python_version,
        })
    }
}

// Use macro for simple list field evaluations
impl_evaluate_list_fields!(Stage0PerlTest => Stage1PerlTest { uses });
impl_evaluate_list_fields!(Stage0RTest => Stage1RTest { libraries });
impl_evaluate_list_fields!(Stage0RubyTest => Stage1RubyTest { requires });

impl Evaluate for Stage0CommandsTestRequirements {
    type Output = Stage1CommandsTestRequirements;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Self::Output {
            run: evaluate_dependency_list(&self.run, context)?,
            build: evaluate_dependency_list(&self.build, context)?,
        })
    }
}

impl Evaluate for Stage0CommandsTestFiles {
    type Output = Stage1CommandsTestFiles;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1CommandsTestFiles {
            source: evaluate_glob_vec_simple(&self.source, context)?,
            recipe: evaluate_glob_vec_simple(&self.recipe, context)?,
        })
    }
}

impl Evaluate for Stage0CommandsTest {
    type Output = Stage1CommandsTest;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let script = evaluate_script(&self.script, context)?;
        let requirements = evaluate_optional_with_default(&self.requirements, context)?;
        let files = evaluate_optional_with_default(&self.files, context)?;

        Ok(Stage1CommandsTest {
            script,
            requirements,
            files,
        })
    }
}

impl Evaluate for Stage0DownstreamTest {
    type Output = Stage1DownstreamTest;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1DownstreamTest {
            downstream: evaluate_string_value(&self.downstream, context)?,
        })
    }
}

// Note: Can't use macro for PackageContentsCheckFiles because fields need GlobVec conversion

impl Evaluate for Stage0PackageContentsCheckFiles {
    type Output = Stage1PackageContentsCheckFiles;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1PackageContentsCheckFiles {
            exists: evaluate_glob_vec_simple(&self.exists, context)?,
            not_exists: evaluate_glob_vec_simple(&self.not_exists, context)?,
        })
    }
}

impl Evaluate for Stage0PackageContentsTest {
    type Output = Stage1PackageContentsTest;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let files = evaluate_optional_with_default(&self.files, context)?;
        let site_packages = evaluate_optional_with_default(&self.site_packages, context)?;
        let bin = evaluate_optional_with_default(&self.bin, context)?;
        let lib = evaluate_optional_with_default(&self.lib, context)?;
        let include = evaluate_optional_with_default(&self.include, context)?;

        Ok(Stage1PackageContentsTest {
            files,
            site_packages,
            bin,
            lib,
            include,
            strict: self.strict,
        })
    }
}

/// Evaluate a single test item (which can be a Value or Conditional), returning a vector
/// (conditionals expand to multiple tests)
/// Supports nested conditionals at any depth.
pub fn evaluate_test(
    test_item: &Item<Stage0TestType>,
    context: &EvaluationContext,
) -> Result<Vec<Stage1TestType>, ParseError> {
    match test_item {
        Item::Value(value) => {
            // Get the concrete TestType from the Value
            let test = value.as_concrete().ok_or_else(|| {
                ParseError::invalid_value(
                    "test",
                    "test cannot be a template",
                    crate::Span::new_blank(),
                )
            })?;
            evaluate_test_type(test, context).map(|t| vec![t])
        }
        Item::Conditional(cond) => {
            // Evaluate the condition
            let condition_met =
                evaluate_condition(&cond.condition, context, cond.condition_span.as_ref())?;

            let tests_to_evaluate = if condition_met {
                &cond.then
            } else {
                match &cond.else_value {
                    Some(else_value) => else_value,
                    None => return Ok(Vec::new()), // No else branch and condition is false
                }
            };

            // Recursively evaluate the selected tests (supports nested conditionals)
            let mut results = Vec::new();
            for nested_item in tests_to_evaluate.iter() {
                results.extend(evaluate_test(nested_item, context)?);
            }
            Ok(results)
        }
    }
}

/// Evaluate a concrete TestType
fn evaluate_test_type(
    test: &Stage0TestType,
    context: &EvaluationContext,
) -> Result<Stage1TestType, ParseError> {
    match test {
        Stage0TestType::Python { python } => Ok(Stage1TestType::Python {
            python: python.evaluate(context)?,
        }),
        Stage0TestType::Perl { perl } => Ok(Stage1TestType::Perl {
            perl: perl.evaluate(context)?,
        }),
        Stage0TestType::R { r } => Ok(Stage1TestType::R {
            r: r.evaluate(context)?,
        }),
        Stage0TestType::Ruby { ruby } => Ok(Stage1TestType::Ruby {
            ruby: ruby.evaluate(context)?,
        }),
        Stage0TestType::Commands(commands) => {
            Ok(Stage1TestType::Commands(commands.evaluate(context)?))
        }
        Stage0TestType::Downstream(downstream) => {
            Ok(Stage1TestType::Downstream(downstream.evaluate(context)?))
        }
        Stage0TestType::PackageContents { package_contents } => {
            Ok(Stage1TestType::PackageContents {
                package_contents: package_contents.evaluate(context)?,
            })
        }
    }
}

impl Evaluate for Stage0TestType {
    type Output = Stage1TestType;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        evaluate_test_type(self, context)
    }
}

/// Evaluate a single post_process item (which can be a Value or Conditional), returning a vector
/// (conditionals expand to multiple post_process entries)
/// Supports nested conditionals at any depth.
fn evaluate_post_process_item(
    item: &Item<Stage0PostProcess>,
    context: &EvaluationContext,
) -> Result<Vec<Stage1PostProcess>, ParseError> {
    match item {
        Item::Value(value) => {
            // Get the concrete PostProcess from the Value
            let pp = value.as_concrete().ok_or_else(|| {
                ParseError::invalid_value(
                    "post_process",
                    "post_process cannot be a template",
                    crate::Span::new_blank(),
                )
            })?;
            pp.evaluate(context).map(|p| vec![p])
        }
        Item::Conditional(cond) => {
            // Evaluate the condition
            let condition_met =
                evaluate_condition(&cond.condition, context, cond.condition_span.as_ref())?;

            let items_to_evaluate = if condition_met {
                &cond.then
            } else {
                match &cond.else_value {
                    Some(else_value) => else_value,
                    None => return Ok(Vec::new()), // No else branch and condition is false
                }
            };

            // Recursively evaluate the selected items (supports nested conditionals)
            let mut results = Vec::new();
            for nested_item in items_to_evaluate.iter() {
                results.extend(evaluate_post_process_item(nested_item, context)?);
            }
            Ok(results)
        }
    }
}

impl Evaluate for Stage0Recipe {
    type Output = Stage1Recipe;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Track variables accessed before context evaluation so we can identify
        // which variables were accessed during context evaluation (before shadowing)
        let accessed_before_context = context.accessed_variables();

        // Get the set of original variable names (variant variables) before context merge
        let original_variable_keys: HashSet<String> = context.variables().keys().cloned().collect();

        // Evaluate the context variables and merge them into a new context
        let (context_with_vars, evaluated_context) = if !self.context.is_empty() {
            context.with_context(&self.context)?
        } else {
            (context.clone(), IndexMap::new())
        };

        // Variables accessed during context evaluation that were ALSO in the original context.
        // This filters out context variables that were accessed by other context variables.
        // For example, if we have:
        //   context:
        //     build_drafts: ${{ drafts == "ON" }}
        //     build_prefix: "${{ 'drafts_' if build_drafts else '' }}"
        // Then `drafts` is a variant variable (should be tracked), but `build_drafts` is a
        // context variable accessed by `build_prefix` (should NOT be tracked as a variant variable).
        let accessed_during_context: HashSet<String> = context_with_vars
            .accessed_variables()
            .difference(&accessed_before_context)
            .filter(|var| original_variable_keys.contains(*var))
            .cloned()
            .collect();
        let package = self.package.evaluate(&context_with_vars)?;
        let build = self.build.evaluate(&context_with_vars)?;
        let about = self.about.evaluate(&context_with_vars)?;
        let requirements = self.requirements.evaluate(&context_with_vars)?;
        let extra = self.extra.evaluate(&context_with_vars)?;

        // Evaluate source list (conditionals expand to multiple sources)
        let mut source = Vec::new();
        for src in &self.source {
            source.extend(evaluate_source(src, &context_with_vars)?);
        }

        // Evaluate tests list (conditionals expand to multiple tests)
        let mut tests = Vec::new();
        for test in &self.tests {
            tests.extend(evaluate_test(test, &context_with_vars)?);
        }

        let accessed_vars = context_with_vars.accessed_variables();
        let free_specs = requirements
            .free_specs()
            .into_iter()
            .map(NormalizedKey::from)
            .collect::<HashSet<_>>();

        // Get the noarch type to determine which variant keys to exclude
        let noarch = build.noarch.unwrap_or(NoArchType::none());

        // For noarch packages, certain variant keys should be excluded from the hash
        // because these packages work across multiple versions of the language
        let should_exclude_from_variant = |key: &str| -> bool {
            if noarch.is_python() {
                // Python noarch packages should exclude python version from hash
                key == "python"
            } else {
                false
            }
        };

        // Get OS environment variable keys that can be overridden by variant config
        let os_env_var_keys = context_with_vars.os_env_var_keys();

        // Build the actual variant from accessed variables
        // IMPORTANT: For variables that were accessed during context evaluation (i.e., variant
        // variables that were used by context templates), we must use the ORIGINAL value from
        // the variant, not the context-transformed value. This ensures the variant correctly
        // captures the input that varied between builds.
        //
        // Example:
        //   variant: mpi: ["bla", "ble"]
        //   context: mpi: ${{ mpi ~ "foobar" }}
        //
        // The variant should record mpi: "bla", not mpi: "blafoobar"
        let original_variables = context.variables();

        let mut actual_variant: BTreeMap<NormalizedKey, Variable> = context_with_vars
            .variables()
            .iter()
            .filter(|(k, _)| {
                let key_str = k.as_str();

                // Context variables (defined in the context: section) should not be included
                // in the variant, UNLESS they shadow a variant variable that was accessed
                // during context evaluation. This happens with patterns like:
                //   context:
                //     foobar: ${{ foobar }}
                // where foobar in context references and shadows the variant foobar.
                //
                // Since accessed_during_context is already filtered to only contain original
                // variant variables (not context variables), we simply check if this context
                // variable name is in that set.
                if self.context.contains_key(key_str) && !accessed_during_context.contains(key_str)
                {
                    return false;
                }
                // Exclude if it's a noarch-excluded key
                if should_exclude_from_variant(key_str) {
                    return false;
                }

                // Include if accessed, part of our (free) dependencies,
                // if it's an always-include variable, or if it's an OS env var key
                accessed_vars.contains(key_str)
                    || free_specs.contains(&NormalizedKey::from(key_str))
                    || ALWAYS_INCLUDED_VARS.contains(&key_str)
                    || os_env_var_keys.contains(key_str)
            })
            .map(|(k, _)| {
                let key_str = k.as_str();
                // For variables accessed during context evaluation, use the original value
                // from the variant (before context transformation)
                let value = if accessed_during_context.contains(key_str) {
                    original_variables.get(key_str).cloned().unwrap_or_else(|| {
                        context_with_vars.variables().get(key_str).unwrap().clone()
                    })
                } else {
                    context_with_vars.variables().get(key_str).unwrap().clone()
                };
                (NormalizedKey::from(key_str), value)
            })
            .collect();

        // Ensure that `target_platform` is set to "noarch" for noarch packages
        if !noarch.is_none() {
            actual_variant.insert("target_platform".into(), Variable::from_string("noarch"));
        }

        // Add virtual packages from run requirements to the variant
        // Virtual packages (starting with '__') should be included in the hash
        for dep in &requirements.run {
            if let crate::stage1::Dependency::Spec(spec) = dep
                && let Some(ref matcher) = spec.name
                && let rattler_conda_types::PackageNameMatcher::Exact(pkg_name) = matcher
                && pkg_name.as_normalized().starts_with("__")
            {
                actual_variant.insert(
                    NormalizedKey::from(pkg_name.as_normalized()),
                    Variable::from(spec.to_string()),
                );
            }
        }

        // DO NOT compute hash here! Hash computation must happen AFTER pin_subpackages are
        // added to the variant (which happens in variant_render.rs after evaluation).
        // The build string will remain unresolved until finalize_build_strings() is called.

        Ok(Stage1Recipe::new(
            package,
            build,
            about,
            requirements,
            extra,
            source,
            tests,
            evaluated_context,
            actual_variant,
        ))
    }
}

/// Merge two Stage1 Build configurations
/// The output build takes precedence, but if output has default/empty values, use top-level
fn merge_stage1_build(
    toplevel: crate::stage1::Build,
    output: crate::stage1::Build,
) -> crate::stage1::Build {
    // Script: use output if not default, otherwise inherit from top-level
    let script = if output.script.is_default() {
        toplevel.script
    } else {
        output.script
    };

    // Build string: use output unless it's Default, then inherit from top-level
    let string = match output.string {
        BuildString::Default => toplevel.string,
        _ => output.string,
    };

    // Build number: use output if set, otherwise inherit from top-level
    let number = output.number.or(toplevel.number);

    // Noarch: inherit from top-level if not set in output
    let noarch = output.noarch.or(toplevel.noarch);

    // Python: use output if not default, otherwise inherit from top-level
    let python = if output.python.is_default() {
        toplevel.python
    } else {
        output.python
    };

    // Skip: combine top-level and output skip with OR logic
    let skip = toplevel.skip || output.skip;

    // Always copy files: use output if not empty, otherwise inherit from top-level
    let always_copy_files = if output.always_copy_files.is_empty() {
        toplevel.always_copy_files
    } else {
        output.always_copy_files
    };

    // Always include files: use output if not empty, otherwise inherit from top-level
    let always_include_files = if output.always_include_files.is_empty() {
        toplevel.always_include_files
    } else {
        output.always_include_files
    };

    // Merge build and host envs: use OR logic (true if either is true)
    let merge_build_and_host_envs =
        output.merge_build_and_host_envs || toplevel.merge_build_and_host_envs;

    // Files: use output if not empty, otherwise inherit from top-level
    let files = if output.files.is_empty() {
        toplevel.files
    } else {
        output.files
    };

    // Dynamic linking: use output if not default, otherwise inherit from top-level
    let dynamic_linking = if output.dynamic_linking.is_default() {
        toplevel.dynamic_linking
    } else {
        output.dynamic_linking
    };

    // Variant: use output if not default, otherwise inherit from top-level
    let variant = if output.variant.is_default() {
        toplevel.variant
    } else {
        output.variant
    };

    // Prefix detection: use output if not default, otherwise inherit from top-level
    let prefix_detection = if output.prefix_detection.is_default() {
        toplevel.prefix_detection
    } else {
        output.prefix_detection
    };

    // Post-process: use output if not empty, otherwise inherit from top-level
    let post_process = if output.post_process.is_empty() {
        toplevel.post_process
    } else {
        output.post_process
    };

    stage1::Build {
        script,
        number,
        string,
        noarch,
        python,
        skip,
        always_copy_files,
        always_include_files,
        merge_build_and_host_envs,
        files,
        dynamic_linking,
        variant,
        prefix_detection,
        post_process,
    }
}

/// Merge two Stage1 About configurations
/// The output about takes precedence for non-empty fields
fn merge_stage1_about(toplevel: stage1::About, output: stage1::About) -> stage1::About {
    stage1::About {
        homepage: if output.homepage.is_some() {
            output.homepage
        } else {
            toplevel.homepage
        },
        repository: if output.repository.is_some() {
            output.repository
        } else {
            toplevel.repository
        },
        documentation: if output.documentation.is_some() {
            output.documentation
        } else {
            toplevel.documentation
        },
        license: if output.license.is_some() {
            output.license
        } else {
            toplevel.license
        },
        license_family: if output.license_family.is_some() {
            output.license_family
        } else {
            toplevel.license_family
        },
        license_file: if output.license_file.is_some() {
            output.license_file
        } else {
            toplevel.license_file
        },
        summary: if output.summary.is_some() {
            output.summary
        } else {
            toplevel.summary
        },
        description: if output.description.is_some() {
            output.description
        } else {
            toplevel.description
        },
    }
}

/// Helper to evaluate a package output into a Stage1Recipe
/// This handles merging top-level recipe sections with output-specific sections
fn evaluate_package_output_to_recipe(
    output: &stage0::PackageOutput,
    recipe: &stage0::MultiOutputRecipe,
    context: &EvaluationContext,
    staging_caches: &IndexMap<String, crate::stage1::StagingCache>,
    accessed_during_context: &HashSet<String>,
) -> Result<Option<Stage1Recipe>, ParseError> {
    // Evaluate package name
    let name_str = evaluate_value_to_string(&output.package.name, context)?;
    let name = PackageName::from_str(&name_str).map_err(|e| {
        ParseError::invalid_value(
            "name",
            format!(
                "invalid value for name: '{}' is not a valid package name: {}",
                name_str, e
            ),
            Span::new_blank(),
        )
    })?;

    // Get version from output or fallback to recipe-level version
    let version_str = if let Some(ref version_value) = output.package.version {
        evaluate_value_to_string(version_value, context)?
    } else if let Some(ref version_value) = recipe.recipe.version {
        evaluate_value_to_string(version_value, context)?
    } else {
        return Err(ParseError::missing_field(
            "version is required for package output",
            Span::new_blank(),
        ));
    };

    let version = VersionWithSource::from_str(&version_str).map_err(|e| {
        ParseError::invalid_value(
            "version",
            format!(
                "invalid value for version: '{}' is not a valid version: {}",
                version_str, e
            ),
            Span::new_blank(),
        )
    })?;

    let package = Stage1Package::new(name, version);

    // Check if this output inherits from top-level
    let inherits_from_toplevel = matches!(output.inherit, crate::stage0::Inherit::TopLevel);

    // Evaluate build section
    // Merging strategy depends on what the output inherits from:
    // - Top-level inheritance: merge everything including script
    // - Cache inheritance: merge build settings (dynamic_linking, etc.) but NOT script
    //   (the cache has its own script, and the output doesn't need one for filtering files)
    let build = if inherits_from_toplevel {
        // Full merge including script
        let toplevel_build = recipe.build.evaluate(context)?;
        let output_build = output.build.evaluate(context)?;
        merge_stage1_build(toplevel_build, output_build)
    } else {
        // Cache inheritance: only merge non-script build settings
        let toplevel_build = recipe.build.evaluate(context)?;
        let mut output_build = output.build.evaluate(context)?;

        // Only inherit specific build settings from top-level, not the script
        if output_build.dynamic_linking.is_default() {
            output_build.dynamic_linking = toplevel_build.dynamic_linking;
        }
        if output_build.prefix_detection.is_default() {
            output_build.prefix_detection = toplevel_build.prefix_detection;
        }
        if output_build.variant.is_default() {
            output_build.variant = toplevel_build.variant;
        }
        if output_build.noarch.is_none() {
            output_build.noarch = toplevel_build.noarch;
        }
        if output_build.python.is_default() {
            output_build.python = toplevel_build.python;
        }
        if output_build.always_copy_files.is_empty() {
            output_build.always_copy_files = toplevel_build.always_copy_files;
        }
        if output_build.always_include_files.is_empty() {
            output_build.always_include_files = toplevel_build.always_include_files;
        }
        if !toplevel_build.merge_build_and_host_envs && output_build.merge_build_and_host_envs {
            output_build.merge_build_and_host_envs = toplevel_build.merge_build_and_host_envs;
        }
        if output_build.post_process.is_empty() {
            output_build.post_process = toplevel_build.post_process;
        }
        // Combine skip with OR logic
        output_build.skip = output_build.skip || toplevel_build.skip;

        output_build
    };

    // Check if this output is skipped (already eagerly evaluated during Build::evaluate).
    // If skipped, return a minimal recipe early - this is like an outer `if: ...` conditional.
    // We skip evaluating requirements, tests, sources, etc. to avoid errors from
    // platform-specific functions (like stdlib('c') on Windows) that would fail.
    if build.skip {
        return Ok(None);
    }

    // Evaluate about section
    // Always merge top-level about with output about (output fields take precedence)
    // This ensures that all outputs inherit package metadata like license and repository
    let about = {
        let toplevel_about = recipe.about.evaluate(context)?;
        let output_about = output.about.evaluate(context)?;

        // Merge: output-specific fields take precedence
        merge_stage1_about(toplevel_about, output_about)
    };

    // Evaluate requirements
    let requirements = output.requirements.evaluate(context)?;

    // Use recipe-level extra (outputs don't have their own extra)
    let extra = recipe.extra.evaluate(context)?;

    // Evaluate source list (conditionals expand to multiple sources)
    // If inheriting from top-level, prepend top-level sources
    let mut source = Vec::new();
    if inherits_from_toplevel {
        for src in &recipe.source {
            source.extend(evaluate_source(src, context)?);
        }
    }
    for src in &output.source {
        source.extend(evaluate_source(src, context)?);
    }

    // Evaluate tests list
    // If inheriting from top-level, prepend top-level tests
    let mut tests = Vec::new();
    if inherits_from_toplevel {
        for test in &recipe.tests {
            tests.extend(evaluate_test(test, context)?);
        }
    }
    for test in &output.tests {
        tests.extend(evaluate_test(test, context)?);
    }

    // Extract the resolved context variables
    let resolved_context = context.variables().clone();

    let accessed_vars = context.accessed_variables();
    let mut free_specs = requirements
        .free_specs()
        .into_iter()
        .map(NormalizedKey::from)
        .collect::<HashSet<_>>();

    // If this output inherits from a staging cache, also include the staging cache's free_specs
    // This ensures that variant variables from the staging cache are included in the hash
    match &output.inherit {
        crate::stage0::Inherit::CacheName(cache_name_value) => {
            let cache_name = evaluate_string_value(cache_name_value, context)?;
            if let Some(cache) = staging_caches.get(&cache_name) {
                // Add the staging cache's free specs to our free specs
                for spec in cache.requirements.free_specs() {
                    free_specs.insert(NormalizedKey::from(spec));
                }
            }
        }
        crate::stage0::Inherit::CacheWithOptions(cache_inherit) => {
            let cache_name = evaluate_string_value(&cache_inherit.from, context)?;
            if let Some(cache) = staging_caches.get(&cache_name) {
                // Add the staging cache's free specs to our free specs
                for spec in cache.requirements.free_specs() {
                    free_specs.insert(NormalizedKey::from(spec));
                }
            }
        }
        crate::stage0::Inherit::TopLevel => {
            // No staging cache, nothing to add
        }
    }

    // Get the noarch type to determine which variant keys to exclude
    let noarch = build.noarch.unwrap_or(NoArchType::none());

    // For noarch packages, certain variant keys should be excluded from the hash
    let should_exclude_from_variant = |key: &str| -> bool {
        if noarch.is_python() {
            key == "python"
        } else {
            false
        }
    };

    let mut actual_variant: BTreeMap<NormalizedKey, Variable> = context
        .variables()
        .iter()
        .filter(|(k, _)| {
            let key_str = k.as_str();

            // Context variables (defined in the context: section) should not be included
            // in the variant, UNLESS they shadow a variant variable that was accessed
            // during context evaluation. Since accessed_during_context is already filtered
            // to only contain original variant variables, we simply check membership.
            if recipe.context.contains_key(key_str) && !accessed_during_context.contains(key_str) {
                return false;
            }
            // Exclude if it's a noarch-excluded key
            if should_exclude_from_variant(key_str) {
                return false;
            }

            // Include if accessed (either during this output evaluation or during context
            // evaluation), part of our (free) dependencies, if it's an always-include variable,
            // or if it's an OS env var key
            let os_env_var_keys = context.os_env_var_keys();
            accessed_vars.contains(key_str)
                || accessed_during_context.contains(key_str)
                || free_specs.contains(&NormalizedKey::from(key_str))
                || ALWAYS_INCLUDED_VARS.contains(&key_str)
                || os_env_var_keys.contains(key_str)
        })
        .map(|(k, v)| (NormalizedKey::from(k.as_str()), v.clone()))
        .collect();

    // Ensure that `target_platform` is set to "noarch" for noarch packages
    if !noarch.is_none() {
        actual_variant.insert("target_platform".into(), Variable::from_string("noarch"));
    }

    // Add virtual packages from run requirements to the variant
    // Virtual packages (starting with '__') should be included in the hash
    for dep in &requirements.run {
        if let crate::stage1::Dependency::Spec(spec) = dep
            && let Some(ref matcher) = spec.name
            && let rattler_conda_types::PackageNameMatcher::Exact(pkg_name) = matcher
            && pkg_name.as_normalized().starts_with("__")
        {
            actual_variant.insert(
                NormalizedKey::from(pkg_name.as_normalized()),
                Variable::from(spec.to_string()),
            );
        }
    }

    // DO NOT compute hash here! Hash computation must happen AFTER pin_subpackages are
    // added to the variant (which happens in variant_render.rs after evaluation).
    // The build string will remain unresolved until finalize_build_strings() is called.

    Ok(Some(Stage1Recipe::new(
        package,
        build,
        about,
        requirements,
        extra,
        source,
        tests,
        resolved_context,
        actual_variant,
    )))
}

/// Implement Evaluate for MultiOutputRecipe
/// Returns a Vec of Stage1Recipe, one for each output
impl Evaluate for crate::stage0::MultiOutputRecipe {
    type Output = Vec<Stage1Recipe>;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        use crate::stage1::StagingCache;

        // Track variables accessed before context evaluation so we can identify
        // which variables were accessed during context evaluation (before shadowing)
        let accessed_before_context = context.accessed_variables();

        // Get the set of original variable names (variant variables) before context merge
        let original_variable_keys: HashSet<String> = context.variables().keys().cloned().collect();

        // Evaluate the context variables and merge them into a new context
        let (context_with_vars, evaluated_context) = if !self.context.is_empty() {
            context.with_context(&self.context)?
        } else {
            (context.clone(), IndexMap::new())
        };

        // Variables accessed during context evaluation that were ALSO in the original context.
        // This filters out context variables that were accessed by other context variables.
        let accessed_during_context: HashSet<String> = context_with_vars
            .accessed_variables()
            .difference(&accessed_before_context)
            .filter(|var| original_variable_keys.contains(*var))
            .cloned()
            .collect();

        // First pass: Evaluate all staging outputs and collect them
        let mut staging_caches = IndexMap::new();
        for output in &self.outputs {
            if let crate::stage0::Output::Staging(staging_output) = output {
                context_with_vars.clear_accessed();

                let staging_name =
                    evaluate_string_value(&staging_output.staging.name, &context_with_vars)?;

                // Evaluate staging output components
                let script = evaluate_script(&staging_output.build.script, &context_with_vars)?;
                let build = crate::stage1::Build {
                    script,
                    ..crate::stage1::Build::default()
                };

                let requirements = staging_output.requirements.evaluate(&context_with_vars)?;

                // Staging outputs inherit top-level sources (prepend), then add their own
                // (conditionals expand to multiple sources)
                let mut source = Vec::new();
                for src in &self.source {
                    source.extend(evaluate_source(src, &context_with_vars)?);
                }
                for src in &staging_output.source {
                    source.extend(evaluate_source(src, &context_with_vars)?);
                }

                // Compute variant for staging output
                let accessed_vars = context_with_vars.accessed_variables();
                let free_specs = requirements
                    .free_specs()
                    .into_iter()
                    .map(NormalizedKey::from)
                    .collect::<HashSet<_>>();

                const ALWAYS_INCLUDE: &[&str] =
                    &["target_platform", "channel_targets", "channel_sources"];

                let os_env_var_keys = context_with_vars.os_env_var_keys();
                let actual_variant: BTreeMap<NormalizedKey, Variable> = context_with_vars
                    .variables()
                    .iter()
                    .filter(|(k, _)| {
                        let key_str = k.as_str();
                        // Exclude context variables UNLESS they were accessed from the variant
                        // DURING context evaluation (before shadowing)
                        if self.context.contains_key(key_str)
                            && !accessed_during_context.contains(key_str)
                        {
                            return false;
                        }
                        accessed_vars.contains(key_str)
                            || accessed_during_context.contains(key_str)
                            || free_specs.contains(&NormalizedKey::from(key_str))
                            || ALWAYS_INCLUDE.contains(&key_str)
                            || os_env_var_keys.contains(key_str)
                    })
                    .map(|(k, v)| (NormalizedKey::from(k.as_str()), v.clone()))
                    .collect();

                let staging_cache = StagingCache::new(
                    staging_name.clone(),
                    build,
                    requirements,
                    source,
                    actual_variant,
                );

                staging_caches.insert(staging_name, staging_cache);
            }
        }

        let mut evaluated_outputs = Vec::new();

        // Second pass: Evaluate package outputs only and set their staging cache dependencies
        for output in &self.outputs {
            // Only process package outputs - staging outputs are not converted to recipes
            if let crate::stage0::Output::Package(pkg_output) = output {
                context_with_vars.clear_accessed();

                let Some(mut recipe) = evaluate_package_output_to_recipe(
                    pkg_output.as_ref(),
                    self,
                    &context_with_vars,
                    &staging_caches,
                    &accessed_during_context,
                )?
                else {
                    continue;
                };

                recipe.context = evaluated_context.clone();

                // Set staging_caches and inherits_from based on the inherit field
                match &pkg_output.inherit {
                    crate::stage0::Inherit::CacheName(cache_name_value) => {
                        let cache_name =
                            evaluate_string_value(cache_name_value, &context_with_vars)?;
                        if let Some(cache) = staging_caches.get(&cache_name) {
                            recipe.staging_caches = vec![cache.clone()];
                            recipe.inherits_from =
                                Some(crate::stage1::InheritsFrom::new(cache_name));
                        } else {
                            return Err(ParseError::invalid_value(
                                "inherit",
                                format!(
                                    "Staging cache '{}' not found. Available caches: {}",
                                    cache_name,
                                    if staging_caches.is_empty() {
                                        "none".to_string()
                                    } else {
                                        staging_caches
                                            .keys()
                                            .map(|k| k.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    }
                                ),
                                cache_name_value
                                    .span()
                                    .copied()
                                    .unwrap_or_else(Span::new_blank),
                            ));
                        }
                    }
                    crate::stage0::Inherit::CacheWithOptions(cache_inherit) => {
                        let cache_name =
                            evaluate_string_value(&cache_inherit.from, &context_with_vars)?;
                        if let Some(cache) = staging_caches.get(&cache_name) {
                            recipe.staging_caches = vec![cache.clone()];
                            recipe.inherits_from =
                                Some(crate::stage1::InheritsFrom::with_run_exports(
                                    cache_name,
                                    cache_inherit.run_exports,
                                ));
                        } else {
                            return Err(ParseError::invalid_value(
                                "inherit.from",
                                format!(
                                    "Staging cache '{}' not found. Available caches: {}",
                                    cache_name,
                                    if staging_caches.is_empty() {
                                        "none".to_string()
                                    } else {
                                        staging_caches
                                            .keys()
                                            .map(|k| k.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    }
                                ),
                                cache_inherit
                                    .from
                                    .span()
                                    .copied()
                                    .unwrap_or_else(Span::new_blank),
                            ));
                        }
                    }
                    crate::stage0::Inherit::TopLevel => {
                        // No staging cache dependency
                        recipe.staging_caches = Vec::new();
                        recipe.inherits_from = None;
                    }
                }

                evaluated_outputs.push(recipe);
            }
        }

        Ok(evaluated_outputs)
    }
}

#[cfg(test)]
mod tests {
    use minijinja::UndefinedBehavior;
    use rattler_build_jinja::{JinjaConfig, Variable};

    use super::*;
    use crate::stage0::{
        self,
        types::{Conditional, ConditionalList, Item, JinjaTemplate, NestedItemList, Value},
    };

    #[test]
    fn test_evaluate_condition_simple() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(true));
        ctx.insert("win".to_string(), Variable::from(false));

        let expr = JinjaExpression::new("unix".to_string()).unwrap();
        assert!(evaluate_condition(&expr, &ctx, None).unwrap());

        let expr2 = JinjaExpression::new("win".to_string()).unwrap();
        assert!(!evaluate_condition(&expr2, &ctx, None).unwrap());
    }

    #[test]
    fn test_evaluate_condition_not() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(true));
        ctx.insert("win".to_string(), Variable::from(false));

        let expr = JinjaExpression::new("not unix".to_string()).unwrap();
        assert!(!evaluate_condition(&expr, &ctx, None).unwrap());

        let expr2 = JinjaExpression::new("not win".to_string()).unwrap();
        assert!(evaluate_condition(&expr2, &ctx, None).unwrap());
    }

    #[test]
    fn test_render_template_simple() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), Variable::from_string("foo"));
        ctx.insert("version".to_string(), Variable::from_string("1.0.0"));

        let template = "${{ name }}-${{ version }}";
        let result = render_template(template, &ctx, None).unwrap();
        assert_eq!(result, "foo-1.0.0");
    }

    #[test]
    fn test_evaluate_string_value_concrete() {
        let value = Value::new_concrete("hello".to_string(), None);
        let ctx = EvaluationContext::new();

        let result = evaluate_string_value(&value, &ctx).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_evaluate_string_value_template() {
        let value = Value::new_template(
            JinjaTemplate::new("${{ greeting }}, ${{ name }}!".to_string()).unwrap(),
            None,
        );

        let mut ctx = EvaluationContext::new();
        ctx.insert("greeting".to_string(), Variable::from_string("Hello"));
        ctx.insert("name".to_string(), Variable::from_string("World"));

        let result = evaluate_string_value(&value, &ctx).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_evaluate_string_list_simple() {
        let list = ConditionalList::new(vec![
            Item::Value(Value::new_concrete("gcc".to_string(), None)),
            Item::Value(Value::new_concrete("make".to_string(), None)),
        ]);

        let ctx = EvaluationContext::new();
        let result = evaluate_string_list(&list, &ctx).unwrap();
        assert_eq!(result, vec!["gcc", "make"]);
    }

    #[test]
    fn test_evaluate_string_list_with_conditional() {
        let list = ConditionalList::new(vec![
            Item::Value(Value::new_concrete("python".to_string(), None)),
            Item::Conditional(Conditional {
                condition: JinjaExpression::new("unix".to_string()).unwrap(),
                then: NestedItemList::new(vec![Item::Value(Value::new_concrete(
                    "gcc".to_string(),
                    None,
                ))]),
                else_value: Some(NestedItemList::new(vec![Item::Value(Value::new_concrete(
                    "msvc".to_string(),
                    None,
                ))])),
                condition_span: None,
            }),
        ]);

        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(true));

        let result = evaluate_string_list(&list, &ctx).unwrap();
        assert_eq!(result, vec!["python", "gcc"]);

        // Test with unix set to false
        let mut ctx2 = EvaluationContext::new();
        ctx2.insert("unix".to_string(), Variable::from(false));
        let result2 = evaluate_string_list(&list, &ctx2).unwrap();
        assert_eq!(result2, vec!["python", "msvc"]);
    }

    #[test]
    fn test_variable_tracking_simple() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), Variable::from_string("foo"));
        ctx.insert("version".to_string(), Variable::from_string("1.0.0"));
        ctx.insert("unused".to_string(), Variable::from_string("bar"));

        // Before rendering, no variables should be accessed
        assert!(ctx.accessed_variables().is_empty());

        // Render a template that uses name and version
        let template = "${{ name }}-${{ version }}";
        let result = render_template(template, &ctx, None).unwrap();
        assert_eq!(result, "foo-1.0.0");

        // After rendering, name and version should be tracked, but not unused
        let accessed = ctx.accessed_variables();
        assert_eq!(accessed.len(), 2);
        assert!(accessed.contains("name"));
        assert!(accessed.contains("version"));
        assert!(!accessed.contains("unused"));
    }

    #[test]
    fn test_variable_tracking_with_conditionals() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from_string("true"));
        ctx.insert("unix_var".to_string(), Variable::from_string("gcc"));
        ctx.insert("win_var".to_string(), Variable::from_string("msvc"));

        let list = ConditionalList::new(vec![Item::Conditional(Conditional {
            condition: JinjaExpression::new("unix".to_string()).unwrap(),
            then: NestedItemList::new(vec![Item::Value(Value::new_concrete(
                "gcc".to_string(),
                None,
            ))]),
            else_value: Some(NestedItemList::new(vec![Item::Value(Value::new_concrete(
                "msvc".to_string(),
                None,
            ))])),
            condition_span: None,
        })]);

        // Evaluate the list - only unix branch is taken
        let _result = evaluate_string_list(&list, &ctx).unwrap();

        // Now we DO track variables from conditional expressions
        // The condition "unix" is evaluated through Jinja's eval method,
        // which tracks accessed variables
        let accessed = ctx.accessed_variables();
        // The conditional expression "unix" should be tracked
        assert_eq!(accessed.len(), 1);
        assert!(accessed.contains("unix"));
    }

    #[test]
    fn test_variable_tracking_with_template() {
        // Create variant with compiler configuration
        let mut variant = BTreeMap::new();
        variant.insert("c_compiler".into(), Variable::from_string("supergcc"));
        variant.insert("c_compiler_version".into(), Variable::from_string("15.0"));

        // Create JinjaConfig with the variant and target_platform
        let jinja_config = JinjaConfig {
            variant: variant.clone(),
            target_platform: rattler_conda_types::Platform::Linux64,
            build_platform: rattler_conda_types::Platform::Linux64,
            host_platform: rattler_conda_types::Platform::Linux64,
            ..Default::default()
        };

        // Create context with variables and config
        let mut variables = IndexMap::new();
        variables.insert("c_compiler".to_string(), Variable::from_string("supergcc"));
        variables.insert(
            "c_compiler_version".to_string(),
            Variable::from_string("15.0"),
        );
        variables.insert("version".to_string(), Variable::from_string("1.0.0"));

        let ctx = EvaluationContext::with_variables_and_config(variables, jinja_config);

        // Create a list with templates that will be evaluated
        let list = ConditionalList::new(vec![
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap(),
                None,
            )),
            Item::Value(Value::new_concrete("static-dep".to_string(), None)),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ version }}".to_string()).unwrap(),
                None,
            )),
        ]);

        let result = evaluate_string_list(&list, &ctx).unwrap();

        // Variables used during template rendering (functions like `compiler` are excluded)
        let accessed = ctx.accessed_variables();
        assert_eq!(accessed.len(), 4);
        assert!(accessed.contains("c_compiler"));
        assert!(accessed.contains("c_compiler_version"));
        assert!(accessed.contains("CONDA_BUILD_SYSROOT"));
        assert!(accessed.contains("version"));
        // Note: compiler() adds '=' before alphanumeric versions
        assert_eq!(result[0], "supergcc_linux-64 =15.0");
    }

    #[test]
    fn test_variable_tracking_clear() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), Variable::from_string("foo"));

        // Render a template
        let template = "${{ name }}";
        let _result = render_template(template, &ctx, None).unwrap();

        // Variable should be tracked
        assert!(ctx.accessed_variables().contains("name"));

        // Clear the tracker
        ctx.clear_accessed();

        // Variables should be empty now
        assert!(ctx.accessed_variables().is_empty());
    }

    #[test]
    fn test_multiple_undefined_variables() {
        let ctx = EvaluationContext::new();
        // No variables set

        let template = "${{ platform }} for ${{ arch }}";
        let result = render_template(template, &ctx, None);

        assert!(result.is_err());

        // First undefined variable encountered is 'platform'
        let undefined = ctx.undefined_variables();
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains("platform"));

        let accessed = ctx.accessed_variables();
        assert_eq!(accessed.len(), 1);
        assert!(accessed.contains("platform"));
    }

    #[test]
    fn test_multiple_undefined_variables_lenient() {
        let jinja_config = JinjaConfig {
            undefined_behavior: UndefinedBehavior::Lenient,
            ..Default::default()
        };
        let ctx = EvaluationContext::with_variables_and_config(IndexMap::new(), jinja_config);
        // No variables set

        let template = "${{ platform }} for ${{ arch }}";
        let result = render_template(template, &ctx, None);

        assert!(result.is_ok());
        assert!(result.unwrap() == " for ");

        // First undefined variable encountered is 'platform'
        let undefined = ctx.undefined_variables();
        assert_eq!(undefined.len(), 2);
        assert!(undefined.contains("platform"));
        assert!(undefined.contains("arch"));

        let accessed = ctx.accessed_variables();
        assert_eq!(accessed.len(), 2);
        assert!(accessed.contains("platform"));
        assert!(accessed.contains("arch"));
    }

    #[test]
    fn test_undefined_variable_in_conditional_reports_correct_line() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        // The conditional 'if: undefined_var' is on line 12
        let recipe_yaml = r#"schema_version: 1

package:
  name: test-pkg
  version: 1.0.0

requirements:
  run:
    - python
    - if: undefined_var
      then:
        - numpy
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::SingleOutput(recipe) => {
                let ctx = EvaluationContext::new();
                let result = recipe.evaluate(&ctx);

                assert!(result.is_err());
                let err = result.unwrap_err();

                // Check that the error span points to the correct line
                let span = err.span();
                assert!(
                    span.start().is_some(),
                    "Error span should have start position"
                );
                let start = span.start().unwrap();
                // The 'if: undefined_var' is on line 10 (1-indexed)
                assert_eq!(
                    start.line(),
                    10,
                    "Error should point to line 10 where 'undefined_var' is used"
                );
            }
            _ => panic!("Expected single recipe"),
        }
    }

    #[test]
    fn test_undefined_variable_in_template_reports_correct_line() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        // The undefined variable is used in the package name on line 7
        let recipe_yaml = r#"schema_version: 1

package:
  name: ${{ UNDEFINED }}
  version: 1.0.0
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::SingleOutput(recipe) => {
                let ctx = EvaluationContext::new();
                let result = recipe.evaluate(&ctx);

                assert!(result.is_err());
                let err = result.unwrap_err();

                // Check that the error span points to the correct line
                let span = err.span();
                assert!(
                    span.start().is_some(),
                    "Error span should have start position"
                );
                let start = span.start().unwrap();
                // The '${{ UNDEFINED }}' is on line 4 (1-indexed)
                assert_eq!(
                    start.line(),
                    4,
                    "Error should point to line 4 where 'UNDEFINED' is used in the package name"
                );
            }
            _ => panic!("Expected single recipe"),
        }
    }

    #[test]
    fn test_undefined_variable_in_inline_conditional_reports_correct_line() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        // The undefined variable is used in an inline conditional on line 4
        let recipe_yaml = r#"schema_version: 1

package:
  name: ${{ "foo" if notsetstring else "bar" }}
  version: 1.0.0
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::SingleOutput(recipe) => {
                let ctx = EvaluationContext::new();
                let result = recipe.evaluate(&ctx);

                assert!(
                    result.is_err(),
                    "Should error on undefined variable in inline conditional"
                );
                let err = result.unwrap_err();

                // Check that the error span points to the correct line
                let span = err.span();
                assert!(
                    span.start().is_some(),
                    "Error span should have start position"
                );
                let start = span.start().unwrap();
                // The inline conditional with undefined variable is on line 4 (1-indexed)
                assert_eq!(
                    start.line(),
                    4,
                    "Error should point to line 4 where 'notsetstring' is used in inline conditional"
                );
            }
            _ => panic!("Expected single recipe"),
        }
    }

    #[test]
    fn test_multi_output_staging_cache_populated() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

outputs:
  - staging:
      name: build-cache
    requirements:
      build:
        - gcc
        - cmake
      host:
        - zlib
    build:
      script:
        - echo "Building"

  - package:
      name: mylib
      version: 1.0.0
    inherit: build-cache
    requirements:
      run:
        - libgcc
    about:
      summary: My library
      license: MIT

  - package:
      name: mylib-dev
      version: 1.0.0
    inherit:
      from: build-cache
      run_exports: false
    requirements:
      run:
        - mylib
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();

                // Should have 2 outputs: only package outputs (staging is not converted to recipe)
                assert_eq!(recipes.len(), 2);

                // First output is mylib (inherits from build-cache with short form)
                let mylib_recipe = &recipes[0];
                assert_eq!(mylib_recipe.package.name.as_normalized(), "mylib");
                assert_eq!(mylib_recipe.staging_caches.len(), 1); // Should have the staging cache
                assert_eq!(mylib_recipe.staging_caches[0].name, "build-cache");
                assert!(mylib_recipe.inherits_from.is_some());
                let inherits = mylib_recipe.inherits_from.as_ref().unwrap();
                assert_eq!(inherits.cache_name, "build-cache");
                assert!(inherits.inherit_run_exports); // Default is true

                // Second output is mylib-dev (inherits with run_exports: false)
                let mylib_dev_recipe = &recipes[1];
                assert_eq!(mylib_dev_recipe.package.name.as_normalized(), "mylib-dev");
                assert_eq!(mylib_dev_recipe.staging_caches.len(), 1);
                assert_eq!(mylib_dev_recipe.staging_caches[0].name, "build-cache");
                assert!(mylib_dev_recipe.inherits_from.is_some());
                let inherits_dev = mylib_dev_recipe.inherits_from.as_ref().unwrap();
                assert_eq!(inherits_dev.cache_name, "build-cache");
                assert!(!inherits_dev.inherit_run_exports); // Explicitly set to false

                // Verify staging cache structure (from mylib's staging_caches)
                let staging_cache = &mylib_recipe.staging_caches[0];
                assert_eq!(staging_cache.name, "build-cache");
                assert!(!staging_cache.requirements.build.is_empty()); // Should have gcc, cmake
                assert!(!staging_cache.requirements.host.is_empty()); // Should have zlib
                assert!(staging_cache.requirements.run.is_empty()); // Staging has no run requirements
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_top_level_inherit() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

outputs:
  - package:
      name: mylib
      version: 1.0.0
    inherit: ~
    about:
      summary: My library
      license: MIT
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();

                assert_eq!(recipes.len(), 1);
                let recipe = &recipes[0];

                // Top-level inheritance should have no staging caches
                assert!(recipe.staging_caches.is_empty());
                assert!(recipe.inherits_from.is_none());
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_build_field_inheritance() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

build:
  number: 5
  noarch: python
  skip:
    - win
  always_copy_files:
    - "*.txt"

outputs:
  - package:
      name: output-with-defaults
      version: 1.0.0

  - package:
      name: output-with-overrides
      version: 1.0.0
    build:
      number: 10
      skip:
        - osx
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                println!("{:#?}", recipes);
                assert_eq!(recipes.len(), 2);

                // First output: inherits everything from top-level
                let output1 = &recipes[0];
                assert_eq!(output1.package.name.as_normalized(), "output-with-defaults");
                assert_eq!(output1.build.number, Some(5)); // Inherited
                assert_eq!(
                    output1.build.noarch,
                    Some(rattler_conda_types::NoArchType::python())
                ); // Inherited
                // Skip "win" evaluates to false on linux-64
                assert!(!output1.build.skip); // Inherited, but not skipped
                assert!(!output1.build.always_copy_files.is_empty()); // Inherited

                // Second output: overrides some fields
                let output2 = &recipes[1];
                assert_eq!(
                    output2.package.name.as_normalized(),
                    "output-with-overrides"
                );
                assert_eq!(output2.build.number, Some(10)); // Overridden
                assert_eq!(
                    output2.build.noarch,
                    Some(rattler_conda_types::NoArchType::python())
                ); // Inherited
                // Skip combines "win" and "osx" with OR, both false on linux-64
                assert!(!output2.build.skip);
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_build_number_zero_not_inherited() {
        // Regression test: build number 0 should NOT be inherited from top-level
        // when explicitly set in an output (0 is a valid build number)
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

context:
  build_number: 1
  server_build_number: 0
  server_version: "2.3.0"
  version: "5.2.0"

recipe:
  name: jupyter-lsp-meta
  version: ${{ version }}

build:
  number: 1234
  noarch: python

outputs:
  - package:
      name: jupyterlab-lsp
      version: ${{ version }}
    build:
      number: ${{ build_number }}

  - package:
      name: jupyter-lsp
      version: ${{ server_version }}
    build:
      number: ${{ server_build_number }}
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                assert_eq!(recipes.len(), 2);

                // First output: jupyterlab-lsp with build_number = 1
                let output1 = &recipes[0];
                assert_eq!(output1.package.name.as_normalized(), "jupyterlab-lsp");
                assert_eq!(output1.build.number, Some(1)); // From context variable

                // Second output: jupyter-lsp with server_build_number = 0
                // This is the key assertion - 0 should NOT be replaced with 1234
                let output2 = &recipes[1];
                assert_eq!(output2.package.name.as_normalized(), "jupyter-lsp");
                assert_eq!(output2.build.number, Some(0)); // Explicitly set to 0, NOT inherited 1234
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_source_inheritance() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

source:
  - url: https://example.com/top-level.tar.gz
    sha256: 0000000000000000000000000000000000000000000000000000000000000000

outputs:
  - staging:
      name: build-cache
    source:
      - url: https://example.com/staging.tar.gz
        sha256: 1111111111111111111111111111111111111111111111111111111111111111

  - package:
      name: pkg-inherit-toplevel
      version: 1.0.0
    source:
      - url: https://example.com/output.tar.gz
        sha256: 2222222222222222222222222222222222222222222222222222222222222222

  - package:
      name: pkg-inherit-cache
      version: 1.0.0
    inherit: build-cache
    source:
      - url: https://example.com/cache-output.tar.gz
        sha256: 3333333333333333333333333333333333333333333333333333333333333333
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                assert_eq!(recipes.len(), 2); // Only package outputs

                // First package output: inherits from top-level (prepends top-level sources)
                let pkg1 = &recipes[0];
                assert_eq!(pkg1.package.name.as_normalized(), "pkg-inherit-toplevel");
                assert_eq!(pkg1.source.len(), 2); // top-level + output
                if let crate::stage1::Source::Url(url_src) = &pkg1.source[0] {
                    assert!(url_src.url[0].to_string().contains("top-level.tar.gz"));
                } else {
                    panic!("Expected URL source");
                }
                if let crate::stage1::Source::Url(url_src) = &pkg1.source[1] {
                    assert!(url_src.url[0].to_string().contains("output.tar.gz"));
                } else {
                    panic!("Expected URL source");
                }
                // pkg1 doesn't inherit from cache, so no staging_caches
                assert_eq!(pkg1.staging_caches.len(), 0);

                // Second package output: inherits from cache (NO top-level sources in the output itself)
                let pkg2 = &recipes[1];
                assert_eq!(pkg2.package.name.as_normalized(), "pkg-inherit-cache");
                assert_eq!(pkg2.source.len(), 1); // Only output source (cache already has top-level)
                if let crate::stage1::Source::Url(url_src) = &pkg2.source[0] {
                    assert!(url_src.url[0].to_string().contains("cache-output.tar.gz"));
                } else {
                    panic!("Expected URL source");
                }

                // pkg2 inherits from cache, so it should have the staging cache
                assert_eq!(pkg2.staging_caches.len(), 1);
                // Check that the staging cache has top-level + staging sources
                let staging_sources = &pkg2.staging_caches[0].source;
                assert_eq!(staging_sources.len(), 2); // top-level + staging
                if let crate::stage1::Source::Url(url_src) = &staging_sources[0] {
                    assert!(url_src.url[0].to_string().contains("top-level.tar.gz"));
                } else {
                    panic!("Expected URL source");
                }
                if let crate::stage1::Source::Url(url_src) = &staging_sources[1] {
                    assert!(url_src.url[0].to_string().contains("staging.tar.gz"));
                } else {
                    panic!("Expected URL source");
                }
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_tests_inheritance() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

tests:
  - script:
      - echo "Top-level test"

outputs:
  - package:
      name: output1
      version: 1.0.0
    tests:
      - script:
          - echo "Output test"

  - package:
      name: output2
      version: 1.0.0
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                assert_eq!(recipes.len(), 2);

                // First output: should have both top-level and output tests
                let output1 = &recipes[0];
                assert_eq!(output1.package.name.as_normalized(), "output1");
                assert_eq!(output1.tests.len(), 2); // top-level + output
                // First test is from top-level
                if let crate::stage1::TestType::Commands(cmd_test) = &output1.tests[0] {
                    // Script could be in various forms
                    match &cmd_test.script.content {
                        rattler_build_script::ScriptContent::Commands(commands) => {
                            assert!(commands.join(" ").contains("Top-level test"));
                        }
                        rattler_build_script::ScriptContent::Command(command) => {
                            assert!(command.contains("Top-level test"));
                        }
                        rattler_build_script::ScriptContent::CommandOrPath(cmd) => {
                            assert!(cmd.contains("Top-level test"));
                        }
                        other => panic!("Unexpected script content type: {:?}", other),
                    }
                } else {
                    panic!("Expected script test");
                }
                // Second test is from output
                if let crate::stage1::TestType::Commands(cmd_test) = &output1.tests[1] {
                    match &cmd_test.script.content {
                        rattler_build_script::ScriptContent::Commands(commands) => {
                            assert!(commands.join(" ").contains("Output test"));
                        }
                        rattler_build_script::ScriptContent::Command(command) => {
                            assert!(command.contains("Output test"));
                        }
                        rattler_build_script::ScriptContent::CommandOrPath(cmd) => {
                            assert!(cmd.contains("Output test"));
                        }
                        other => panic!("Unexpected script content type: {:?}", other),
                    }
                } else {
                    panic!("Expected script test");
                }

                // Second output: should have only top-level test
                let output2 = &recipes[1];
                assert_eq!(output2.package.name.as_normalized(), "output2");
                assert_eq!(output2.tests.len(), 1); // Only top-level
                if let crate::stage1::TestType::Commands(cmd_test) = &output2.tests[0] {
                    match &cmd_test.script.content {
                        rattler_build_script::ScriptContent::Commands(commands) => {
                            assert!(commands.join(" ").contains("Top-level test"));
                        }
                        rattler_build_script::ScriptContent::Command(command) => {
                            assert!(command.contains("Top-level test"));
                        }
                        rattler_build_script::ScriptContent::CommandOrPath(cmd) => {
                            assert!(cmd.contains("Top-level test"));
                        }
                        other => panic!("Unexpected script content type: {:?}", other),
                    }
                } else {
                    panic!("Expected script test");
                }
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_version_inheritance() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

outputs:
  - package:
      name: output-inherits-version
      # No version specified, should inherit from recipe

  - package:
      name: output-overrides-version
      version: 2.0.0
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                assert_eq!(recipes.len(), 2);

                // First output: inherits version from recipe
                let output1 = &recipes[0];
                assert_eq!(
                    output1.package.name.as_normalized(),
                    "output-inherits-version"
                );
                assert_eq!(output1.package.version().to_string(), "1.0.0");

                // Second output: uses its own version
                let output2 = &recipes[1];
                assert_eq!(
                    output2.package.name.as_normalized(),
                    "output-overrides-version"
                );
                assert_eq!(output2.package.version().to_string(), "2.0.0");
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_multi_output_about_inheritance() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

about:
  homepage: https://example.com
  license: MIT
  summary: Top-level summary

outputs:
  - package:
      name: output-full-inherit
      version: 1.0.0

  - package:
      name: output-partial-override
      version: 1.0.0
    about:
      summary: Custom output summary
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                assert_eq!(recipes.len(), 2);

                // First output: inherits all about fields
                let output1 = &recipes[0];
                // URL parser adds a trailing slash
                assert!(
                    output1
                        .about
                        .homepage
                        .as_ref()
                        .unwrap()
                        .as_str()
                        .starts_with("https://example.com")
                );
                assert_eq!(output1.about.summary.as_ref().unwrap(), "Top-level summary");
                assert!(output1.about.license.is_some());

                // Second output: overrides summary but inherits homepage and license
                let output2 = &recipes[1];
                assert!(
                    output2
                        .about
                        .homepage
                        .as_ref()
                        .unwrap()
                        .as_str()
                        .starts_with("https://example.com")
                ); // Inherited
                assert_eq!(
                    output2.about.summary.as_ref().unwrap(),
                    "Custom output summary"
                ); // Overridden
                assert!(output2.about.license.is_some()); // Inherited
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_staging_cache_inheritance_about_and_build() {
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: myproject
  version: 1.0.0

about:
  homepage: https://example.com
  license: Apache-2.0
  license_file: LICENSE
  repository: https://github.com/example/repo

build:
  script: echo "Top-level script"
  dynamic_linking:
    rpaths:
      - lib/
      - custom/

outputs:
  - staging:
      name: compile-stage
    requirements:
      build:
        - cmake
    build:
      script: echo "Building cache"

  - package:
      name: cache-output
      version: 1.0.0
    inherit: compile-stage
    build:
      files:
        - lib/**
    about:
      summary: Cache-inherited output

  - package:
      name: toplevel-output
      version: 1.0.0
    inherit: null
    build:
      files:
        - share/**
    about:
      summary: Top-level inherited output
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

                let recipes = multi.evaluate(&ctx).unwrap();
                assert_eq!(recipes.len(), 2); // Only package outputs, not staging

                // First output: inherits from cache
                let cache_output = &recipes[0];
                assert_eq!(cache_output.package.name.as_normalized(), "cache-output");

                // About section: should merge cache output's about with top-level
                assert_eq!(
                    cache_output.about.summary.as_ref().unwrap(),
                    "Cache-inherited output"
                ); // From output
                assert!(
                    cache_output
                        .about
                        .homepage
                        .as_ref()
                        .unwrap()
                        .as_str()
                        .starts_with("https://example.com")
                ); // Inherited from top-level
                assert!(cache_output.about.license.is_some()); // Inherited from top-level
                assert!(
                    cache_output
                        .about
                        .repository
                        .as_ref()
                        .unwrap()
                        .as_str()
                        .starts_with("https://github.com/example/repo")
                ); // Inherited from top-level

                // Build section: should inherit dynamic_linking but NOT script
                assert!(!cache_output.build.dynamic_linking.is_default()); // Inherited from top-level
                assert!(!cache_output.build.dynamic_linking.rpaths.is_empty()); // Inherited from top-level
                assert!(cache_output.build.script.is_default()); // NOT inherited (cache has its own script)

                // Second output: inherits from top-level
                let toplevel_output = &recipes[1];
                assert_eq!(
                    toplevel_output.package.name.as_normalized(),
                    "toplevel-output"
                );

                // About section: should merge
                assert_eq!(
                    toplevel_output.about.summary.as_ref().unwrap(),
                    "Top-level inherited output"
                ); // From output
                assert!(toplevel_output.about.license.is_some()); // Inherited from top-level

                // Build section: should inherit everything including script
                assert!(!toplevel_output.build.dynamic_linking.is_default()); // Inherited
                assert!(!toplevel_output.build.script.is_default()); // Inherited (top-level has script)
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_build_platform_tracked_from_conditional() {
        use crate::stage0::parser::parse_recipe_from_source;

        // Recipe with build_platform in a conditional if expression in requirements
        let recipe_yaml = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
requirements:
  build:
    - gcc
    - if: build_platform != target_platform
      then:
        - cross-python_linux-64
        - numpy
"#;

        let parsed = parse_recipe_from_source(recipe_yaml).unwrap();

        // Create context with both build_platform and target_platform
        let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

        // Evaluate the recipe
        let result = parsed.evaluate(&ctx);
        assert!(result.is_ok(), "Recipe evaluation should succeed");

        // Check that build_platform was tracked as accessed
        let accessed = ctx.accessed_variables();
        assert!(
            accessed.contains("build_platform"),
            "build_platform should be tracked as accessed during evaluation of 'if: build_platform != target_platform'. Accessed vars: {:?}",
            accessed
        );
        assert!(
            accessed.contains("target_platform"),
            "target_platform should be tracked as accessed during evaluation of 'if: build_platform != target_platform'. Accessed vars: {:?}",
            accessed
        );
    }

    #[test]
    fn test_skip_variable_tracking_plain_string() {
        use crate::stage0::parser::parse_recipe_from_source;

        // Recipe with skip conditions as plain strings (not templates)
        let recipe_yaml = r#"
schema_version: 1
package:
  name: test
  version: 1.0.0
build:
  skip:
    - not (match(python, python_min ~ ".*") and is_abi3)
"#;

        let parsed = parse_recipe_from_source(recipe_yaml).unwrap();

        // Create context with all required variables
        // Using python=3.8 and python_min=3.8 so that match() succeeds and is_abi3 is evaluated
        let mut ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);
        ctx.insert("python".to_string(), Variable::from_string("3.8"));
        ctx.insert("python_min".to_string(), Variable::from_string("3.8"));
        ctx.insert("is_abi3".to_string(), Variable::from(true));

        // Evaluate the recipe
        let result = parsed.evaluate(&ctx);
        assert!(result.is_ok(), "Recipe evaluation should succeed");

        // Check that skip variables were tracked
        // Note: Only variables that are actually accessed during evaluation are tracked.
        // If short-circuit evaluation skips a variable, it won't be in accessed_vars.
        let accessed = ctx.accessed_variables();
        assert!(
            accessed.contains("python"),
            "python should be tracked from skip expression. Accessed vars: {:?}",
            accessed
        );
        assert!(
            accessed.contains("python_min"),
            "python_min should be tracked from skip expression. Accessed vars: {:?}",
            accessed
        );
        assert!(
            accessed.contains("is_abi3"),
            "is_abi3 should be tracked from skip expression (since match() succeeds, the 'and' evaluates the second operand). Accessed vars: {:?}",
            accessed
        );
    }

    #[test]
    fn test_evaluate_extra_with_templates() {
        use serde_value::Value as SerdeValue;

        // Create a Stage0Extra with template values
        let mut extra_map = indexmap::IndexMap::new();
        extra_map.insert(
            "feedstock-name".to_string(),
            SerdeValue::String("${{ repo_name }}".to_string()),
        );
        extra_map.insert(
            "custom-field".to_string(),
            SerdeValue::String("custom-value".to_string()),
        );
        extra_map.insert(
            "version-str".to_string(),
            SerdeValue::String("Version: ${{ version }}".to_string()),
        );

        let extra = Stage0Extra { extra: extra_map };

        // Create evaluation context with variables
        let mut ctx = EvaluationContext::new();
        ctx.insert("repo_name".to_string(), Variable::from_string("my-repo"));
        ctx.insert("version".to_string(), Variable::from_string("1.2.3"));

        // Evaluate the extra
        let result = extra.evaluate(&ctx).unwrap();

        // Check that templates were rendered
        assert_eq!(
            result.extra.get("feedstock-name"),
            Some(&SerdeValue::String("my-repo".to_string()))
        );
        assert_eq!(
            result.extra.get("custom-field"),
            Some(&SerdeValue::String("custom-value".to_string()))
        );
        assert_eq!(
            result.extra.get("version-str"),
            Some(&SerdeValue::String("Version: 1.2.3".to_string()))
        );

        // Check that variables were tracked
        let accessed = ctx.accessed_variables();
        assert!(
            accessed.contains("repo_name"),
            "repo_name should be tracked from extra template"
        );
        assert!(
            accessed.contains("version"),
            "version should be tracked from extra template"
        );
    }

    #[test]
    fn test_evaluate_extra_with_nested_structures() {
        use serde_value::Value as SerdeValue;
        use std::collections::BTreeMap;

        // Create a Stage0Extra with nested structures
        let mut extra_map = indexmap::IndexMap::new();

        // Add a nested map with templates
        let mut nested_map = BTreeMap::new();
        nested_map.insert(
            SerdeValue::String("key1".to_string()),
            SerdeValue::String("${{ var1 }}".to_string()),
        );
        nested_map.insert(
            SerdeValue::String("key2".to_string()),
            SerdeValue::String("static".to_string()),
        );
        extra_map.insert("nested".to_string(), SerdeValue::Map(nested_map));

        // Add a list with templates
        extra_map.insert(
            "list".to_string(),
            SerdeValue::Seq(vec![
                SerdeValue::String("${{ var2 }}".to_string()),
                SerdeValue::String("fixed".to_string()),
            ]),
        );

        let extra = Stage0Extra { extra: extra_map };

        // Create evaluation context
        let mut ctx = EvaluationContext::new();
        ctx.insert("var1".to_string(), Variable::from_string("value1"));
        ctx.insert("var2".to_string(), Variable::from_string("value2"));

        // Evaluate
        let result = extra.evaluate(&ctx).unwrap();

        // Check nested map
        if let Some(SerdeValue::Map(map)) = result.extra.get("nested") {
            assert_eq!(
                map.get(&SerdeValue::String("key1".to_string())),
                Some(&SerdeValue::String("value1".to_string()))
            );
            assert_eq!(
                map.get(&SerdeValue::String("key2".to_string())),
                Some(&SerdeValue::String("static".to_string()))
            );
        } else {
            panic!("nested should be a map");
        }

        // Check list
        if let Some(SerdeValue::Seq(seq)) = result.extra.get("list") {
            assert_eq!(seq[0], SerdeValue::String("value2".to_string()));
            assert_eq!(seq[1], SerdeValue::String("fixed".to_string()));
        } else {
            panic!("list should be a sequence");
        }

        // Check variables were tracked
        let accessed = ctx.accessed_variables();
        assert!(accessed.contains("var1"));
        assert!(accessed.contains("var2"));
    }

    #[test]
    fn test_evaluate_glob_patterns_filters_empty_strings() {
        // Test that glob patterns with conditional expressions that evaluate to empty strings
        // are filtered out. This handles cases like `${{ "loguru" if unix }}` on non-unix platforms.

        // Create list with some normal patterns and one that will evaluate to empty
        let list = ConditionalList::new(vec![
            Item::Value(Value::new_concrete("*.txt".to_string(), None)),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ \"loguru\" if unix else \"\" }}".to_string()).unwrap(),
                None,
            )),
            Item::Value(Value::new_concrete("*.md".to_string(), None)),
        ]);

        // Test with unix = false (should filter out empty string)
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(false));

        let result = evaluate_glob_patterns(&list, &ctx).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "*.txt");
        assert_eq!(result[1], "*.md");

        // Test with unix = true (should include "loguru")
        let mut ctx2 = EvaluationContext::new();
        ctx2.insert("unix".to_string(), Variable::from(true));

        let result2 = evaluate_glob_patterns(&list, &ctx2).unwrap();
        assert_eq!(result2.len(), 3);
        assert_eq!(result2[0], "*.txt");
        assert_eq!(result2[1], "loguru");
        assert_eq!(result2[2], "*.md");
    }

    #[test]
    fn test_evaluate_glob_patterns_simple_conditional_template() {
        // Test the simpler form: `${{ "value" if condition }}`
        // which renders to empty string when condition is false

        let list = ConditionalList::new(vec![
            Item::Value(Value::new_concrete("pattern1".to_string(), None)),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ \"Library/bin/logurud.dll\" if win }}".to_string())
                    .unwrap(),
                None,
            )),
        ]);

        // When win is false, the template evaluates to empty and should be filtered
        let mut ctx = EvaluationContext::new();
        ctx.insert("win".to_string(), Variable::from(false));

        let result = evaluate_glob_patterns(&list, &ctx).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "pattern1");

        // When win is true, the pattern should be included
        let mut ctx2 = EvaluationContext::new();
        ctx2.insert("win".to_string(), Variable::from(true));

        let result2 = evaluate_glob_patterns(&list, &ctx2).unwrap();
        assert_eq!(result2.len(), 2);
        assert_eq!(result2[0], "pattern1");
        assert_eq!(result2[1], "Library/bin/logurud.dll");
    }

    #[test]
    fn test_skipped_output_does_not_evaluate_requirements() {
        // Test that when an output is skipped (e.g., skip: win), its requirements
        // are not evaluated. This is important because platform-specific functions
        // like stdlib('c') may fail on certain platforms where they have no default.
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

context:
  name: test-pkg
  version: "1.0.0"

recipe:
  name: ${{ name }}-split
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}-unix
    build:
      skip: win
    requirements:
      build:
        - ${{ stdlib('c') }}
        - ${{ compiler('cxx') }}
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                // Test on Windows - the output should be skipped and not fail
                let jinja_config = JinjaConfig {
                    target_platform: rattler_conda_types::Platform::Win64,
                    build_platform: rattler_conda_types::Platform::Win64,
                    host_platform: rattler_conda_types::Platform::Win64,
                    // Note: no c_stdlib in variant - stdlib('c') would fail on Windows
                    variant: std::collections::BTreeMap::new(),
                    ..Default::default()
                };
                let ctx =
                    EvaluationContext::with_variables_and_config(IndexMap::new(), jinja_config);

                // This should NOT fail even though stdlib('c') has no default on Windows
                // because the output is skipped (skip: win)
                let result = multi.evaluate(&ctx);
                assert!(
                    result.is_ok(),
                    "Skipped output should not fail on requirement evaluation. Error: {:?}",
                    result.err()
                );

                // Skipped outputs are completely filtered out
                let recipes = result.unwrap();
                assert_eq!(
                    recipes.len(),
                    0,
                    "Skipped output should be filtered out, not returned"
                );
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_non_skipped_output_evaluates_requirements() {
        // Test that non-skipped outputs still have their requirements evaluated
        use crate::stage0::parser::parse_recipe_or_multi_from_source;

        let recipe_yaml = r#"
schema_version: 1

context:
  name: test-pkg
  version: "1.0.0"

recipe:
  name: ${{ name }}-split
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}-unix
    build:
      skip: win
    requirements:
      build:
        - ${{ stdlib('c') }}
        - ${{ compiler('cxx') }}
"#;

        let parsed = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::MultiOutput(multi) => {
                // Test on Linux - the output should NOT be skipped and requirements should be evaluated
                let mut variant = std::collections::BTreeMap::new();
                variant.insert("c_stdlib".into(), Variable::from_string("sysroot"));
                variant.insert("c_stdlib_version".into(), Variable::from_string("2.17"));
                variant.insert("cxx_compiler".into(), Variable::from_string("gxx"));
                variant.insert("cxx_compiler_version".into(), Variable::from_string("12"));

                let jinja_config = JinjaConfig {
                    target_platform: rattler_conda_types::Platform::Linux64,
                    build_platform: rattler_conda_types::Platform::Linux64,
                    host_platform: rattler_conda_types::Platform::Linux64,
                    variant,
                    ..Default::default()
                };
                let ctx =
                    EvaluationContext::with_variables_and_config(IndexMap::new(), jinja_config);

                let result = multi.evaluate(&ctx);
                assert!(
                    result.is_ok(),
                    "Non-skipped output should evaluate requirements"
                );

                let recipes = result.unwrap();
                assert_eq!(recipes.len(), 1);

                // The output should NOT be skipped and should have requirements
                let recipe = &recipes[0];
                assert!(
                    !recipe.requirements.build.is_empty(),
                    "Requirements should NOT be empty for non-skipped output"
                );
            }
            _ => panic!("Expected MultiOutputRecipe"),
        }
    }

    #[test]
    fn test_conditional_post_process_evaluation() {
        use crate::stage0::parser::parse_recipe_from_source;

        let recipe_yaml = r#"
schema_version: 1

package:
  name: test-pkg
  version: 1.0.0

build:
  post_process:
    - if: unix
      then:
        - files:
            - "*.txt"
          regex: "unix_pattern"
          replacement: "unix_replacement"
    - if: win
      then:
        - files:
            - "*.txt"
          regex: "win_pattern"
          replacement: "win_replacement"
    - files:
        - "*.md"
      regex: "always"
      replacement: "present"
"#;

        let parsed = parse_recipe_from_source(recipe_yaml).unwrap();

        // Test with unix=true, win=false
        let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

        let result = parsed.evaluate(&ctx).unwrap();

        // Should have 2 post_process entries: the unix one and the unconditional one
        assert_eq!(result.build.post_process.len(), 2);

        // First should be the unix post_process
        assert_eq!(result.build.post_process[0].regex.as_str(), "unix_pattern");

        // Second should be the unconditional one
        assert_eq!(result.build.post_process[1].regex.as_str(), "always");

        // Test with unix=false, win=true
        let ctx2 = EvaluationContext::for_platform(rattler_conda_types::Platform::Win64);

        let result2 = parsed.evaluate(&ctx2).unwrap();

        // Should have 2 post_process entries: the win one and the unconditional one
        assert_eq!(result2.build.post_process.len(), 2);

        // First should be the win post_process
        assert_eq!(result2.build.post_process[0].regex.as_str(), "win_pattern");

        // Second should be the unconditional one
        assert_eq!(result2.build.post_process[1].regex.as_str(), "always");
    }

    #[test]
    fn test_skip_true_unconditionally_skips() {
        // Test that `skip: true` (boolean literal) causes the build to be skipped
        use crate::stage0::parser::parse_recipe_from_source;

        let recipe_yaml = r#"
schema_version: 1
package:
  name: test-skip
  version: "1.0.0"
build:
  number: 0
  skip: true
  script:
    - exit 1
"#;

        let parsed = parse_recipe_from_source(recipe_yaml).unwrap();

        // Create a minimal evaluation context
        let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Linux64);

        // Evaluate the recipe - it should be skipped
        let result = parsed.evaluate(&ctx);
        assert!(
            result.is_ok(),
            "Recipe evaluation should succeed, got error: {:?}",
            result.err()
        );

        let recipe = result.unwrap();

        // Skip is eagerly evaluated to true during recipe evaluation
        assert!(
            recipe.build.skip,
            "Recipe with skip: true should be skipped"
        );
    }

    #[test]
    fn test_is_skipped_with_literal_true() {
        // Direct test of is_skipped function with the string "true"
        let ctx = EvaluationContext::new();

        // The string "true" should evaluate to boolean true in Jinja
        let skip_conditions = vec!["true".to_string()];
        assert!(
            is_skipped(&skip_conditions, &ctx),
            "is_skipped should return true for skip condition 'true'"
        );

        // Also test "false" - should NOT skip
        let skip_conditions_false = vec!["false".to_string()];
        assert!(
            !is_skipped(&skip_conditions_false, &ctx),
            "is_skipped should return false for skip condition 'false'"
        );
    }

    #[test]
    fn test_default_filter_with_undefined_variable() {
        // Reproducer for https://github.com/prefix-dev/rattler-build/issues/2232
        // `cxx_compiler_version | default(cxx_compiler | replace("vs", ""))` should succeed
        // even though `cxx_compiler_version` is undefined, because it's handled by `default`.
        let mut ctx = EvaluationContext::new();
        ctx.insert("cxx_compiler".to_string(), Variable::from_string("vs2022"));

        let result = render_template_to_variable(
            r#"${{ cxx_compiler_version | default(cxx_compiler | replace("vs", "")) }}"#,
            &ctx,
            None,
        );

        assert!(result.is_ok(), "Expected Ok but got: {:?}", result.err());
        assert_eq!(result.unwrap().as_ref().to_string(), "2022");
    }

    #[test]
    fn test_default_filter_preserves_undefined_error_for_unguarded_vars() {
        // `foo | default(bar)` should error because `bar` (the fallback) is itself
        // undefined and not guarded by `default`.
        let ctx = EvaluationContext::new();

        let result = render_template_to_variable(r#"${{ foo | default(bar) }}"#, &ctx, None);

        assert!(
            result.is_err(),
            "Expected Err because `bar` is undefined and unguarded"
        );
    }

    #[test]
    fn test_default_filter_nested() {
        // `foo | default(bar | default("fallback"))` should succeed because both
        // `foo` and `bar` are guarded by `default`.
        let ctx = EvaluationContext::new();

        let result = render_template_to_variable(
            r#"${{ foo | default(bar | default("fallback")) }}"#,
            &ctx,
            None,
        );

        assert!(
            result.is_ok(),
            "Expected Ok but got: {}",
            result.err().unwrap()
        );
        assert_eq!(result.unwrap().as_ref().to_string(), "fallback");
    }

    #[test]
    fn test_default_filter_in_template_context() {
        // Test the default filter in a multi-expression template context
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), Variable::from_string("pkg"));

        let result = render_template(
            r#"${{ name }}-${{ version | default("0.0.0") }}"#,
            &ctx,
            None,
        );

        assert!(
            result.is_ok(),
            "Expected Ok but got: {}",
            result.err().unwrap()
        );
        assert_eq!(result.unwrap(), "pkg-0.0.0");
    }

    #[test]
    fn test_default_filter_end_to_end_recipe() {
        // End-to-end test for https://github.com/prefix-dev/rattler-build/issues/2232
        // Simulates a real-world recipe where cxx_compiler_version is not set in
        // the variant but derived from cxx_compiler via the `default` filter.
        let recipe_yaml = r#"schema_version: 1

context:
  cxx_compiler: vs2022
  cxx_compiler_version: ${{ cxx_compiler_version | default(cxx_compiler | replace("vs", "")) }}

package:
  name: test-default-filter
  version: ${{ cxx_compiler_version }}
"#;

        let parsed = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        match parsed {
            stage0::Recipe::SingleOutput(recipe) => {
                let ctx = EvaluationContext::for_platform(rattler_conda_types::Platform::Win64);
                let result = recipe.evaluate(&ctx);

                assert!(
                    result.is_ok(),
                    "Expected Ok but got: {}",
                    result.err().unwrap()
                );
                let evaluated = result.unwrap();

                let context = &evaluated.context;
                assert_eq!(
                    context.get("cxx_compiler_version").map(|v| v.to_string()),
                    Some("2022".to_string()),
                    "cxx_compiler_version should be derived from cxx_compiler via default filter"
                );
                assert_eq!(evaluated.package.version.to_string(), "2022");
            }
            _ => panic!("Expected single recipe"),
        }
    }
}
