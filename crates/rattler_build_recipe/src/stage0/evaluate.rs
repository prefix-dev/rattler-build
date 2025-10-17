//! Evaluation of stage0 types into stage1 types
//!
//! This module implements the `Evaluate` trait for stage0 types,
//! converting them into their stage1 equivalents by:
//! - Rendering Jinja templates
//! - Flattening conditionals based on the evaluation context
//! - Validating the results

use std::{path::PathBuf, str::FromStr};

use rattler_conda_types::{MatchSpec, NoArchType, PackageName, ParseStrictness, VersionWithSource};

use crate::{
    ErrorKind, ParseError, Span,
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
        types::{ConditionalList, Item, JinjaExpression, ScriptContent, Value},
    },
    stage1::{
        About as Stage1About, AllOrGlobVec, Dependency, Evaluate, EvaluationContext,
        Extra as Stage1Extra, GlobVec, Package as Stage1Package, Recipe as Stage1Recipe,
        Requirements as Stage1Requirements,
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
use rattler_build_jinja::Jinja;

/// Helper to render a Jinja template with the evaluation context
fn render_template(
    template: &str,
    context: &EvaluationContext,
    span: &Span,
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
            let suggestion = if !undefined_vars.is_empty() {
                if undefined_vars.len() == 1 {
                    Some(format!(
                        "The variable '{}' is not defined in the evaluation context. \
                         Make sure it is provided or defined in the context section.",
                        undefined_vars[0]
                    ))
                } else {
                    Some(format!(
                        "The variables {} are not defined in the evaluation context. \
                         Make sure they are provided or defined in the context section.",
                        undefined_vars
                            .iter()
                            .map(|s| format!("'{}'", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
            } else {
                None
            };

            Err(ParseError {
                kind: ErrorKind::JinjaError,
                span: *span,
                message: Some(format!(
                    "Template rendering failed: {} (template: {})",
                    e, template
                )),
                suggestion,
            })
        }
    }
}

/// Evaluate a simple conditional expression
fn evaluate_condition(
    expr: &JinjaExpression,
    context: &EvaluationContext,
) -> Result<bool, ParseError> {
    // For now, simple variable existence check
    // This should be expanded to support full boolean expressions
    let expr_str = expr.source().trim();

    // Check for "not" prefix
    if let Some(rest) = expr_str.strip_prefix("not ") {
        return Ok(!context.contains(rest.trim()));
    }

    // Simple variable existence
    Ok(context.contains(expr_str))
}

/// Evaluate a Value<String> into a String
pub fn evaluate_string_value(
    value: &Value<String>,
    context: &EvaluationContext,
) -> Result<String, ParseError> {
    match value {
        Value::Concrete { value: s, .. } => Ok(s.clone()),
        Value::Template { template, span } => render_template(template.source(), context, span),
    }
}

/// Evaluate a Value<T: ToString> into a String
pub fn evaluate_value_to_string<T: ToString>(
    value: &Value<T>,
    context: &EvaluationContext,
) -> Result<String, ParseError> {
    match value {
        Value::Concrete { value: v, .. } => Ok(v.to_string()),
        Value::Template { template, span } => render_template(template.source(), context, span),
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
            T::from_str(&s).map(Some).map_err(|e| ParseError {
                kind: ErrorKind::InvalidValue,
                span: Span::unknown(),
                message: Some(format!("Failed to parse value: {}", e)),
                suggestion: None,
            })
        }
    }
}

/// Evaluate a ConditionalList<String> into Vec<String>
pub fn evaluate_string_list(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<Vec<String>, ParseError> {
    let mut results = Vec::new();

    for item in list.iter() {
        match item {
            Item::Value(value) => {
                let s = evaluate_string_value(value, context)?;
                results.push(s);
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;

                if condition_met {
                    // Evaluate the "then" items
                    for val in cond.then.iter() {
                        let s = evaluate_string_value(val, context)?;
                        results.push(s);
                    }
                } else {
                    // Evaluate the "else" items
                    if let Some(else_value) = &cond.else_value {
                        for val in else_value.iter() {
                            let s = evaluate_string_value(val, context)?;
                            results.push(s);
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Evaluate a ConditionalList<String> into a GlobVec (include-only), preserving span information for error reporting
pub fn evaluate_glob_vec_simple(
    list: &ConditionalList<String>,
    context: &EvaluationContext,
) -> Result<GlobVec, ParseError> {
    let mut globs = Vec::new();

    for item in list.iter() {
        match item {
            Item::Value(value) => {
                let pattern = evaluate_string_value(value, context)?;
                // Validate the glob pattern immediately with proper error reporting
                match rattler_build_types::glob::validate_glob_pattern(&pattern) {
                    Ok(_) => globs.push(pattern),
                    Err(e) => {
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: value.span(),
                            message: Some(format!("Invalid glob pattern '{}': {}", pattern, e)),
                            suggestion: Some("Check your glob pattern syntax. Common issues include unmatched braces or invalid escape sequences.".to_string()),
                        });
                    }
                }
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;
                let items_to_process = if condition_met {
                    Some(&cond.then)
                } else {
                    cond.else_value.as_ref()
                };

                if let Some(items) = items_to_process {
                    for val in items.iter() {
                        let pattern = evaluate_string_value(val, context)?;
                        match rattler_build_types::glob::validate_glob_pattern(&pattern) {
                            Ok(_) => globs.push(pattern),
                            Err(e) => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: val.span(),
                                    message: Some(format!(
                                        "Invalid glob pattern '{}': {}",
                                        pattern, e
                                    )),
                                    suggestion: Some("Check your glob pattern syntax.".to_string()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Create the GlobVec with only include patterns
    GlobVec::from_strings(globs, Vec::new()).map_err(|e| ParseError {
        kind: ErrorKind::InvalidValue,
        span: Span::unknown(),
        message: Some(format!("Failed to build glob set: {}", e)),
        suggestion: None,
    })
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

    // Evaluate and parse include patterns
    let mut include_globs = Vec::new();
    for item in include_list.iter() {
        match item {
            Item::Value(value) => {
                let pattern = evaluate_string_value(value, context)?;
                // Validate the glob pattern immediately with proper error reporting
                match rattler_build_types::glob::validate_glob_pattern(&pattern) {
                    Ok(_) => include_globs.push(pattern),
                    Err(e) => {
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: value.span(),
                            message: Some(format!("Invalid glob pattern '{}': {}", pattern, e)),
                            suggestion: Some("Check your glob pattern syntax. Common issues include unmatched braces or invalid escape sequences.".to_string()),
                        });
                    }
                }
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;
                let items_to_process = if condition_met {
                    Some(&cond.then)
                } else {
                    cond.else_value.as_ref()
                };

                if let Some(items) = items_to_process {
                    for val in items.iter() {
                        let pattern = evaluate_string_value(val, context)?;
                        match rattler_build_types::glob::validate_glob_pattern(&pattern) {
                            Ok(_) => include_globs.push(pattern),
                            Err(e) => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: val.span(),
                                    message: Some(format!(
                                        "Invalid glob pattern '{}': {}",
                                        pattern, e
                                    )),
                                    suggestion: Some("Check your glob pattern syntax.".to_string()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Evaluate and parse exclude patterns
    let mut exclude_globs = Vec::new();
    for item in exclude_list.iter() {
        match item {
            Item::Value(value) => {
                let pattern = evaluate_string_value(value, context)?;
                match rattler_build_types::glob::validate_glob_pattern(&pattern) {
                    Ok(_) => exclude_globs.push(pattern),
                    Err(e) => {
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: value.span(),
                            message: Some(format!("Invalid glob pattern '{}': {}", pattern, e)),
                            suggestion: Some("Check your glob pattern syntax.".to_string()),
                        });
                    }
                }
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;
                let items_to_process = if condition_met {
                    Some(&cond.then)
                } else {
                    cond.else_value.as_ref()
                };

                if let Some(items) = items_to_process {
                    for val in items.iter() {
                        let pattern = evaluate_string_value(val, context)?;
                        match rattler_build_types::glob::validate_glob_pattern(&pattern) {
                            Ok(_) => exclude_globs.push(pattern),
                            Err(e) => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: val.span(),
                                    message: Some(format!(
                                        "Invalid glob pattern '{}': {}",
                                        pattern, e
                                    )),
                                    suggestion: Some("Check your glob pattern syntax.".to_string()),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Now create the GlobVec - this should not fail since we've already validated
    GlobVec::from_strings(include_globs, exclude_globs).map_err(|e| ParseError {
        kind: ErrorKind::InvalidValue,
        span: Span::unknown(),
        message: Some(format!("Failed to build glob set: {}", e)),
        suggestion: None,
    })
}

/// Evaluate a ConditionalList<EntryPoint> into Vec<EntryPoint>
/// Entry points can be concrete values or templates that render to strings
pub fn evaluate_entry_point_list(
    list: &ConditionalList<rattler_conda_types::package::EntryPoint>,
    context: &EvaluationContext,
) -> Result<Vec<rattler_conda_types::package::EntryPoint>, ParseError> {
    let mut results = Vec::new();

    // Helper to evaluate a single Value<EntryPoint>
    let evaluate_entry_point = |val: &Value<rattler_conda_types::package::EntryPoint>| -> Result<rattler_conda_types::package::EntryPoint, ParseError> {
        match val {
            Value::Concrete { value: ep, .. } => Ok(ep.clone()),
            Value::Template { template, span } => {
                let s = render_template(template.source(), context, span)?;
                s.parse::<rattler_conda_types::package::EntryPoint>()
                    .map_err(|e| ParseError {
                        kind: ErrorKind::InvalidValue,
                        span: *span,
                        message: Some(format!("Invalid entry point '{}': {}", s, e)),
                        suggestion: Some("Entry points should be in the format 'command = module:function'".to_string()),
                    })
            }
        }
    };

    for item in list.iter() {
        match item {
            Item::Value(value) => {
                results.push(evaluate_entry_point(value)?);
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;
                let items_to_process = if condition_met {
                    Some(&cond.then)
                } else {
                    cond.else_value.as_ref()
                };

                if let Some(items) = items_to_process {
                    for val in items.iter() {
                        results.push(evaluate_entry_point(val)?);
                    }
                }
            }
        }
    }

    Ok(results)
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
pub fn evaluate_dependency_list(
    list: &crate::stage0::types::ConditionalList<crate::stage0::SerializableMatchSpec>,
    context: &EvaluationContext,
) -> Result<Vec<crate::stage1::Dependency>, ParseError> {
    let mut results = Vec::new();

    // Iterate over the conditional list items directly to preserve span information
    for item in list.iter() {
        match item {
            Item::Value(value) => {
                match value {
                    Value::Concrete {
                        value: match_spec, ..
                    } => {
                        // Concrete MatchSpec - already validated at parse time!
                        results.push(Dependency::Spec(Box::new(match_spec.0.clone())));
                    }
                    Value::Template { template, span } => {
                        // Template - need to render and parse
                        let s = render_template(template.source(), context, span)?;

                        // Check if it's a JSON dictionary (pin_subpackage or pin_compatible)
                        if s.trim().starts_with('{') {
                            // Try to deserialize as Dependency (which handles pin types)
                            let dep: Dependency =
                                serde_yaml::from_str(&s).map_err(|e| ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: *span,
                                    message: Some(format!("Failed to parse pin dependency: {}", e)),
                                    suggestion: None,
                                })?;
                            results.push(dep);
                        } else {
                            // It's a regular MatchSpec string
                            let spec =
                                MatchSpec::from_str(&s, ParseStrictness::Strict).map_err(|e| {
                                    ParseError {
                                        kind: ErrorKind::InvalidValue,
                                        span: *span,
                                        message: Some(format!("Invalid match spec '{}': {}", s, e)),
                                        suggestion: None,
                                    }
                                })?;
                            results.push(Dependency::Spec(Box::new(spec)));
                        }
                    }
                }
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;

                let items_to_process = if condition_met {
                    Some(&cond.then)
                } else {
                    cond.else_value.as_ref()
                };
                if let Some(items_to_process) = items_to_process {
                    for val in items_to_process.iter() {
                        match val {
                            Value::Concrete {
                                value: match_spec, ..
                            } => {
                                results.push(Dependency::Spec(Box::new(match_spec.0.clone())));
                            }
                            Value::Template { template, span } => {
                                // Render the template and parse as matchspec
                                let s = render_template(template.source(), context, span)?;
                                let spec = MatchSpec::from_str(&s, ParseStrictness::Strict)
                                    .map_err(|e| ParseError {
                                        kind: ErrorKind::InvalidValue,
                                        span: *span,
                                        message: Some(format!("Invalid match spec '{}': {}", s, e)),
                                        suggestion: None,
                                    })?;
                                results.push(Dependency::Spec(Box::new(spec)));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Evaluate a ConditionalList<ScriptContent> into rattler_build_script::Script
pub fn evaluate_script_list(
    list: &ConditionalList<ScriptContent>,
    context: &EvaluationContext,
) -> Result<rattler_build_script::Script, ParseError> {
    use rattler_build_script::{Script, ScriptContent as ScriptContentOutput};

    // If the list is empty, return default script
    if list.is_empty() {
        return Ok(Script::default());
    }

    // Collect all script items after evaluating conditionals
    let mut all_commands = Vec::new();
    let mut interpreter: Option<String> = None;
    let mut env = indexmap::IndexMap::new();
    let mut secrets = Vec::new();
    let mut file_path: Option<PathBuf> = None;

    for item in list.iter() {
        match item {
            Item::Value(value) => {
                match value {
                    Value::Concrete {
                        value: script_content,
                        ..
                    } => {
                        match script_content {
                            ScriptContent::Command(cmd) => {
                                all_commands.push(cmd.clone());
                            }
                            ScriptContent::Inline(inline) => {
                                // Extract interpreter if specified
                                if let Some(interp) = &inline.interpreter {
                                    interpreter = Some(evaluate_string_value(interp, context)?);
                                }

                                // Extract environment variables
                                for (key, val) in &inline.env {
                                    let evaluated_val = evaluate_string_value(val, context)?;
                                    env.insert(key.clone(), evaluated_val);
                                }

                                // Extract secrets
                                secrets.extend(inline.secrets.clone());

                                // Extract content or file
                                if let Some(content_list) = &inline.content {
                                    let commands = evaluate_string_list(content_list, context)?;
                                    all_commands.extend(commands);
                                } else if let Some(file_val) = &inline.file {
                                    let file_str = evaluate_string_value(file_val, context)?;
                                    file_path = Some(PathBuf::from(file_str));
                                }
                            }
                        }
                    }
                    Value::Template { .. } => {
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: Span::unknown(),
                            message: Some("Script content cannot be a template".to_string()),
                            suggestion: None,
                        });
                    }
                }
            }
            Item::Conditional(cond) => {
                let condition_met = evaluate_condition(&cond.condition, context)?;

                let items_to_process = if condition_met {
                    &cond.then.0
                } else {
                    cond.else_value
                        .as_ref()
                        .map(|v| v.0.as_slice())
                        .unwrap_or(&[])
                };

                for val in items_to_process {
                    match val {
                        Value::Concrete {
                            value: script_content,
                            ..
                        } => {
                            match script_content {
                                ScriptContent::Command(cmd) => {
                                    all_commands.push(cmd.clone());
                                }
                                ScriptContent::Inline(inline) => {
                                    // Extract interpreter if specified
                                    if let Some(interp) = &inline.interpreter {
                                        interpreter = Some(evaluate_string_value(interp, context)?);
                                    }

                                    // Extract environment variables
                                    for (key, val) in &inline.env {
                                        let evaluated_val = evaluate_string_value(val, context)?;
                                        env.insert(key.clone(), evaluated_val);
                                    }

                                    // Extract secrets
                                    secrets.extend(inline.secrets.clone());

                                    // Extract content or file
                                    if let Some(content_list) = &inline.content {
                                        let commands = evaluate_string_list(content_list, context)?;
                                        all_commands.extend(commands);
                                    } else if let Some(file_val) = &inline.file {
                                        let file_str = evaluate_string_value(file_val, context)?;
                                        file_path = Some(PathBuf::from(file_str));
                                    }
                                }
                            }
                        }
                        Value::Template { .. } => {
                            return Err(ParseError {
                                kind: ErrorKind::InvalidValue,
                                span: Span::unknown(),
                                message: Some("Script content cannot be a template".to_string()),
                                suggestion: None,
                            });
                        }
                    }
                }
            }
        }
    }

    // Build the final Script content
    let content = if let Some(path) = file_path {
        ScriptContentOutput::Path(path)
    } else if all_commands.is_empty() {
        ScriptContentOutput::Default
    } else if all_commands.len() == 1 {
        // Single command - could be a path or command, use CommandOrPath
        ScriptContentOutput::CommandOrPath(all_commands.into_iter().next().unwrap())
    } else {
        // Multiple commands
        ScriptContentOutput::Commands(all_commands)
    };

    Ok(Script {
        interpreter,
        env,
        secrets,
        content,
        cwd: None,
    })
}

/// Parse a boolean from a string (case-insensitive)
fn parse_bool_from_str(s: &str, field_name: &str) -> Result<bool, ParseError> {
    match s.to_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(ParseError {
            kind: ErrorKind::InvalidValue,
            span: Span::unknown(),
            message: Some(format!(
                "Invalid boolean value for '{}': '{}' (expected true/false, yes/no, or 1/0)",
                field_name, s
            )),
            suggestion: None,
        }),
    }
}

/// Evaluate a Value<bool> into a bool
pub fn evaluate_bool_value(
    value: &Value<bool>,
    context: &EvaluationContext,
    field_name: &str,
) -> Result<bool, ParseError> {
    match value {
        Value::Concrete { value: b, .. } => Ok(*b),
        Value::Template { template, span } => {
            let s = render_template(template.source(), context, span)?;
            parse_bool_from_str(&s, field_name)
        }
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
        let name = PackageName::from_str(&name_str).map_err(|e| ParseError {
            kind: ErrorKind::InvalidValue,
            span: Span::unknown(),
            message: Some(format!(
                "invalid value for name: '{}' is not a valid package name: {}",
                name_str, e
            )),
            suggestion: None,
        })?;

        let version = VersionWithSource::from_str(&version_str).map_err(|e| ParseError {
            kind: ErrorKind::InvalidValue,
            span: Span::unknown(),
            message: Some(format!(
                "invalid value for version: '{}' is not a valid version: {}",
                version_str, e
            )),
            suggestion: None,
        })?;

        Ok(Stage1Package::new(name, version))
    }
}

impl Evaluate for Stage0About {
    type Output = Stage1About;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Helper to evaluate a URL field
        let evaluate_url = |field_name: &str,
                            value: &Option<Value<url::Url>>|
         -> Result<Option<url::Url>, ParseError> {
            match value {
                None => Ok(None),
                Some(v) => {
                    let s = evaluate_value_to_string(v, context)?;
                    Some(url::Url::parse(&s).map_err(|e| ParseError {
                        kind: ErrorKind::InvalidValue,
                        span: Span::unknown(),
                        message: Some(format!("Invalid URL for {}: {}", field_name, e)),
                        suggestion: None,
                    }))
                    .transpose()
                }
            }
        };

        // Evaluate URL fields
        let homepage = evaluate_url("homepage", &self.homepage)?;
        let repository = evaluate_url("repository", &self.repository)?;
        let documentation = evaluate_url("documentation", &self.documentation)?;

        // Evaluate license as spdx::Expression (unwrap from License wrapper)
        let license = match &self.license {
            None => None,
            Some(v) => match v {
                Value::Concrete { value: license, .. } => Some(license.clone()),
                Value::Template { template, span } => {
                    let s = render_template(template.source(), context, span)?;
                    Some(s.parse::<License>().map_err(|e| ParseError {
                        kind: ErrorKind::InvalidValue,
                        span: *span,
                        message: Some(format!("Invalid SPDX license expression: {}", e)),
                        suggestion: None,
                    })?)
                }
            },
        };

        Ok(Stage1About {
            homepage,
            repository,
            documentation,
            license,
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
                Some(s.parse::<i32>().map_err(|_| ParseError {
                    kind: ErrorKind::InvalidValue,
                    span: Span::unknown(),
                    message: Some(format!(
                        "Invalid integer value for down_prioritize_variant: '{}'",
                        s
                    )),
                    suggestion: None,
                })?)
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
                let bool_val = match val {
                    Value::Concrete { value: b, .. } => *b,
                    Value::Template { template, span } => {
                        let s = render_template(template.source(), context, span)?;
                        match s.as_str() {
                            "true" | "True" | "yes" | "Yes" => true,
                            "false" | "False" | "no" | "No" => false,
                            _ => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: *span,
                                    message: Some(format!(
                                        "Invalid boolean value for prefix_detection.ignore: '{}'",
                                        s
                                    )),
                                    suggestion: None,
                                });
                            }
                        }
                    }
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
        let regex = regex::Regex::new(&regex_str).map_err(|e| ParseError {
            kind: ErrorKind::InvalidValue,
            span: Span::unknown(),
            message: Some(format!("Invalid regular expression: {}", e)),
            suggestion: Some("Check your regex syntax. Common issues include unescaped special characters or unbalanced brackets.".to_string()),
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
                let bool_val = match val {
                    Value::Concrete { value: b, .. } => *b,
                    Value::Template { template, span } => {
                        let s = render_template(template.source(), context, span)?;
                        match s.as_str() {
                            "true" | "True" | "yes" | "Yes" => true,
                            "false" | "False" | "no" | "No" => false,
                            _ => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: *span,
                                    message: Some(format!(
                                        "Invalid boolean value for binary_relocation: '{}'",
                                        s
                                    )),
                                    suggestion: None,
                                });
                            }
                        }
                    }
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
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: Span::unknown(),
                            message: Some(format!(
                                "Invalid overdepending_behavior '{}'. Expected 'ignore' or 'error'",
                                s
                            )),
                            suggestion: None,
                        });
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
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: Span::unknown(),
                            message: Some(format!(
                                "Invalid overlinking_behavior '{}'. Expected 'ignore' or 'error'",
                                s
                            )),
                            suggestion: None,
                        });
                    }
                }
            }
        };

        Ok(Stage1DynamicLinking {
            rpaths: evaluate_string_list(&self.rpaths, context)?,
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
        // Evaluate build string
        let string = evaluate_optional_string_value(&self.string, context)?;

        // Evaluate script
        let script = evaluate_script_list(&self.script, context)?;

        // Evaluate noarch
        let noarch = match &self.noarch {
            None => None,
            Some(v) => {
                let s = evaluate_value_to_string(v, context)?;
                match s.as_str() {
                    "python" => Some(NoArchType::python()),
                    "generic" => Some(NoArchType::generic()),
                    _ => {
                        return Err(ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: Span::unknown(),
                            message: Some(format!(
                                "Invalid noarch type '{}'. Expected 'python' or 'generic'",
                                s
                            )),
                            suggestion: None,
                        });
                    }
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
        let number = match &self.number {
            Value::Concrete { value: n, .. } => *n,
            Value::Template { template, span } => {
                let s = render_template(template.source(), context, span)?;
                s.parse::<u64>().map_err(|_| ParseError {
                    kind: ErrorKind::InvalidValue,
                    span: *span,
                    message: Some(format!(
                        "Invalid build number: '{}' is not a valid positive integer",
                        s
                    )),
                    suggestion: None,
                })?
            }
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
                    Stage1GitRev::from_str(&rev_str).map_err(|e| ParseError {
                        kind: ErrorKind::InvalidValue,
                        span: Span::unknown(),
                        message: Some(format!("Invalid git revision: {}", e)),
                        suggestion: None,
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

        // Evaluate depth
        let depth = evaluate_optional_value_to_type(&self.depth, context)?;

        // Evaluate patches (flatten conditionals and convert to PathBuf)
        let patches = evaluate_string_list(&self.patches, context)?
            .into_iter()
            .map(PathBuf::from)
            .collect();

        // Evaluate target_directory
        let target_directory = match &self.target_directory {
            None => None,
            Some(v) => match v {
                Value::Concrete { value: p, .. } => Some(p.clone()),
                Value::Template { template, span } => {
                    let s = render_template(template.source(), context, span)?;
                    Some(PathBuf::from(s))
                }
            },
        };

        // Evaluate lfs flag
        let lfs = match &self.lfs {
            None => false,
            Some(v) => evaluate_bool_value(v, context, "lfs")?,
        };

        Ok(Stage1GitSource {
            url,
            rev,
            depth,
            patches,
            target_directory,
            lfs,
        })
    }
}

impl Evaluate for Stage0UrlSource {
    type Output = Stage1UrlSource;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Evaluate URLs and parse into url::Url
        let mut urls = Vec::new();
        for url_value in &self.url {
            let url_str = evaluate_string_value(url_value, context)?;
            let url = url::Url::parse(&url_str).map_err(|e| ParseError {
                kind: ErrorKind::InvalidValue,
                span: Span::unknown(),
                message: Some(format!("Invalid URL '{}': {}", url_str, e)),
                suggestion: None,
            })?;
            urls.push(url);
        }

        // Evaluate checksum fields separately (both can be set)
        // For hash types, we can just extract the concrete value or evaluate templates
        let sha256 = match &self.sha256 {
            None => None,
            Some(Value::Concrete { value, .. }) => Some(*value),
            Some(Value::Template { template, span }) => {
                let sha256_str = render_template(template.source(), context, span)?;
                let sha256_hash =
                    rattler_digest::parse_digest_from_hex::<rattler_digest::Sha256>(&sha256_str)
                        .ok_or_else(|| ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: *span,
                            message: Some(format!("Invalid SHA256 checksum: {}", sha256_str)),
                            suggestion: None,
                        })?;
                Some(sha256_hash)
            }
        };

        let md5 = match &self.md5 {
            None => None,
            Some(Value::Concrete { value, .. }) => Some(*value),
            Some(Value::Template { template, span }) => {
                let md5_str = render_template(template.source(), context, span)?;
                let md5_hash =
                    rattler_digest::parse_digest_from_hex::<rattler_digest::Md5>(&md5_str)
                        .ok_or_else(|| ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: *span,
                            message: Some(format!("Invalid MD5 checksum: {}", md5_str)),
                            suggestion: None,
                        })?;
                Some(md5_hash)
            }
        };

        // Evaluate file_name
        let file_name = evaluate_optional_string_value(&self.file_name, context)?;

        // Evaluate patches
        let patches = evaluate_string_list(&self.patches, context)?
            .into_iter()
            .map(PathBuf::from)
            .collect();

        // Evaluate target_directory
        let target_directory = match &self.target_directory {
            None => None,
            Some(v) => match v {
                Value::Concrete { value: p, .. } => Some(p.clone()),
                Value::Template { template, span } => {
                    let s = render_template(template.source(), context, span)?;
                    Some(PathBuf::from(s))
                }
            },
        };

        Ok(Stage1UrlSource {
            url: urls,
            sha256,
            md5,
            file_name,
            patches,
            target_directory,
        })
    }
}

impl Evaluate for Stage0PathSource {
    type Output = Stage1PathSource;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        // Evaluate path
        let path = match &self.path {
            Value::Concrete { value: p, .. } => p.clone(),
            Value::Template { template, span } => {
                let s = render_template(template.source(), context, span)?;
                PathBuf::from(s)
            }
        };

        // Evaluate checksum fields separately (both can be set)
        // For hash types, we can just extract the concrete value or evaluate templates
        let sha256 = match &self.sha256 {
            None => None,
            Some(Value::Concrete { value, .. }) => Some(*value),
            Some(Value::Template { template, span }) => {
                let sha256_str = render_template(template.source(), context, span)?;
                let sha256_hash =
                    rattler_digest::parse_digest_from_hex::<rattler_digest::Sha256>(&sha256_str)
                        .ok_or_else(|| ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: *span,
                            message: Some(format!("Invalid SHA256 checksum: {}", sha256_str)),
                            suggestion: None,
                        })?;
                Some(sha256_hash)
            }
        };

        let md5 = match &self.md5 {
            None => None,
            Some(Value::Concrete { value, .. }) => Some(*value),
            Some(Value::Template { template, span }) => {
                let md5_str = render_template(template.source(), context, span)?;
                let md5_hash =
                    rattler_digest::parse_digest_from_hex::<rattler_digest::Md5>(&md5_str)
                        .ok_or_else(|| ParseError {
                            kind: ErrorKind::InvalidValue,
                            span: *span,
                            message: Some(format!("Invalid MD5 checksum: {}", md5_str)),
                            suggestion: None,
                        })?;
                Some(md5_hash)
            }
        };

        // Evaluate patches
        let patches = evaluate_string_list(&self.patches, context)?
            .into_iter()
            .map(PathBuf::from)
            .collect();

        // Evaluate target_directory
        let target_directory = match &self.target_directory {
            None => None,
            Some(v) => match v {
                Value::Concrete { value: p, .. } => Some(p.clone()),
                Value::Template { template, span } => {
                    let s = render_template(template.source(), context, span)?;
                    Some(PathBuf::from(s))
                }
            },
        };

        // Evaluate file_name
        let file_name = match &self.file_name {
            None => None,
            Some(v) => match v {
                Value::Concrete { value: p, .. } => Some(p.clone()),
                Value::Template { template, span } => {
                    let s = render_template(template.source(), context, span)?;
                    Some(PathBuf::from(s))
                }
            },
        };

        // Evaluate filter and convert to GlobVec (handle both list and include/exclude variants)
        let filter = evaluate_glob_vec(&self.filter, context)?;

        Ok(Stage1PathSource {
            path,
            sha256,
            md5,
            patches,
            target_directory,
            file_name,
            use_gitignore: self.use_gitignore,
            filter,
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
        match self {
            Stage0PythonVersion::Single(v) => Ok(Stage1PythonVersion::Single(
                evaluate_string_value(v, context)?,
            )),
            Stage0PythonVersion::Multiple(versions) => {
                let mut evaluated = Vec::new();
                for v in versions {
                    evaluated.push(evaluate_string_value(v, context)?);
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
        let script = evaluate_script_list(&self.script, context)?;
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
        let context_with_vars = if !self.context.is_empty() {
            context.with_context(&self.context)?
        } else {
            context.clone()
        };

        let package = self.package.evaluate(&context_with_vars)?;
        let build = self.build.evaluate(&context_with_vars)?;
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

        // Extract the resolved context variables (all variables from the evaluation context)
        let resolved_context = context_with_vars.variables().clone();

        Ok(Stage1Recipe::new(
            package,
            build,
            about,
            requirements,
            extra,
            source,
            tests,
            resolved_context,
        ))
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
        let result = render_template(template, &ctx, &Span::unknown()).unwrap();
        assert_eq!(result, "foo-1.0.0");
    }

    #[test]
    fn test_evaluate_string_value_concrete() {
        let value = Value::new_concrete("hello".to_string(), Span::unknown());
        let ctx = EvaluationContext::new();

        let result = evaluate_string_value(&value, &ctx).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_evaluate_string_value_template() {
        let value = Value::new_template(
            JinjaTemplate::new("${{ greeting }}, ${{ name }}!".to_string()).unwrap(),
            Span::unknown(),
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
            Item::Value(Value::new_concrete("gcc".to_string(), Span::unknown())),
            Item::Value(Value::new_concrete("make".to_string(), Span::unknown())),
        ]);

        let ctx = EvaluationContext::new();
        let result = evaluate_string_list(&list, &ctx).unwrap();
        assert_eq!(result, vec!["gcc", "make"]);
    }

    #[test]
    fn test_evaluate_string_list_with_conditional() {
        let list = ConditionalList::new(vec![
            Item::Value(Value::new_concrete("python".to_string(), Span::unknown())),
            Item::Conditional(Conditional {
                condition: JinjaExpression::new("unix".to_string()).unwrap(),
                then: ListOrItem::new(vec![Value::new_concrete(
                    "gcc".to_string(),
                    Span::unknown(),
                )]),
                else_value: Some(ListOrItem::new(vec![Value::new_concrete(
                    "msvc".to_string(),
                    Span::unknown(),
                )])),
            }),
        ]);

        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), Variable::from(true));

        let result = evaluate_string_list(&list, &ctx).unwrap();
        assert_eq!(result, vec!["python", "gcc"]);

        // Test with unix not set
        let ctx2 = EvaluationContext::new();
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
        let result = render_template(template, &ctx, &Span::unknown()).unwrap();
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
            then: ListOrItem::new(vec![Value::new_concrete(
                "gcc".to_string(),
                Span::unknown(),
            )]),
            else_value: Some(ListOrItem::new(vec![Value::new_concrete(
                "msvc".to_string(),
                Span::unknown(),
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
                Span::unknown(),
            )),
            Item::Value(Value::new_concrete(
                "static-dep".to_string(),
                Span::unknown(),
            )),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ version }}".to_string()).unwrap(),
                Span::unknown(),
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
        let _result = render_template(template, &ctx, &Span::unknown()).unwrap();

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
        let result = render_template(template, &ctx, &Span::unknown());

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
        let result = render_template(template, &ctx, &Span::unknown());

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
