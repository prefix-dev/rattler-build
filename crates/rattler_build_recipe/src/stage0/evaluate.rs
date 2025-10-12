//! Evaluation of stage0 types into stage1 types
//!
//! This module implements the `Evaluate` trait for stage0 types,
//! converting them into their stage1 equivalents by:
//! - Rendering Jinja templates
//! - Flattening conditionals based on the evaluation context
//! - Validating the results

use std::{path::PathBuf, str::FromStr};

use rattler_conda_types::{PackageName, Version};

use crate::{
    ErrorKind, ParseError, Span,
    stage0::{
        About as Stage0About, Build as Stage0Build, Extra as Stage0Extra, Package as Stage0Package,
        Requirements as Stage0Requirements, Source as Stage0Source, Stage0Recipe,
        TestType as Stage0TestType,
        build::{
            BinaryRelocation as Stage0BinaryRelocation, DynamicLinking as Stage0DynamicLinking,
            ForceFileType as Stage0ForceFileType, PostProcess as Stage0PostProcess,
            PrefixDetection as Stage0PrefixDetection, PrefixIgnore as Stage0PrefixIgnore,
            PythonBuild as Stage0PythonBuild, VariantKeyUsage as Stage0VariantKeyUsage,
        },
        jinja_functions::{setup_default_filters, setup_jinja_functions},
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
        About as Stage1About, AllOrGlobVec, Evaluate, EvaluationContext, Extra as Stage1Extra,
        GlobVec, Package as Stage1Package, Recipe as Stage1Recipe,
        Requirements as Stage1Requirements,
        build::{
            Build as Stage1Build, DynamicLinking as Stage1DynamicLinking,
            ForceFileType as Stage1ForceFileType, NoArchType, PostProcess as Stage1PostProcess,
            PrefixDetection as Stage1PrefixDetection, PythonBuild as Stage1PythonBuild,
            VariantKeyUsage as Stage1VariantKeyUsage,
        },
        requirements::{
            IgnoreRunExports as Stage1IgnoreRunExports, RunExports as Stage1RunExports,
        },
        source::{
            Checksum as Stage1Checksum, GitRev as Stage1GitRev, GitSource as Stage1GitSource,
            GitUrl as Stage1GitUrl, PathSource as Stage1PathSource, Source as Stage1Source,
            UrlSource as Stage1UrlSource,
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
use minijinja::{
    Environment,
    value::{Object, Value as MiniJinjaValue},
};
use std::sync::Arc;

/// A wrapper around the evaluation context that tracks variable access
///
/// This allows us to know which variables were actually used during template rendering,
/// which is important for understanding which conditional branches were taken.
#[derive(Debug, Clone)]
struct TrackingContext {
    evaluation_context: Arc<EvaluationContext>,
}

impl TrackingContext {
    fn new(context: &EvaluationContext) -> Self {
        Self {
            evaluation_context: Arc::new(context.clone()),
        }
    }
}

impl Object for TrackingContext {
    fn get_value(self: &Arc<Self>, key: &MiniJinjaValue) -> Option<MiniJinjaValue> {
        let key_str = key.as_str()?;

        // Track that this variable was accessed
        self.evaluation_context.track_access(key_str);

        // Get the value from the context
        match self.evaluation_context.get(key_str) {
            Some(v) => Some(MiniJinjaValue::from(v.as_str())),
            None => {
                // Track that this variable was undefined
                self.evaluation_context.track_undefined(key_str);
                None
            }
        }
    }
}

/// Helper to render a Jinja template with the evaluation context
fn render_template(
    template: &str,
    context: &EvaluationContext,
    span: &Span,
) -> Result<String, ParseError> {
    let mut env = Environment::new();

    // Use Strict undefined behavior to get clear error messages for undefined variables
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);

    // Setup Jinja functions (compiler, cdt, match, etc.)
    setup_jinja_functions(&mut env, context.jinja_config());

    // Setup default filters
    setup_default_filters(&mut env);

    // rattler-build uses ${{ }} syntax, but minijinja uses {{ }}
    // Strip the $ prefix from template expressions
    let normalized_template = template.replace("${{", "{{");

    // Create a tracking context that records variable access
    let tracking_ctx = TrackingContext::new(context);
    let ctx_value = MiniJinjaValue::from_object(tracking_ctx);

    match env.render_str(&normalized_template, ctx_value) {
        Ok(result) => Ok(result),
        Err(e) => {
            // Extract more information from the MiniJinja error
            let error_string = e.to_string();

            // Use our tracked undefined variables to provide helpful suggestions
            let suggestion = if error_string.contains("undefined") {
                let undefined_vars = context.undefined_variables();

                if undefined_vars.is_empty() {
                    // Shouldn't happen, but fallback gracefully
                    Some(
                        "Make sure all variables used in templates are defined in the evaluation context."
                            .to_string(),
                    )
                } else if undefined_vars.len() == 1 {
                    let var_name = undefined_vars.iter().next().unwrap();
                    Some(format!(
                        "The variable '{}' is not defined in the evaluation context. \
                         Make sure it is provided or defined in the context section.",
                        var_name
                    ))
                } else {
                    let mut vars: Vec<_> = undefined_vars.iter().collect();
                    vars.sort();
                    Some(format!(
                        "One or more variables ({}) are not defined in the evaluation context. \
                         Make sure all variables are provided or defined in the context section.",
                        vars.iter()
                            .map(|s| s.as_str())
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
        Value::Concrete(s) => Ok(s.clone()),
        Value::Template(template) => render_template(template.source(), context, &Span::unknown()),
    }
}

/// Evaluate a Value<T: ToString> into a String
pub fn evaluate_value_to_string<T: ToString>(
    value: &Value<T>,
    context: &EvaluationContext,
) -> Result<String, ParseError> {
    match value {
        Value::Concrete(v) => Ok(v.to_string()),
        Value::Template(template) => render_template(template.source(), context, &Span::unknown()),
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
                        results.push(val.clone());
                    }
                } else {
                    // Evaluate the "else" items
                    for val in cond.else_value.iter() {
                        results.push(val.clone());
                    }
                }
            }
        }
    }

    Ok(results)
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
        Value::Concrete(b) => Ok(*b),
        Value::Template(template) => {
            let s = render_template(template.source(), context, &Span::unknown())?;
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

        let version = Version::from_str(&version_str).map_err(|e| ParseError {
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
                Value::Concrete(license) => Some(license.0.clone()),
                Value::Template(template) => {
                    let s = render_template(template.source(), context, &Span::unknown())?;
                    Some(s.parse::<spdx::Expression>().map_err(|e| ParseError {
                        kind: ErrorKind::InvalidValue,
                        span: Span::unknown(),
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
            license_file: evaluate_optional_string_value(&self.license_file, context)?,
            summary: evaluate_optional_string_value(&self.summary, context)?,
            description: evaluate_optional_string_value(&self.description, context)?,
        })
    }
}

// Use macro for simple list field evaluations
impl_evaluate_list_fields!(Stage0RunExports => Stage1RunExports {
    noarch,
    strong,
    strong_constraints,
    weak,
    weak_constraints,
});

impl_evaluate_list_fields!(Stage0IgnoreRunExports => Stage1IgnoreRunExports {
    by_name,
    from_package,
});

impl Evaluate for Stage0Requirements {
    type Output = Stage1Requirements;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        Ok(Stage1Requirements {
            build: evaluate_string_list(&self.build, context)?,
            host: evaluate_string_list(&self.host, context)?,
            run: evaluate_string_list(&self.run, context)?,
            run_constraints: evaluate_string_list(&self.run_constraints, context)?,
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
        let skip_pyc_compilation =
            GlobVec::from_strings(evaluate_string_list(&self.skip_pyc_compilation, context)?)?;

        Ok(Stage1PythonBuild {
            entry_points: evaluate_string_list(&self.entry_points, context)?,
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
            text: GlobVec::from_strings(evaluate_string_list(&self.text, context)?)?,
            binary: GlobVec::from_strings(evaluate_string_list(&self.binary, context)?)?,
        })
    }
}

impl Evaluate for Stage0PrefixDetection {
    type Output = Stage1PrefixDetection;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let ignore = match &self.ignore {
            Stage0PrefixIgnore::Boolean(val) => {
                let bool_val = match val {
                    Value::Concrete(b) => *b,
                    Value::Template(template) => {
                        let s = render_template(template.source(), context, &Span::unknown())?;
                        match s.as_str() {
                            "true" | "True" | "yes" | "Yes" => true,
                            "false" | "False" | "no" | "No" => false,
                            _ => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: Span::unknown(),
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
                AllOrGlobVec::from_strings(evaluate_string_list(list, context)?)?
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
        Ok(Stage1PostProcess {
            files: GlobVec::from_strings(evaluate_string_list(&self.files, context)?)?,
            regex: evaluate_string_value(&self.regex, context)?,
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
                    Value::Concrete(b) => *b,
                    Value::Template(template) => {
                        let s = render_template(template.source(), context, &Span::unknown())?;
                        match s.as_str() {
                            "true" | "True" | "yes" | "Yes" => true,
                            "false" | "False" | "no" | "No" => false,
                            _ => {
                                return Err(ParseError {
                                    kind: ErrorKind::InvalidValue,
                                    span: Span::unknown(),
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
                AllOrGlobVec::from_strings(evaluate_string_list(list, context)?)?
            }
        };

        // Evaluate and validate glob patterns
        let missing_dso_allowlist =
            GlobVec::from_strings(evaluate_string_list(&self.missing_dso_allowlist, context)?)?;
        let rpath_allowlist =
            GlobVec::from_strings(evaluate_string_list(&self.rpath_allowlist, context)?)?;

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
        let script = evaluate_string_list(&self.script, context)?;

        // Evaluate noarch
        let noarch = match &self.noarch {
            None => None,
            Some(v) => {
                let s = evaluate_value_to_string(v, context)?;
                match s.as_str() {
                    "python" => Some(NoArchType::Python),
                    "generic" => Some(NoArchType::Generic),
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

        // Evaluate skip condition
        let skip = evaluate_optional_string_value(&self.skip, context)?;

        // Evaluate python configuration
        let python = self.python.evaluate(context)?;

        // Evaluate file lists and validate glob patterns
        let always_copy_files =
            GlobVec::from_strings(evaluate_string_list(&self.always_copy_files, context)?)?;
        let always_include_files =
            GlobVec::from_strings(evaluate_string_list(&self.always_include_files, context)?)?;
        let files = GlobVec::from_strings(evaluate_string_list(&self.files, context)?)?;

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

        Ok(Stage1Build {
            number: self.number,
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
                Value::Concrete(p) => Some(p.clone()),
                Value::Template(template) => {
                    let s = render_template(template.source(), context, &Span::unknown())?;
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

        // Evaluate checksum (sha256 or md5, but only one can be set)
        let checksum = if let Some(sha256) = &self.sha256 {
            let sha256_str = evaluate_string_value(sha256, context)?;
            Some(Stage1Checksum::Sha256(sha256_str))
        } else if let Some(md5) = &self.md5 {
            let md5_str = evaluate_string_value(md5, context)?;
            Some(Stage1Checksum::Md5(md5_str))
        } else {
            None
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
                Value::Concrete(p) => Some(p.clone()),
                Value::Template(template) => {
                    let s = render_template(template.source(), context, &Span::unknown())?;
                    Some(PathBuf::from(s))
                }
            },
        };

        Ok(Stage1UrlSource {
            url: urls,
            checksum,
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
            Value::Concrete(p) => p.clone(),
            Value::Template(template) => {
                let s = render_template(template.source(), context, &Span::unknown())?;
                PathBuf::from(s)
            }
        };

        // Evaluate checksum (sha256 or md5, but only one can be set)
        let checksum = if let Some(sha256) = &self.sha256 {
            let sha256_str = evaluate_string_value(sha256, context)?;
            Some(Stage1Checksum::Sha256(sha256_str))
        } else if let Some(md5) = &self.md5 {
            let md5_str = evaluate_string_value(md5, context)?;
            Some(Stage1Checksum::Md5(md5_str))
        } else {
            None
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
                Value::Concrete(p) => Some(p.clone()),
                Value::Template(template) => {
                    let s = render_template(template.source(), context, &Span::unknown())?;
                    Some(PathBuf::from(s))
                }
            },
        };

        // Evaluate file_name
        let file_name = match &self.file_name {
            None => None,
            Some(v) => match v {
                Value::Concrete(p) => Some(p.clone()),
                Value::Template(template) => {
                    let s = render_template(template.source(), context, &Span::unknown())?;
                    Some(PathBuf::from(s))
                }
            },
        };

        // Evaluate filter and convert to GlobVec
        let filter = GlobVec::from_strings(evaluate_string_list(&self.filter, context)?)?;

        Ok(Stage1PathSource {
            path,
            checksum,
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
            source: GlobVec::from_strings(evaluate_string_list(&self.source, context)?)?,
            recipe: GlobVec::from_strings(evaluate_string_list(&self.recipe, context)?)?,
        })
    }
}

impl Evaluate for Stage0CommandsTest {
    type Output = Stage1CommandsTest;

    fn evaluate(&self, context: &EvaluationContext) -> Result<Self::Output, ParseError> {
        let script = evaluate_string_list(&self.script, context)?;
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
            exists: GlobVec::from_strings(evaluate_string_list(&self.exists, context)?)?,
            not_exists: GlobVec::from_strings(evaluate_string_list(&self.not_exists, context)?)?,
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
        let package = self.package.evaluate(context)?;
        let build = self.build.evaluate(context)?;
        let about = self.about.evaluate(context)?;
        let requirements = self.requirements.evaluate(context)?;
        let extra = self.extra.evaluate(context)?;

        // Evaluate source list
        let mut source = Vec::new();
        for src in &self.source {
            source.push(src.evaluate(context)?);
        }

        // Evaluate tests list
        let mut tests = Vec::new();
        for test in &self.tests {
            tests.push(test.evaluate(context)?);
        }

        Ok(Stage1Recipe::new(
            package,
            build,
            about,
            requirements,
            extra,
            source,
            tests,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage0::types::{
        Conditional, ConditionalList, Item, JinjaTemplate, ListOrItem, Value,
    };

    #[test]
    fn test_evaluate_condition_simple() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), "true".to_string());

        let expr = JinjaExpression::new("unix".to_string()).unwrap();
        assert!(evaluate_condition(&expr, &ctx).unwrap());

        let expr2 = JinjaExpression::new("win".to_string()).unwrap();
        assert!(!evaluate_condition(&expr2, &ctx).unwrap());
    }

    #[test]
    fn test_evaluate_condition_not() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), "true".to_string());

        let expr = JinjaExpression::new("not unix".to_string()).unwrap();
        assert!(!evaluate_condition(&expr, &ctx).unwrap());

        let expr2 = JinjaExpression::new("not win".to_string()).unwrap();
        assert!(evaluate_condition(&expr2, &ctx).unwrap());
    }

    #[test]
    fn test_render_template_simple() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), "foo".to_string());
        ctx.insert("version".to_string(), "1.0.0".to_string());

        let template = "${{ name }}-${{ version }}";
        let result = render_template(template, &ctx, &Span::unknown()).unwrap();
        assert_eq!(result, "foo-1.0.0");
    }

    #[test]
    fn test_evaluate_string_value_concrete() {
        let value = Value::Concrete("hello".to_string());
        let ctx = EvaluationContext::new();

        let result = evaluate_string_value(&value, &ctx).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_evaluate_string_value_template() {
        let value = Value::Template(
            JinjaTemplate::new("${{ greeting }}, ${{ name }}!".to_string()).unwrap(),
        );

        let mut ctx = EvaluationContext::new();
        ctx.insert("greeting".to_string(), "Hello".to_string());
        ctx.insert("name".to_string(), "World".to_string());

        let result = evaluate_string_value(&value, &ctx).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_evaluate_string_list_simple() {
        let list = ConditionalList::new(vec![
            Item::Value(Value::Concrete("gcc".to_string())),
            Item::Value(Value::Concrete("make".to_string())),
        ]);

        let ctx = EvaluationContext::new();
        let result = evaluate_string_list(&list, &ctx).unwrap();
        assert_eq!(result, vec!["gcc", "make"]);
    }

    #[test]
    fn test_evaluate_string_list_with_conditional() {
        let list = ConditionalList::new(vec![
            Item::Value(Value::Concrete("python".to_string())),
            Item::Conditional(Conditional {
                condition: JinjaExpression::new("unix".to_string()).unwrap(),
                then: ListOrItem::new(vec!["gcc".to_string()]),
                else_value: ListOrItem::new(vec!["msvc".to_string()]),
            }),
        ]);

        let mut ctx = EvaluationContext::new();
        ctx.insert("unix".to_string(), "true".to_string());

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
        ctx.insert("name".to_string(), "foo".to_string());
        ctx.insert("version".to_string(), "1.0.0".to_string());
        ctx.insert("unused".to_string(), "bar".to_string());

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
        ctx.insert("unix".to_string(), "true".to_string());
        ctx.insert("unix_var".to_string(), "gcc".to_string());
        ctx.insert("win_var".to_string(), "msvc".to_string());

        let list = ConditionalList::new(vec![Item::Conditional(Conditional {
            condition: JinjaExpression::new("unix".to_string()).unwrap(),
            then: ListOrItem::new(vec!["gcc".to_string()]),
            else_value: ListOrItem::new(vec!["msvc".to_string()]),
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
        ctx.insert("compiler".to_string(), "gcc".to_string());
        ctx.insert("version".to_string(), "1.0.0".to_string());

        // Create a list with templates that will be evaluated
        let list = ConditionalList::new(vec![
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ compiler }}".to_string()).unwrap(),
            )),
            Item::Value(Value::Concrete("static-dep".to_string())),
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ version }}".to_string()).unwrap(),
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
        ctx.insert("name".to_string(), "foo".to_string());

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
    fn test_undefined_variable_error() {
        let ctx = EvaluationContext::new();
        // No variables set

        // Try to render a template with an undefined variable
        let template = "${{ undefined_var }}";
        let result = render_template(template, &ctx, &Span::unknown());

        // Should get an error about undefined variable
        assert!(result.is_err());
        let err = result.unwrap_err();
        let message = err.message.as_ref().unwrap();
        // The error should mention the undefined variable
        // MiniJinja's Strict mode produces an error like "undefined value 'undefined_var'"
        assert!(
            message.contains("undefined_var") || message.contains("undefined"),
            "Error message should mention undefined variable, got: {}",
            message
        );

        // Check that the undefined variable was tracked
        let undefined = ctx.undefined_variables();
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains("undefined_var"));
    }

    #[test]
    fn test_undefined_variable_tracking() {
        let mut ctx = EvaluationContext::new();
        ctx.insert("name".to_string(), "foo".to_string());
        // version is not defined

        // Try to render a template with an undefined variable
        let template = "${{ name }}-${{ version }}";
        let result = render_template(template, &ctx, &Span::unknown());

        // Should get an error
        assert!(result.is_err());

        // Check that only "version" was tracked as undefined, not "name"
        let undefined = ctx.undefined_variables();
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains("version"));
        assert!(!undefined.contains("name"));

        // Check that both were tracked as accessed
        let accessed = ctx.accessed_variables();
        assert_eq!(accessed.len(), 2);
        assert!(accessed.contains("name"));
        assert!(accessed.contains("version"));
    }

    #[test]
    fn test_multiple_undefined_variables() {
        let ctx = EvaluationContext::new();
        // No variables set

        let template = "${{ platform }} for ${{ arch }}";
        let result = render_template(template, &ctx, &Span::unknown());

        assert!(result.is_err());

        // Only the first undefined variable is tracked because MiniJinja stops at first error
        let undefined = ctx.undefined_variables();
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains("platform"));

        // The error suggestion should mention the undefined variable
        let err = result.unwrap_err();
        let suggestion = err.suggestion.unwrap();
        assert!(suggestion.contains("platform"));
    }
}
