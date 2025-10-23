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
    path::PathBuf,
    str::FromStr,
};

use indexmap::IndexMap;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{MatchSpec, NoArchType, PackageName, ParseStrictness, VersionWithSource};
use rattler_digest::{Md5Hash, Sha256Hash};

use crate::{
    ParseError, Span,
    stage0::{
        About as Stage0About, Build as Stage0Build, Extra as Stage0Extra, License,
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
            GitRev as Stage0GitRev, GitSource as Stage0GitSource, PathSource as Stage0PathSource,
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
        About as Stage1About, AllOrGlobVec, Dependency, Evaluate, EvaluationContext,
        Extra as Stage1Extra, GlobVec, Package as Stage1Package, Recipe as Stage1Recipe,
        Requirements as Stage1Requirements, Rpaths,
        build::{
            Build as Stage1Build, DynamicLinking as Stage1DynamicLinking,
            ForceFileType as Stage1ForceFileType, PostProcess as Stage1PostProcess,
            PrefixDetection as Stage1PrefixDetection, PythonBuild as Stage1PythonBuild,
            VariantKeyUsage as Stage1VariantKeyUsage,
        },
        requirements::{
            IgnoreRunExports as Stage1IgnoreRunExports, RunExports as Stage1RunExports,
        },
        source::{
            GitRev as Stage1GitRev, GitSource as Stage1GitSource, GitUrl as Stage1GitUrl,
            PathSource as Stage1PathSource, Source as Stage1Source, UrlSource as Stage1UrlSource,
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
use rattler_build_jinja::{Jinja, Variable};

/// Helper to render a Jinja template to a Variable (preserving type information)
fn render_template_to_variable(
    template: &str,
    context: &EvaluationContext,
    span: Option<&Span>,
) -> Result<Variable, ParseError> {
    // Create a Jinja instance with the configuration from the evaluation context
    let jinja_config = context.jinja_config().clone();
    let mut jinja = Jinja::new(jinja_config);

    // Use with_context to add all variables from the evaluation context
    jinja = jinja.with_context(context.variables());

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

        // Simple expression - use compile_expression for type-preserving evaluation
        let env = jinja.env();
        let compiled_expr = match env.compile_expression(expression) {
            Ok(expr) => expr,
            Err(e) => {
                return Err(ParseError::jinja_error(
                    format!("Failed to compile expression '{}': {}", expression, e),
                    span.cloned().unwrap_or(Span::new_blank()),
                ));
            }
        };

        // Evaluate the expression with the context to get a typed Value
        match compiled_expr.eval(jinja.context()) {
            Ok(val) => val,
            Err(e) => {
                // Transfer the tracked variables from Jinja to EvaluationContext
                for var in jinja.accessed_variables() {
                    context.track_access(&var);
                }
                for var in jinja.undefined_variables() {
                    context.track_undefined(&var);
                }

                // Build error with suggestion based on undefined variables
                let undefined_vars: Vec<String> = jinja.undefined_variables().into_iter().collect();
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
                for var in jinja.accessed_variables() {
                    context.track_access(&var);
                }
                for var in jinja.undefined_variables() {
                    context.track_undefined(&var);
                }

                // Build error with suggestion based on undefined variables
                let undefined_vars: Vec<String> = jinja.undefined_variables().into_iter().collect();
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

        // Parse the string to detect type (for simple values like "true" or "42")
        // This is a fallback - it won't preserve types for complex expressions
        return Ok(parse_rendered_value(&rendered_str));
    };

    // Transfer the tracked variables from Jinja to EvaluationContext
    for var in jinja.accessed_variables() {
        context.track_access(&var);
    }
    for var in jinja.undefined_variables() {
        context.track_undefined(&var);
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
    // Create a Jinja instance with the configuration from the evaluation context
    let jinja_config = context.jinja_config().clone();
    let mut jinja = Jinja::new(jinja_config);

    // Use with_context to add all variables from the evaluation context
    jinja = jinja.with_context(context.variables());

    // The Jinja environment is already configured to use ${{ }} syntax
    // so we can pass the template as-is
    // The Jinja type now tracks accessed and undefined variables automatically
    match jinja.render_str(template) {
        Ok(result) => {
            // Transfer the tracked variables from Jinja to EvaluationContext
            for var in jinja.accessed_variables() {
                context.track_access(&var);
            }
            for var in jinja.undefined_variables() {
                context.track_undefined(&var);
            }
            Ok(result)
        }
        Err(e) => {
            // Transfer the tracked variables from Jinja to EvaluationContext
            for var in jinja.accessed_variables() {
                context.track_access(&var);
            }
            for var in jinja.undefined_variables() {
                context.track_undefined(&var);
            }

            // Build error suggestion based on undefined variables
            let undefined_vars: Vec<String> = jinja.undefined_variables().into_iter().collect();
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
) -> Result<bool, ParseError> {
    // Create a Jinja instance with the configuration from the evaluation context
    let jinja_config = context.jinja_config().clone();
    let mut jinja = Jinja::new(jinja_config);

    // Use with_context to add all variables from the evaluation context
    jinja = jinja.with_context(context.variables());

    // Evaluate the expression to get its value
    let value = jinja.eval(expr.source()).map_err(|e| {
        ParseError::jinja_error(
            format!("Failed to evaluate condition '{}': {}", expr.source(), e),
            Span::new_blank(),
        )
    })?;

    // Transfer the tracked variables from Jinja to EvaluationContext
    for var in jinja.accessed_variables() {
        context.track_access(&var);
    }

    // Check for undefined variables and error out
    let undefined_vars = jinja.undefined_variables();
    for var in &undefined_vars {
        context.track_undefined(var);
    }

    if !undefined_vars.is_empty() {
        return Err(
            ParseError::jinja_error(
                format!(
                    "Undefined variable(s) in condition '{}': {}",
                    expr.source(),
                    undefined_vars.iter().map(|s| format!("'{}'", s)).collect::<Vec<_>>().join(", ")
                ),
                Span::new_blank(),
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
        render_template_to_variable(template.source(), context, value.span())
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
        Some(v) => evaluate_string_value(v, context).map(Some),
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
                    &format!("Failed to parse: {}", e),
                    Span::new_blank(),
                )
            })
        }
    }
}

/// Generic helper to evaluate a ConditionalList<T> by processing each Value<T>
///
/// This abstracts the common pattern of iterating over a conditional list,
/// evaluating conditionals, and processing each value with a closure.
///
/// This helper significantly reduces code duplication across the evaluation functions
/// for different list types (strings, globs, dependencies, etc.).
fn evaluate_conditional_list<T, R, F>(
    list: &ConditionalList<T>,
    context: &EvaluationContext,
    mut process: F,
) -> Result<Vec<R>, ParseError>
where
    T: std::fmt::Debug,
    F: FnMut(&Value<T>, &EvaluationContext) -> Result<Option<R>, ParseError>,
{
    let mut results = Vec::new();

    for item in list.iter() {
        match item {
            Item::Value(value) => {
                if let Some(result) = process(value, context)? {
                    results.push(result);
                }
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;

                let items_to_process = if condition_met {
                    &cond.then
                } else {
                    match &cond.else_value {
                        Some(else_items) => else_items,
                        None => continue,
                    }
                };

                for val in items_to_process.iter() {
                    if let Some(result) = process(val, context)? {
                        results.push(result);
                    }
                }
            }
        }
    }

    Ok(results)
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
    evaluate_conditional_list(list, context, |value, ctx| {
        let s = evaluate_string_value(value, ctx)?;
        // Filter out empty strings from templates like `${{ "value" if condition }}`
        Ok(if s.is_empty() { None } else { Some(s) })
    })
}

/// Helper function to validate and evaluate glob patterns from a ConditionalList
fn evaluate_glob_patterns(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    evaluate_conditional_list(list, context, |value, ctx| {
        let pattern = evaluate_string_value(value, ctx)?;
        // Validate the glob pattern immediately with proper error reporting
        match rattler_build_types::glob::validate_glob_pattern(&pattern) {
            Ok(_) => Ok(Some(pattern)),
            Err(e) => Err(
                ParseError::invalid_value(
                    "glob pattern",
                    &format!("Invalid glob pattern '{}': {}", pattern, e),
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
            &format!("Failed to build glob set: {}", e),
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
    evaluate_conditional_list(list, context, |val, ctx| {
        if let Some(ep) = val.as_concrete() {
            Ok(Some(ep.clone()))
        } else if let Some(template) = val.as_template() {
            let s = render_template(template.source(), ctx, val.span())?;
            s.parse::<rattler_conda_types::package::EntryPoint>()
                .map(Some)
                .map_err(|e| {
                    ParseError::invalid_value(
                        "entry point",
                        &format!("Invalid entry point '{}': {}", s, e),
                        val.span().copied().unwrap_or_else(Span::new_blank),
                    )
                    .with_suggestion(
                        "Entry points should be in the format 'command = module:function'",
                    )
                })
        } else {
            unreachable!("Value must be either concrete or template")
        }
    })
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
    evaluate_conditional_list(list, context, |value, ctx| {
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
        serde_yaml::from_str(s).map_err(|e| {
            ParseError::invalid_value(
                "pin dependency",
                &format!("Failed to parse pin dependency: {}", e),
                span,
            )
        })
    } else {
        // It's a regular MatchSpec string
        let spec = MatchSpec::from_str(s, ParseStrictness::Strict).map_err(|e| {
            ParseError::invalid_value(
                "match spec",
                &format!("Invalid match spec '{}': {}", s, e),
                span,
            )
        })?;
        Ok(Dependency::Spec(Box::new(spec)))
    }
}

/// Evaluate a ConditionalList<ScriptContent> into rattler_build_script::Script
/// Evaluate a stage0::Script into a rattler_build_script::Script
pub fn evaluate_script(
    script: &crate::stage0::types::Script,
    context: &EvaluationContext,
) -> Result<rattler_build_script::Script, ParseError> {
    use rattler_build_script::{Script, ScriptContent as ScriptContentOutput};

    // If the script is default/empty, return default script
    if script.is_default() {
        return Ok(Script::default());
    }

    // Evaluate interpreter
    let interpreter = if let Some(interp) = &script.interpreter {
        Some(evaluate_string_value(interp, context)?)
    } else {
        None
    };

    // Evaluate environment variables
    // Filter out keys whose values evaluate to empty strings
    let mut env = indexmap::IndexMap::new();
    for (key, val) in &script.env {
        let evaluated_val = evaluate_string_value(val, context)?;
        if !evaluated_val.is_empty() {
            env.insert(key.clone(), evaluated_val);
        }
    }

    // Copy secrets as-is
    let secrets = script.secrets.clone();

    // Evaluate cwd
    let cwd = if let Some(cwd_val) = &script.cwd {
        Some(PathBuf::from(evaluate_string_value(cwd_val, context)?))
    } else {
        None
    };

    // Evaluate content or file
    let content = if let Some(file_val) = &script.file {
        let file_str = evaluate_string_value(file_val, context)?;
        ScriptContentOutput::Path(PathBuf::from(file_str))
    } else if let Some(content_list) = &script.content {
        let commands = evaluate_string_list(content_list, context)?;
        if commands.is_empty() {
            ScriptContentOutput::Default
        } else if commands.len() == 1 {
            // Single command - could be a path or command, use CommandOrPath
            ScriptContentOutput::CommandOrPath(commands.into_iter().next().unwrap())
        } else {
            // Multiple commands
            ScriptContentOutput::Commands(commands)
        }
    } else {
        ScriptContentOutput::Default
    };

    Ok(Script {
        interpreter,
        env,
        secrets,
        content,
        cwd,
    })
}

/// Parse a boolean from a string (case-insensitive)
fn parse_bool_from_str(s: &str, field_name: &str) -> Result<bool, ParseError> {
    match s.to_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ParseError::invalid_value(
            field_name,
            &format!(
                "Invalid boolean value for '{}': '{}' (expected true/false)",
                field_name, s
            ),
            Span::new_blank(),
        )),
    }
}

/// Evaluate a Value<bool> into a bool
pub fn evaluate_bool_value(
    value: &Value<bool>,
    context: &EvaluationContext,
    field_name: &str,
) -> Result<bool, ParseError> {
    if let Some(b) = value.as_concrete() {
        Ok(*b)
    } else if let Some(template) = value.as_template() {
        let s = render_template(template.source(), context, value.span())?;
        parse_bool_from_str(&s, field_name)
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
                &format!("Failed to parse {}: {}", type_name, e),
                Span::new_blank(),
            )
        })?)
    } else if let Some(template) = value.as_template() {
        let s = render_template(template.source(), context, value.span())?;
        s.parse().map_err(|e| {
            ParseError::invalid_value(
                type_name,
                &format!("Invalid {} '{}': {}", type_name, s, e),
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
                    &format!("Invalid URL '{}': {}", s, e),
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
                    &format!("Invalid SPDX license expression: {}", e),
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
                &format!("Invalid SHA256 checksum: {}", s),
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
                &format!("Invalid MD5 checksum: {}", s),
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
                &format!(
                    "invalid value for name: '{}' is not a valid package name: {}",
                    name_str, e
                ),
                Span::new_blank(),
            )
        })?;

        let version = VersionWithSource::from_str(&version_str).map_err(|e| {
            ParseError::invalid_value(
                "version",
                &format!(
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
            license_file: evaluate_glob_vec_simple(&self.license_file, context)?,
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

impl_evaluate_list_fields!(Stage0IgnoreRunExports => Stage1IgnoreRunExports {
    by_name,
    from_package,
});

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

// Use macro for simple list field evaluation
impl_evaluate_list_fields!(Stage0Extra => Stage1Extra { recipe_maintainers });

impl Evaluate for Stage0PythonBuild {
    type Output = Stage1PythonBuild;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let skip_pyc_compilation = evaluate_glob_vec_simple(&self.skip_pyc_compilation, context)?;

        Ok(Stage1PythonBuild {
            entry_points: evaluate_entry_point_list(&self.entry_points, context)?,
            skip_pyc_compilation,
            use_python_app_entrypoint: self.use_python_app_entrypoint,
            version_independent: self.version_independent,
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
                            &format!("Invalid integer value for down_prioritize_variant: '{}'", s),
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
                                &format!(
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

        Ok(Stage1PrefixDetection {
            force_file_type: self.force_file_type.evaluate(context)?,
            ignore,
            ignore_binary_files: self.ignore_binary_files,
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
                &format!("Invalid regular expression: {}", e),
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
                        _ => {
                            return Err(ParseError::invalid_value(
                                "binary_relocation",
                                &format!("Invalid boolean value for binary_relocation: '{}'", s),
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
                            &format!(
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
                            &format!(
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
        let string = self.string.as_ref().map(|s| {
            let template = extract_template_source(s).unwrap();
            crate::stage1::build::BuildString::unresolved(template)
        });

        // Evaluate script
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
                                &format!(
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

        // Evaluate skip conditions
        let skip = evaluate_string_list(&self.skip, context)?;

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

        // Evaluate post_process
        let mut post_process = Vec::new();
        for pp in &self.post_process {
            post_process.push(pp.evaluate(context)?);
        }

        // Evaluate build number
        let number = if let Some(n) = self.number.as_concrete() {
            *n
        } else if let Some(template) = self.number.as_template() {
            let s = render_template(template.source(), context, self.number.span())?;
            // If we render to an empty string, treat it as 0
            if s.is_empty() {
                0
            } else {
                s.parse::<u64>().map_err(|_| {
                    ParseError::invalid_value(
                        "build number",
                        &format!(
                            "Invalid build number: '{}' is not a valid positive integer",
                            s
                        ),
                        self.number.span().copied().unwrap_or_else(Span::new_blank),
                    )
                })?
            }
        } else {
            unreachable!("Value must be either concrete or template")
        };

        Ok(Stage1Build {
            number,
            string,
            script,
            noarch,
            python,
            skip,
            always_copy_files,
            always_include_files,
            merge_build_and_host_envs: self.merge_build_and_host_envs,
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
                            &format!("Invalid git revision: {}", e),
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
            lfs: self
                .lfs
                .as_ref()
                .map(|v| evaluate_bool_value(v, context, "lfs"))
                .transpose()?
                .unwrap_or(false),
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
                    &format!("Invalid URL '{}': {}", url_str, e),
                    Span::new_blank(),
                )
            })?;
            urls.push(url);
        }

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
                    &format!(
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
                let mut evaluated = Vec::new();
                for v in versions {
                    let version_str = evaluate_string_value(v, context)?;
                    validate_version(&version_str, v.span())?;
                    evaluated.push(version_str);
                }
                Ok(Stage1PythonVersion::Multiple(evaluated))
            }
        }
    }
}

impl Evaluate for Stage0PythonTest {
    type Output = Stage1PythonTest;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let imports = evaluate_string_list(&self.imports, context)?;

        let pip_check = match &self.pip_check {
            None => true, // default to true
            Some(v) => evaluate_bool_value(v, context, "pip_check")?,
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
impl_evaluate_list_fields!(Stage0CommandsTestRequirements => Stage1CommandsTestRequirements { run, build });

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

impl Evaluate for Stage0TestType {
    type Output = Stage1TestType;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        match self {
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
}

impl Evaluate for Stage0Recipe {
    type Output = Stage1Recipe;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // First, evaluate the context variables and merge them into a new context
        let (context_with_vars, evaluated_context) = if !self.context.is_empty() {
            context.with_context(&self.context)?
        } else {
            (context.clone(), IndexMap::new())
        };

        let package = self.package.evaluate(&context_with_vars)?;
        let mut build = self.build.evaluate(&context_with_vars)?;
        let about = self.about.evaluate(&context_with_vars)?;
        let requirements = self.requirements.evaluate(&context_with_vars)?;
        let extra = self.extra.evaluate(&context_with_vars)?;

        // Evaluate source list
        let mut source = Vec::new();
        for src in &self.source {
            source.push(src.evaluate(&context_with_vars)?);
        }

        // Evaluate tests list
        let mut tests = Vec::new();
        for test in &self.tests {
            tests.push(test.evaluate(&context_with_vars)?);
        }

        // Now that evaluation is complete, we know which variables were actually accessed.
        // Compute the hash from the actual variant (accessed variables) and render the build string.
        // Variables that should ALWAYS be included in the hash, even if not accessed
        const ALWAYS_INCLUDE: &[&str] = &["target_platform", "channel_targets", "channel_sources"];

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

        let mut actual_variant: BTreeMap<NormalizedKey, Variable> = context_with_vars
            .variables()
            .iter()
            .filter(|(k, _)| {
                let key_str = k.as_str();

                // Context variables (defined in the context: section) should not be included
                // in the variant for hash computation. We need to determine which variables
                // are context variables vs variant variables.
                // Exclude context variables (from context: section)
                if self.context.contains_key(key_str) {
                    return false;
                }
                // Exclude if it's a noarch-excluded key
                if should_exclude_from_variant(key_str) {
                    return false;
                }

                // Include if accessed, part of our (free) dependencies or if it's an always-include variable
                accessed_vars.contains(key_str)
                    || free_specs.contains(&NormalizedKey::from(key_str))
                    || ALWAYS_INCLUDE.contains(&key_str)
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
            if let crate::stage1::Dependency::Spec(spec) = dep {
                if let Some(ref name) = spec.name {
                    if name.as_normalized().starts_with("__") {
                        actual_variant.insert(
                            NormalizedKey::from(name.as_normalized()),
                            Variable::from(spec.to_string()),
                        );
                    }
                }
            }
        }

        // Compute hash from the actual variant (includes prefix like "py312h...")
        let (prefix, hash) = crate::stage1::compute_hash(&actual_variant, &noarch);

        // If no build string was specified, use the default: {prefix}{hash}_{build_number}
        if build.string.is_none() {
            build.string = Some(crate::stage1::build::BuildString::resolved(format!(
                "{}h{}_{}",
                prefix, hash, build.number
            )));
        } else {
            // For custom build strings, we need to extract just the hash part (without prefix)
            // The template will add its own formatting
            // Extract the part after the last 'h' which is the actual hash
            build.resolve_build_string(&hash, &context_with_vars)?;
        }

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

/// Helper to evaluate a package output into a Stage1Recipe
/// This handles merging top-level recipe sections with output-specific sections
fn evaluate_package_output_to_recipe(
    output: &crate::stage0::PackageOutput,
    recipe: &crate::stage0::MultiOutputRecipe,
    context: &EvaluationContext,
) -> Result<Stage1Recipe, ParseError> {
    use rattler_conda_types::{PackageName, VersionWithSource};
    use std::str::FromStr;

    // Evaluate package name
    let name_str = evaluate_value_to_string(&output.package.name, context)?;
    let name = PackageName::from_str(&name_str).map_err(|e| {
        ParseError::invalid_value(
            "name",
            &format!(
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
            &format!(
                "invalid value for version: '{}' is not a valid version: {}",
                version_str, e
            ),
            Span::new_blank(),
        )
    })?;

    let package = Stage1Package::new(name, version);

    // Evaluate build section (output-specific, no inheritance from top-level currently)
    let mut build = output.build.evaluate(context)?;

    // Evaluate about section (output-specific, or could inherit from top-level)
    let about = output.about.evaluate(context)?;

    // Evaluate requirements
    let requirements = output.requirements.evaluate(context)?;

    // Use recipe-level extra (outputs don't have their own extra)
    let extra = recipe.extra.evaluate(context)?;

    // Evaluate source list (output-specific sources)
    let mut source = Vec::new();
    for src in &output.source {
        source.push(src.evaluate(context)?);
    }

    // Evaluate tests list
    let mut tests = Vec::new();
    for test in &output.tests {
        tests.push(test.evaluate(context)?);
    }

    // Extract the resolved context variables
    let resolved_context = context.variables().clone();

    // Compute the actual variant for this output (subset of accessed variables)
    // This is the same logic as SingleOutputRecipe
    const ALWAYS_INCLUDE: &[&str] = &["target_platform", "channel_targets", "channel_sources"];

    let accessed_vars = context.accessed_variables();
    let free_specs = requirements
        .free_specs()
        .into_iter()
        .map(NormalizedKey::from)
        .collect::<HashSet<_>>();

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

            // Exclude recipe context variables
            if recipe.context.contains_key(key_str) {
                return false;
            }
            // Exclude if it's a noarch-excluded key
            if should_exclude_from_variant(key_str) {
                return false;
            }

            // Include if accessed, part of our (free) dependencies or if it's an always-include variable
            accessed_vars.contains(key_str)
                || free_specs.contains(&NormalizedKey::from(key_str))
                || ALWAYS_INCLUDE.contains(&key_str)
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
        if let crate::stage1::Dependency::Spec(spec) = dep {
            if let Some(ref name) = spec.name {
                if name.as_normalized().starts_with("__") {
                    actual_variant.insert(
                        NormalizedKey::from(name.as_normalized()),
                        Variable::from(spec.to_string()),
                    );
                }
            }
        }
    }

    // Compute hash from the actual variant
    let (prefix, hash) = crate::stage1::compute_hash(&actual_variant, &noarch);

    // Resolve build string with hash
    if build.string.is_none() {
        build.string = Some(crate::stage1::build::BuildString::resolved(format!(
            "{}h{}_{}",
            prefix, hash, build.number
        )));
    } else {
        build.resolve_build_string(&hash, context)?;
    }

    Ok(Stage1Recipe::new(
        package,
        build,
        about,
        requirements,
        extra,
        source,
        tests,
        resolved_context,
        actual_variant,
    ))
}

/// Helper to evaluate a staging output into a Stage1Recipe
/// Staging outputs are simpler - they don't produce packages but cache build results
fn evaluate_staging_output_to_recipe(
    output: &crate::stage0::StagingOutput,
    recipe: &crate::stage0::MultiOutputRecipe,
    context: &EvaluationContext,
) -> Result<Stage1Recipe, ParseError> {
    use rattler_conda_types::{PackageName, VersionWithSource};
    use std::str::FromStr;

    // For staging outputs, we create a special package name based on the staging name
    let staging_name_str = evaluate_string_value(&output.staging.name, context)?;
    let name = PackageName::from_str(&format!("_staging_{}", staging_name_str)).map_err(|e| {
        ParseError::invalid_value(
            "staging name",
            &format!(
                "invalid staging name: '{}' cannot be converted to package name: {}",
                staging_name_str, e
            ),
            Span::new_blank(),
        )
    })?;

    // Use recipe-level version or a default
    let version = if let Some(ref version_value) = recipe.recipe.version {
        let version_str = evaluate_value_to_string(version_value, context)?;
        VersionWithSource::from_str(&version_str).map_err(|e| {
            ParseError::invalid_value(
                "version",
                &format!("invalid version '{}': {}", version_str, e),
                Span::new_blank(),
            )
        })?
    } else {
        VersionWithSource::from_str("0.0.0").unwrap()
    };

    let package = Stage1Package::new(name, version);

    // Evaluate staging build (only has script field)
    let script = evaluate_script(&output.build.script, context)?;
    let build = Stage1Build {
        script,
        ..Stage1Build::default()
    };

    // Staging outputs don't have about/tests
    let about = Stage1About::default();
    let extra = recipe.extra.evaluate(context)?;

    // Evaluate requirements (only build/host/ignore_run_exports allowed for staging)
    let requirements = output.requirements.evaluate(context)?;

    // Evaluate source
    let mut source = Vec::new();
    for src in &output.source {
        source.push(src.evaluate(context)?);
    }

    let tests = Vec::new(); // No tests for staging outputs

    let resolved_context = context.variables().clone();

    // Staging outputs still need a variant for caching purposes
    // Use similar logic to package outputs
    const ALWAYS_INCLUDE: &[&str] = &["target_platform", "channel_targets", "channel_sources"];

    let accessed_vars = context.accessed_variables();
    let free_specs = requirements
        .free_specs()
        .into_iter()
        .map(NormalizedKey::from)
        .collect::<HashSet<_>>();

    let actual_variant: BTreeMap<NormalizedKey, Variable> = context
        .variables()
        .iter()
        .filter(|(k, _)| {
            let key_str = k.as_str();
            if recipe.context.contains_key(key_str) {
                return false;
            }
            accessed_vars.contains(key_str)
                || free_specs.contains(&NormalizedKey::from(key_str))
                || ALWAYS_INCLUDE.contains(&key_str)
        })
        .map(|(k, v)| (NormalizedKey::from(k.as_str()), v.clone()))
        .collect();

    Ok(Stage1Recipe::new(
        package,
        build,
        about,
        requirements,
        extra,
        source,
        tests,
        resolved_context,
        actual_variant,
    ))
}

/// Implement Evaluate for MultiOutputRecipe
/// Returns a Vec of Stage1Recipe, one for each output
impl Evaluate for crate::stage0::MultiOutputRecipe {
    type Output = Vec<Stage1Recipe>;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // First, evaluate the context variables and merge them into a new context
        let (context_with_vars, evaluated_context) = if !self.context.is_empty() {
            context.with_context(&self.context)?
        } else {
            (context.clone(), IndexMap::new())
        };

        let mut evaluated_outputs = Vec::with_capacity(self.outputs.len());

        // Evaluate each output
        for output in &self.outputs {
            // Clear accessed variables for each output to track them independently
            context_with_vars.clear_accessed();

            match output {
                crate::stage0::Output::Package(pkg_output) => {
                    let mut recipe = evaluate_package_output_to_recipe(
                        pkg_output.as_ref(),
                        self,
                        &context_with_vars,
                    )?;
                    recipe.context = evaluated_context.clone();
                    evaluated_outputs.push(recipe);
                }
                crate::stage0::Output::Staging(staging_output) => {
                    let mut recipe = evaluate_staging_output_to_recipe(
                        staging_output.as_ref(),
                        self,
                        &context_with_vars,
                    )?;
                    recipe.context = evaluated_context.clone();
                    evaluated_outputs.push(recipe);
                }
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
    use crate::stage0::types::{
        Conditional, ConditionalList, Item, JinjaTemplate, ListOrItem, Value,
    };

    #[test]
    fn test_evaluate_condition_simple() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(true));

        let expr = JinjaExpression::new("unix".to_string()).unwrap();
        assert!(evaluate_condition(&expr, &ctx).unwrap());

        let expr2 = JinjaExpression::new("win".to_string()).unwrap();
        assert!(!evaluate_condition(&expr2, &ctx).unwrap());
    }

    #[test]
    fn test_evaluate_condition_not() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(true));

        let expr = JinjaExpression::new("not unix".to_string()).unwrap();
        assert!(!evaluate_condition(&expr, &ctx).unwrap());

        let expr2 = JinjaExpression::new("not win".to_string()).unwrap();
        assert!(evaluate_condition(&expr2, &ctx).unwrap());
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
                then: ListOrItem::new(vec![Value::new_concrete("gcc".to_string(), None)]),
                else_value: Some(ListOrItem::new(vec![Value::new_concrete(
                    "msvc".to_string(),
                    None,
                )])),
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
            then: ListOrItem::new(vec![Value::new_concrete("gcc".to_string(), None)]),
            else_value: Some(ListOrItem::new(vec![Value::new_concrete(
                "msvc".to_string(),
                None,
            )])),
        })]);

        // Evaluate the list - only unix branch is taken
        let _result = evaluate_string_list(&list, &ctx).unwrap();

        // unix should be checked in the condition, but we don't track that yet
        // The concrete values don't trigger template rendering, so no template variables tracked
        let accessed = ctx.accessed_variables();
        // Since the then/else branches contain concrete strings (not templates),
        // no variables are accessed during evaluation
        assert_eq!(accessed.len(), 0);
    }

    #[test]
    fn test_variable_tracking_with_template() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("compiler".to_string(), Variable::from_string("gcc"));
        ctx.insert("version".to_string(), Variable::from_string("1.0.0"));

        // Create a list with templates that will be evaluated
        let list = ConditionalList::new(vec![
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ compiler }}".to_string()).unwrap(),
                None,
            )),
            Item::Value(Value::new_concrete("static-dep".to_string(), None)),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ version }}".to_string()).unwrap(),
                None,
            )),
        ]);

        let _result = evaluate_string_list(&list, &ctx).unwrap();

        // Both "compiler" and "version" should be accessed during template rendering
        let accessed = ctx.accessed_variables();
        assert_eq!(accessed.len(), 2);
        assert!(accessed.contains("compiler"));
        assert!(accessed.contains("version"));
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
        let mut ctx = EvaluationContext::new();
        ctx.set_jinja_config(jinja_config);
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
}
