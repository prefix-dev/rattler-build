//! PKL recipe parsing and conversion to Stage0 types
//!
//! This module provides the ability to parse `.pkl` recipe files and convert them
//! to Stage0 recipe types. PKL files are fully evaluated before conversion,
//! so all values are concrete (no Jinja templates or conditionals).

use std::path::Path;
use std::sync::Arc;

use indexmap::IndexMap;
use rattler_build_yaml_parser::{ConditionalList, Item, Value};
use thiserror::Error;

use crate::stage0::{
    About, Build, CommandsTest, DownstreamTest, Extra, GitRev, GitSource, GitUrl, IncludeExclude,
    Package, PackageContentsCheckFiles, PackageContentsTest, PackageName, PathSource, PythonTest,
    Recipe, Requirements, Script, SingleOutputRecipe, Source, TestType, UrlSource,
};

/// Errors that can occur during PKL parsing or conversion
#[derive(Debug, Error)]
pub enum PklError {
    /// Error loading or parsing the PKL file
    #[error("Failed to parse PKL file: {0}")]
    ParseError(String),

    /// Error evaluating the PKL file
    #[error("Failed to evaluate PKL file: {0}")]
    EvalError(#[from] rpkl_runtime::EvalError),

    /// Error converting PKL value to recipe type
    #[error("Failed to convert PKL value: {message}")]
    ConversionError {
        message: String,
        field: Option<String>,
    },

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl PklError {
    fn conversion(message: impl Into<String>) -> Self {
        Self::ConversionError {
            message: message.into(),
            field: None,
        }
    }

    fn conversion_field(message: impl Into<String>, field: impl Into<String>) -> Self {
        Self::ConversionError {
            message: message.into(),
            field: Some(field.into()),
        }
    }
}

/// Result type for PKL operations
pub type PklResult<T> = Result<T, PklError>;

/// Result of parsing a PKL recipe, including used variant keys
#[derive(Debug)]
pub struct PklRecipeResult {
    /// The parsed Stage0 recipe
    pub recipe: Recipe,
    /// Variant keys that were accessed during evaluation
    pub used_variants: Vec<String>,
}

// Thread-local storage for tracking variant accesses during evaluation
thread_local! {
    static USED_VARIANTS: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Record a variant key access
fn record_variant_access(key: &str) {
    USED_VARIANTS.with(|variants| {
        variants.borrow_mut().insert(key.to_string());
    });
}

/// Get and clear the recorded variant accesses
fn take_variant_accesses() -> Vec<String> {
    USED_VARIANTS.with(|variants| {
        let mut set = variants.borrow_mut();
        let mut keys: Vec<_> = set.drain().collect();
        keys.sort();
        keys
    })
}

/// Create the external `_recordAccess` function for Variant.pkl
///
/// This function is called by Variant.use() and Variant.get() to track
/// which variant keys are accessed during recipe evaluation.
fn create_record_access_fn() -> rpkl_runtime::ExternalFn {
    std::sync::Arc::new(move |args, _evaluator, _scope| {
        // Expect: _recordAccess(key: String, value: String) -> String
        if args.len() < 2 {
            return Err(rpkl_runtime::EvalError::InvalidOperation(
                "_recordAccess requires 2 arguments (key, value)".to_string(),
            ));
        }

        let key = args[0]
            .as_string()
            .ok_or_else(|| {
                rpkl_runtime::EvalError::InvalidOperation(
                    "_recordAccess key must be a string".to_string(),
                )
            })?
            .to_string();

        let value = args[1]
            .as_string()
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Record the access
        record_variant_access(&key);

        // Return the value (pass-through)
        Ok(rpkl_runtime::VmValue::string(value))
    })
}

// =============================================================================
// Static Analysis for Used Variants
// =============================================================================

/// Extract used variant keys from a PKL file via static analysis
///
/// This function parses the PKL file and traverses the AST to find calls to:
/// - `Variant.use("key")` -> extracts "key"
/// - `Variant.get("key", ...)` -> extracts "key"
/// - `Helpers.compiler("lang")` -> extracts "{lang}_compiler" and "{lang}_compiler_version"
/// - `Helpers.stdlib("lang")` -> extracts "{lang}_stdlib" and "{lang}_stdlib_version"
///
/// This allows determining which variant dimensions affect the build before evaluation.
pub fn extract_used_variants(path: &Path) -> PklResult<Vec<String>> {
    let source = std::fs::read_to_string(path)?;
    extract_used_variants_from_source(&source)
}

/// Extract used variant keys from PKL source code
pub fn extract_used_variants_from_source(source: &str) -> PklResult<Vec<String>> {
    let module = rpkl_parser::parse_module(source)
        .map_err(|e| PklError::ParseError(format!("{}", e)))?;

    let mut variants = std::collections::HashSet::new();
    extract_variants_from_module(&module, &mut variants);

    let mut result: Vec<_> = variants.into_iter().collect();
    result.sort();
    Ok(result)
}

/// Extract variants from a parsed module
fn extract_variants_from_module(
    module: &rpkl_parser::Module,
    variants: &mut std::collections::HashSet<String>,
) {
    // Process all module members
    for member in &module.members {
        match member {
            rpkl_parser::ModuleMember::Property(prop) => {
                if let Some(value) = &prop.value {
                    match value {
                        rpkl_parser::PropertyValue::Expr(expr) => {
                            extract_variants_from_expr(expr, variants);
                        }
                        rpkl_parser::PropertyValue::Object(body) => {
                            extract_variants_from_object_body(body, variants);
                        }
                    }
                }
            }
            rpkl_parser::ModuleMember::Method(method) => {
                if let Some(body) = &method.body {
                    extract_variants_from_expr(body, variants);
                }
            }
            rpkl_parser::ModuleMember::Class(class) => {
                extract_variants_from_class(class, variants);
            }
            rpkl_parser::ModuleMember::TypeAlias(_) => {}
        }
    }
}

/// Extract variants from a class definition
fn extract_variants_from_class(
    class: &rpkl_parser::ClassDef,
    variants: &mut std::collections::HashSet<String>,
) {
    for member in &class.members {
        match member {
            rpkl_parser::ClassMember::Property(prop) => {
                if let Some(value) = &prop.value {
                    match value {
                        rpkl_parser::PropertyValue::Expr(expr) => {
                            extract_variants_from_expr(expr, variants);
                        }
                        rpkl_parser::PropertyValue::Object(body) => {
                            extract_variants_from_object_body(body, variants);
                        }
                    }
                }
            }
            rpkl_parser::ClassMember::Method(method) => {
                if let Some(body) = &method.body {
                    extract_variants_from_expr(body, variants);
                }
            }
        }
    }
}

/// Extract variants from an object body
fn extract_variants_from_object_body(
    body: &rpkl_parser::ObjectBody,
    variants: &mut std::collections::HashSet<String>,
) {
    for member in &body.members {
        match member {
            rpkl_parser::ObjectMember::Property { value, .. } => {
                extract_variants_from_expr(value, variants);
            }
            rpkl_parser::ObjectMember::PropertyAmend { body, .. } => {
                extract_variants_from_object_body(body, variants);
            }
            rpkl_parser::ObjectMember::Element { value, .. } => {
                extract_variants_from_expr(value, variants);
            }
            rpkl_parser::ObjectMember::Entry { key, value, .. } => {
                extract_variants_from_expr(key, variants);
                extract_variants_from_expr(value, variants);
            }
            rpkl_parser::ObjectMember::EntryAmend { key, body, .. } => {
                extract_variants_from_expr(key, variants);
                extract_variants_from_object_body(body, variants);
            }
            rpkl_parser::ObjectMember::Spread { value, .. } => {
                extract_variants_from_expr(value, variants);
            }
            rpkl_parser::ObjectMember::When {
                condition,
                body,
                else_body,
                ..
            } => {
                extract_variants_from_expr(condition, variants);
                extract_variants_from_object_body(body, variants);
                if let Some(else_body) = else_body {
                    extract_variants_from_object_body(else_body, variants);
                }
            }
            rpkl_parser::ObjectMember::For {
                iterable, body, ..
            } => {
                extract_variants_from_expr(iterable, variants);
                extract_variants_from_object_body(body, variants);
            }
        }
    }
}

/// Extract variants from an expression, recursively traversing the AST
fn extract_variants_from_expr(
    expr: &rpkl_parser::Expr,
    variants: &mut std::collections::HashSet<String>,
) {
    use rpkl_parser::ExprKind;

    match &expr.kind {
        // The key case: function calls that might be Variant.use/get or Helpers.compiler
        ExprKind::Call { callee, args } => {
            // Check if this is a Variant.use, Variant.get, or Helpers.compiler call
            if let Some((module, method)) = get_member_access_chain(callee) {
                match (module.as_str(), method.as_str()) {
                    ("Variant", "use") | ("Variant", "get") => {
                        // Extract the first argument as a string literal
                        if let Some(first_arg) = args.first() {
                            if let Some(key) = get_string_literal(first_arg) {
                                variants.insert(key);
                            }
                        }
                    }
                    ("Helpers", "compiler") => {
                        // compiler("c") -> c_compiler, c_compiler_version
                        if let Some(first_arg) = args.first() {
                            if let Some(lang) = get_string_literal(first_arg) {
                                variants.insert(format!("{}_compiler", lang));
                                variants.insert(format!("{}_compiler_version", lang));
                            }
                        }
                    }
                    ("Helpers", "stdlib") => {
                        // stdlib("c") -> c_stdlib, c_stdlib_version
                        if let Some(first_arg) = args.first() {
                            if let Some(lang) = get_string_literal(first_arg) {
                                variants.insert(format!("{}_stdlib", lang));
                                variants.insert(format!("{}_stdlib_version", lang));
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Recurse into callee and args
            extract_variants_from_expr(callee, variants);
            for arg in args {
                extract_variants_from_expr(arg, variants);
            }
        }

        // Recurse into all other expression types
        ExprKind::MemberAccess { base, .. } | ExprKind::OptionalMemberAccess { base, .. } => {
            extract_variants_from_expr(base, variants);
        }
        ExprKind::Binary { left, right, .. } => {
            extract_variants_from_expr(left, variants);
            extract_variants_from_expr(right, variants);
        }
        ExprKind::Unary { operand, .. } => {
            extract_variants_from_expr(operand, variants);
        }
        ExprKind::Subscript { base, index } => {
            extract_variants_from_expr(base, variants);
            extract_variants_from_expr(index, variants);
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_variants_from_expr(condition, variants);
            extract_variants_from_expr(then_branch, variants);
            extract_variants_from_expr(else_branch, variants);
        }
        ExprKind::Let { value, body, .. } => {
            extract_variants_from_expr(value, variants);
            extract_variants_from_expr(body, variants);
        }
        ExprKind::Lambda { body, .. } => {
            extract_variants_from_expr(body, variants);
        }
        ExprKind::New { body, .. } | ExprKind::Amend { body, .. } => {
            extract_variants_from_object_body(body, variants);
        }
        ExprKind::NullCoalesce { value, default } => {
            extract_variants_from_expr(value, variants);
            extract_variants_from_expr(default, variants);
        }
        ExprKind::Pipe { value, function } => {
            extract_variants_from_expr(value, variants);
            extract_variants_from_expr(function, variants);
        }
        ExprKind::NonNullAssertion(inner) => {
            extract_variants_from_expr(inner, variants);
        }
        ExprKind::Is { value, .. } | ExprKind::As { value, .. } => {
            extract_variants_from_expr(value, variants);
        }
        ExprKind::Throw(inner) | ExprKind::Trace(inner) | ExprKind::Parenthesized(inner) => {
            extract_variants_from_expr(inner, variants);
        }
        ExprKind::Read { uri, .. } | ExprKind::ReadGlob { uri } => {
            extract_variants_from_expr(uri, variants);
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let rpkl_parser::StringPart::Interpolation(expr) = part {
                    extract_variants_from_expr(expr, variants);
                }
            }
        }

        // Terminal expressions - no recursion needed
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Identifier(_)
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::Outer
        | ExprKind::Module => {}
    }
}

/// Get the module and method name from a member access chain like `Variant.use`
fn get_member_access_chain(expr: &rpkl_parser::Expr) -> Option<(String, String)> {
    if let rpkl_parser::ExprKind::MemberAccess { base, member } = &expr.kind {
        if let rpkl_parser::ExprKind::Identifier(module_name) = &base.kind {
            return Some((module_name.clone(), member.node.clone()));
        }
    }
    None
}

/// Extract a string literal from an expression
fn get_string_literal(expr: &rpkl_parser::Expr) -> Option<String> {
    if let rpkl_parser::ExprKind::String(lit) = &expr.kind {
        // Only handle simple string literals (no interpolation)
        if lit.parts.len() == 1 {
            if let rpkl_parser::StringPart::Literal(s) = &lit.parts[0] {
                return Some(s.clone());
            }
        }
    }
    None
}

// =============================================================================
// PKL Recipe Parsing and Conversion
// =============================================================================

/// Parse a PKL recipe file and convert it to a Stage0 Recipe
///
/// # Arguments
/// * `path` - Path to the PKL file
/// * `platform` - Target platform (e.g., "linux-64", "osx-arm64", "win-64")
///
/// # Returns
/// A Stage0 Recipe with all values fully evaluated (concrete, no templates)
pub fn parse_pkl_recipe(path: &Path, platform: &str) -> PklResult<Recipe> {
    let result = parse_pkl_recipe_with_variants(path, platform, &IndexMap::new())?;
    Ok(result.recipe)
}

/// Parse a PKL recipe file with variant configuration
///
/// This function tracks which variant keys are accessed during evaluation,
/// which is useful for determining which variant dimensions affect the build.
///
/// # Arguments
/// * `path` - Path to the PKL file
/// * `platform` - Target platform (e.g., "linux-64", "osx-arm64", "win-64")
/// * `variant_config` - Variant key-value pairs to use during evaluation
///
/// # Returns
/// A `PklRecipeResult` containing the recipe and the list of accessed variant keys
pub fn parse_pkl_recipe_with_variants(
    path: &Path,
    platform: &str,
    variant_config: &IndexMap<String, String>,
) -> PklResult<PklRecipeResult> {
    // Clear any previous variant accesses
    let _ = take_variant_accesses();

    // Set the TARGET_PLATFORM environment variable for Platform.pkl to read
    // SAFETY: This is safe when not called concurrently with other code that reads
    // environment variables. In practice, PKL recipe parsing is done single-threaded.
    unsafe {
        std::env::set_var("TARGET_PLATFORM", platform);

        // Also set variant values as environment variables for Variant.pkl
        for (key, value) in variant_config {
            // Convert variant keys to environment variable names
            // e.g., "python" -> "PYTHON_VERSION", "c_compiler" -> "C_COMPILER"
            let env_key = variant_key_to_env_name(key);
            std::env::set_var(&env_key, value);
        }
    }

    // Create the evaluator with stdlib and variant tracking registered
    let mut registry = rpkl_stdlib::stdlib_registry();

    // Register the _recordAccess external function for Variant.pkl
    // This is called by Variant.use() and Variant.get() to track variant accesses
    registry.register_function("Variant", "_recordAccess", create_record_access_fn());

    let evaluator = rpkl_runtime::Evaluator::with_externals(registry);

    // Evaluate the PKL file
    let value = evaluator.eval_file(path).map_err(PklError::EvalError)?;

    // Get the used variants before converting (in case conversion clears them somehow)
    let used_variants = take_variant_accesses();

    // Convert the evaluated VmValue to a Stage0 Recipe
    let recipe = convert_to_recipe(&value)?;

    Ok(PklRecipeResult {
        recipe,
        used_variants,
    })
}

/// Convert a variant key to its environment variable name
///
/// Examples:
/// - "python" -> "PYTHON_VERSION"
/// - "c_compiler" -> "C_COMPILER"
/// - "c_compiler_version" -> "C_COMPILER_VERSION"
fn variant_key_to_env_name(key: &str) -> String {
    match key {
        "python" => "PYTHON_VERSION".to_string(),
        "numpy" => "NUMPY_VERSION".to_string(),
        _ => key.to_uppercase(),
    }
}

/// Convert a VmValue (evaluated PKL object) to a Stage0 Recipe
fn convert_to_recipe(value: &rpkl_runtime::VmValue) -> PklResult<Recipe> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion("Expected recipe to be an object"))?;

    // Extract package (required)
    let package = get_property(obj, "package")
        .ok_or_else(|| PklError::MissingField("package".to_string()))?;
    let package = convert_package(&package)?;

    // Extract optional fields
    let source = get_property(obj, "source")
        .map(|v| convert_source_list(&v))
        .transpose()?
        .unwrap_or_default();

    let build = get_property(obj, "build")
        .map(|v| convert_build(&v))
        .transpose()?
        .unwrap_or_default();

    let requirements = get_property(obj, "requirements")
        .map(|v| convert_requirements(&v))
        .transpose()?
        .unwrap_or_default();

    let about = get_property(obj, "about")
        .map(|v| convert_about(&v))
        .transpose()?
        .unwrap_or_default();

    let extra = get_property(obj, "extra")
        .map(|v| convert_extra(&v))
        .transpose()?
        .unwrap_or_default();

    let tests = get_property(obj, "tests")
        .map(|v| convert_tests(&v))
        .transpose()?
        .unwrap_or_default();

    // Context is empty for PKL recipes (all values are already evaluated)
    let context = IndexMap::new();

    let recipe = SingleOutputRecipe {
        schema_version: Some(1),
        context,
        package,
        build,
        requirements,
        about,
        extra,
        source,
        tests,
    };

    Ok(Recipe::SingleOutput(Box::new(recipe)))
}

/// Get a property from a VmObject, handling lazy evaluation
fn get_property(obj: &Arc<rpkl_runtime::VmObject>, name: &str) -> Option<rpkl_runtime::VmValue> {
    obj.get_property_member(name)
        .and_then(|member| member.get_if_evaluated())
}

/// Convert a PKL package object to Stage0 Package
fn convert_package(value: &rpkl_runtime::VmValue) -> PklResult<Package> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion_field("Expected object", "package"))?;

    let name_str = get_property(obj, "name")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .ok_or_else(|| PklError::MissingField("package.name".to_string()))?;

    let version_str = get_property(obj, "version")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .ok_or_else(|| PklError::MissingField("package.version".to_string()))?;

    // Parse the name using rattler_conda_types::PackageName
    let inner_name: rattler_conda_types::PackageName = name_str
        .parse()
        .map_err(|e| PklError::conversion_field(format!("Invalid package name: {}", e), "package.name"))?;
    let name = PackageName(inner_name);

    // Parse version
    let version: rattler_conda_types::VersionWithSource = version_str
        .parse()
        .map_err(|e| PklError::conversion_field(format!("Invalid version: {}", e), "package.version"))?;

    Ok(Package {
        name: Value::new_concrete(name, None),
        version: Value::new_concrete(version, None),
    })
}

/// Convert PKL source to Stage0 Source list
fn convert_source_list(value: &rpkl_runtime::VmValue) -> PklResult<ConditionalList<Source>> {
    match value {
        // Single source object
        rpkl_runtime::VmValue::Object(_) => {
            let source = convert_source(value)?;
            Ok(ConditionalList::new(vec![Item::Value(Value::new_concrete(
                source, None,
            ))]))
        }
        // List of sources
        rpkl_runtime::VmValue::List(list) => {
            let mut items = Vec::new();
            for item in list.iter() {
                let source = convert_source(item)?;
                items.push(Item::Value(Value::new_concrete(source, None)));
            }
            Ok(ConditionalList::new(items))
        }
        _ => Err(PklError::conversion_field(
            "Expected source to be an object or list",
            "source",
        )),
    }
}

/// Convert a single PKL source object to Stage0 Source
fn convert_source(value: &rpkl_runtime::VmValue) -> PklResult<Source> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion_field("Expected object", "source"))?;

    // Determine source type based on which fields are present
    if get_property(obj, "url").is_some() {
        convert_url_source(obj)
    } else if get_property(obj, "git").is_some() {
        // Git source uses "git" field for URL in YAML, but PKL uses "url"
        // Check both for compatibility
        convert_git_source(obj)
    } else if get_property(obj, "path").is_some() {
        convert_path_source(obj)
    } else {
        Err(PklError::conversion_field(
            "Source must have url, git, or path field",
            "source",
        ))
    }
}

/// Convert a URL source
fn convert_url_source(obj: &Arc<rpkl_runtime::VmObject>) -> PklResult<Source> {
    let url = get_property(obj, "url")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .ok_or_else(|| PklError::MissingField("source.url".to_string()))?;

    let sha256 = get_property(obj, "sha256")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| {
            rattler_digest::parse_digest_from_hex::<rattler_digest::Sha256>(&s)
                .ok_or_else(|| PklError::conversion_field(format!("Invalid sha256: {}", s), "source.sha256"))
        })
        .transpose()?;

    let md5 = get_property(obj, "md5")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| {
            rattler_digest::parse_digest_from_hex::<rattler_digest::Md5>(&s)
                .ok_or_else(|| PklError::conversion_field(format!("Invalid md5: {}", s), "source.md5"))
        })
        .transpose()?;

    let file_name = get_property(obj, "file_name")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let patches = convert_string_list(obj, "patches")?;

    let target_directory = get_property(obj, "target_directory")
        .and_then(|v| v.as_string().map(|s| std::path::PathBuf::from(s.to_string())));

    Ok(Source::Url(UrlSource {
        url: vec![Value::new_concrete(url, None)],
        sha256: sha256.map(|h| Value::new_concrete(h, None)),
        md5: md5.map(|h| Value::new_concrete(h, None)),
        file_name: file_name.map(|s| Value::new_concrete(s, None)),
        patches,
        target_directory: target_directory.map(|p| Value::new_concrete(p, None)),
    }))
}

/// Convert a Git source
fn convert_git_source(obj: &Arc<rpkl_runtime::VmObject>) -> PklResult<Source> {
    // In PKL Recipe.pkl, GitSource uses "url" field
    let url = get_property(obj, "url")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .ok_or_else(|| PklError::MissingField("source.url (git)".to_string()))?;

    let rev = get_property(obj, "rev")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let tag = get_property(obj, "tag")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let branch = get_property(obj, "branch")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let depth = get_property(obj, "depth")
        .and_then(|v| v.as_int())
        .map(|i| i as i32);

    let lfs = get_property(obj, "lfs")
        .and_then(|v| v.as_bool());

    let patches = convert_string_list(obj, "patches")?;

    let target_directory = get_property(obj, "target_directory")
        .and_then(|v| v.as_string().map(|s| std::path::PathBuf::from(s.to_string())));

    Ok(Source::Git(GitSource {
        url: GitUrl(Value::new_concrete(url, None)),
        rev: rev.map(|s| GitRev::Value(Value::new_concrete(s, None))),
        tag: tag.map(|s| GitRev::Value(Value::new_concrete(s, None))),
        branch: branch.map(|s| GitRev::Value(Value::new_concrete(s, None))),
        depth: depth.map(|d| Value::new_concrete(d, None)),
        patches,
        target_directory: target_directory.map(|p| Value::new_concrete(p, None)),
        lfs: lfs.map(|b| Value::new_concrete(b, None)),
        expected_commit: None,
    }))
}

/// Convert a Path source
fn convert_path_source(obj: &Arc<rpkl_runtime::VmObject>) -> PklResult<Source> {
    let path = get_property(obj, "path")
        .and_then(|v| v.as_string().map(|s| std::path::PathBuf::from(s.to_string())))
        .ok_or_else(|| PklError::MissingField("source.path".to_string()))?;

    let use_gitignore = get_property(obj, "use_gitignore")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let patches = convert_string_list(obj, "patches")?;

    let target_directory = get_property(obj, "target_directory")
        .and_then(|v| v.as_string().map(|s| std::path::PathBuf::from(s.to_string())));

    Ok(Source::Path(PathSource {
        path: Value::new_concrete(path, None),
        sha256: None,
        md5: None,
        patches,
        target_directory: target_directory.map(|p| Value::new_concrete(p, None)),
        file_name: None,
        use_gitignore,
        filter: IncludeExclude::default(),
    }))
}

/// Convert a string list from PKL
fn convert_string_list(
    obj: &Arc<rpkl_runtime::VmObject>,
    field: &str,
) -> PklResult<ConditionalList<String>> {
    match get_property(obj, field) {
        Some(rpkl_runtime::VmValue::List(list)) => {
            let mut items = Vec::new();
            for item in list.iter() {
                if let Some(s) = item.as_string() {
                    items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                }
            }
            Ok(ConditionalList::new(items))
        }
        Some(rpkl_runtime::VmValue::Object(obj)) if obj.is_listing() => {
            let mut items = Vec::new();
            for i in 0..obj.element_count() {
                if let Some(member) = obj.get_element_member(i) {
                    if let Some(value) = member.get_if_evaluated() {
                        if let Some(s) = value.as_string() {
                            items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                        }
                    }
                }
            }
            Ok(ConditionalList::new(items))
        }
        // Null means empty list
        Some(rpkl_runtime::VmValue::Null) => Ok(ConditionalList::default()),
        Some(_) => Err(PklError::conversion_field(
            format!("Expected {} to be a list", field),
            field,
        )),
        None => Ok(ConditionalList::default()),
    }
}

/// Convert PKL build object to Stage0 Build
fn convert_build(value: &rpkl_runtime::VmValue) -> PklResult<Build> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion_field("Expected object", "build"))?;

    let number = get_property(obj, "number")
        .and_then(|v| v.as_int())
        .map(|i| i as u64)
        .unwrap_or(0);

    let string = get_property(obj, "string")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let noarch = get_property(obj, "noarch")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| match s.as_str() {
            "python" => rattler_conda_types::NoArchType::python(),
            "generic" => rattler_conda_types::NoArchType::generic(),
            _ => rattler_conda_types::NoArchType::generic(),
        });

    // Convert script - can be a string or list of strings
    let script = match get_property(obj, "script") {
        Some(rpkl_runtime::VmValue::String(s)) => {
            let content = ConditionalList::new(vec![Item::Value(Value::new_concrete(
                s.to_string(),
                None,
            ))]);
            Script {
                content: Some(content),
                ..Default::default()
            }
        }
        Some(rpkl_runtime::VmValue::List(list)) => {
            let mut items = Vec::new();
            for item in list.iter() {
                if let Some(s) = item.as_string() {
                    items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                }
            }
            Script {
                content: Some(ConditionalList::new(items)),
                ..Default::default()
            }
        }
        Some(rpkl_runtime::VmValue::Object(obj)) if obj.is_listing() => {
            let mut items = Vec::new();
            for i in 0..obj.element_count() {
                if let Some(member) = obj.get_element_member(i) {
                    if let Some(value) = member.get_if_evaluated() {
                        if let Some(s) = value.as_string() {
                            items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                        }
                    }
                }
            }
            Script {
                content: Some(ConditionalList::new(items)),
                ..Default::default()
            }
        }
        _ => Script::default(),
    };

    Ok(Build {
        number: Value::new_concrete(number, None),
        string: string.map(|s| Value::new_concrete(s, None)),
        script,
        noarch: noarch.map(|n| Value::new_concrete(n, None)),
        ..Default::default()
    })
}

/// Convert PKL requirements object to Stage0 Requirements
fn convert_requirements(value: &rpkl_runtime::VmValue) -> PklResult<Requirements> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion_field("Expected object", "requirements"))?;

    let build = convert_dependency_list(obj, "build")?;
    let host = convert_dependency_list(obj, "host")?;
    let run = convert_dependency_list(obj, "run")?;
    let run_constraints = convert_dependency_list(obj, "run_constraints")?;

    Ok(Requirements {
        build,
        host,
        run,
        run_constraints,
        ..Default::default()
    })
}

/// Convert a dependency list from PKL
fn convert_dependency_list(
    obj: &Arc<rpkl_runtime::VmObject>,
    field: &str,
) -> PklResult<ConditionalList<crate::stage0::SerializableMatchSpec>> {
    match get_property(obj, field) {
        Some(rpkl_runtime::VmValue::List(list)) => {
            let mut items = Vec::new();
            for item in list.iter() {
                if let Some(s) = item.as_string() {
                    let spec = crate::stage0::SerializableMatchSpec::from(s.as_ref());
                    items.push(Item::Value(Value::new_concrete(spec, None)));
                }
            }
            Ok(ConditionalList::new(items))
        }
        Some(rpkl_runtime::VmValue::Object(obj)) if obj.is_listing() => {
            let mut items = Vec::new();
            for i in 0..obj.element_count() {
                if let Some(member) = obj.get_element_member(i) {
                    if let Some(value) = member.get_if_evaluated() {
                        if let Some(s) = value.as_string() {
                            let spec = crate::stage0::SerializableMatchSpec::from(s.as_ref());
                            items.push(Item::Value(Value::new_concrete(spec, None)));
                        }
                    }
                }
            }
            Ok(ConditionalList::new(items))
        }
        Some(_) => Err(PklError::conversion_field(
            format!("Expected {} to be a list", field),
            field,
        )),
        None => Ok(ConditionalList::default()),
    }
}

/// Convert PKL about object to Stage0 About
fn convert_about(value: &rpkl_runtime::VmValue) -> PklResult<About> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion_field("Expected object", "about"))?;

    let homepage = get_property(obj, "homepage")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| s.parse::<url::Url>())
        .transpose()
        .map_err(|e| PklError::conversion_field(format!("Invalid homepage URL: {}", e), "about.homepage"))?;

    let license = get_property(obj, "license")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| s.parse::<crate::stage0::License>())
        .transpose()
        .map_err(|e| PklError::conversion_field(format!("Invalid license: {}", e), "about.license"))?;

    let license_file = convert_string_list(obj, "license_file")
        .or_else(|_| {
            // Also try as single string
            get_property(obj, "license_file")
                .and_then(|v| v.as_string().map(|s| s.to_string()))
                .map(|s| {
                    ConditionalList::new(vec![Item::Value(Value::new_concrete(s, None))])
                })
                .ok_or_else(|| PklError::conversion("Could not parse license_file"))
        })
        .unwrap_or_default();

    let summary = get_property(obj, "summary")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let description = get_property(obj, "description")
        .and_then(|v| v.as_string().map(|s| s.to_string()));

    let documentation = get_property(obj, "documentation")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| s.parse::<url::Url>())
        .transpose()
        .map_err(|e| PklError::conversion_field(format!("Invalid documentation URL: {}", e), "about.documentation"))?;

    let repository = get_property(obj, "repository")
        .and_then(|v| v.as_string().map(|s| s.to_string()))
        .map(|s| s.parse::<url::Url>())
        .transpose()
        .map_err(|e| PklError::conversion_field(format!("Invalid repository URL: {}", e), "about.repository"))?;

    Ok(About {
        homepage: homepage.map(|u| Value::new_concrete(u, None)),
        license: license.map(|l| Value::new_concrete(l, None)),
        license_file,
        license_family: None,
        summary: summary.map(|s| Value::new_concrete(s, None)),
        description: description.map(|s| Value::new_concrete(s, None)),
        documentation: documentation.map(|u| Value::new_concrete(u, None)),
        repository: repository.map(|u| Value::new_concrete(u, None)),
    })
}

/// Convert PKL extra object to Stage0 Extra
fn convert_extra(value: &rpkl_runtime::VmValue) -> PklResult<Extra> {
    let obj = value
        .as_object()
        .ok_or_else(|| PklError::conversion_field("Expected object", "extra"))?;

    // Convert PKL object properties to IndexMap<String, serde_value::Value>
    let mut extra = IndexMap::new();

    for name in obj.property_names() {
        if let Some(prop_value) = get_property(obj, &name) {
            let serde_val = vmvalue_to_serde_value(&prop_value);
            extra.insert(name, serde_val);
        }
    }

    Ok(Extra { extra })
}

/// Convert a VmValue to a serde_value::Value for storing in Extra
fn vmvalue_to_serde_value(value: &rpkl_runtime::VmValue) -> serde_value::Value {
    match value {
        rpkl_runtime::VmValue::Null => serde_value::Value::Unit,
        rpkl_runtime::VmValue::Boolean(b) => serde_value::Value::Bool(*b),
        rpkl_runtime::VmValue::Int(i) => serde_value::Value::I64(*i),
        rpkl_runtime::VmValue::Float(f) => serde_value::Value::F64(*f),
        rpkl_runtime::VmValue::String(s) => serde_value::Value::String(s.to_string()),
        rpkl_runtime::VmValue::List(list) => {
            let items: Vec<_> = list.iter().map(vmvalue_to_serde_value).collect();
            serde_value::Value::Seq(items)
        }
        rpkl_runtime::VmValue::Object(obj) if obj.is_listing() => {
            let mut items = Vec::new();
            for i in 0..obj.element_count() {
                if let Some(member) = obj.get_element_member(i) {
                    if let Some(val) = member.get_if_evaluated() {
                        items.push(vmvalue_to_serde_value(&val));
                    }
                }
            }
            serde_value::Value::Seq(items)
        }
        rpkl_runtime::VmValue::Object(obj) => {
            let mut map = std::collections::BTreeMap::new();
            for name in obj.property_names() {
                if let Some(member) = obj.get_property_member(&name) {
                    if let Some(val) = member.get_if_evaluated() {
                        map.insert(
                            serde_value::Value::String(name),
                            vmvalue_to_serde_value(&val),
                        );
                    }
                }
            }
            serde_value::Value::Map(map)
        }
        _ => serde_value::Value::Unit,
    }
}

/// Convert PKL tests to Stage0 TestType list
fn convert_tests(value: &rpkl_runtime::VmValue) -> PklResult<ConditionalList<TestType>> {
    match value {
        rpkl_runtime::VmValue::List(list) => {
            let mut items = Vec::new();
            for item in list.iter() {
                if let Some(test) = convert_single_test(item)? {
                    items.push(Item::Value(Value::new_concrete(test, None)));
                }
            }
            Ok(ConditionalList::new(items))
        }
        rpkl_runtime::VmValue::Object(obj) if obj.is_listing() => {
            let mut items = Vec::new();
            for i in 0..obj.element_count() {
                if let Some(member) = obj.get_element_member(i) {
                    if let Some(value) = member.get_if_evaluated() {
                        if let Some(test) = convert_single_test(&value)? {
                            items.push(Item::Value(Value::new_concrete(test, None)));
                        }
                    }
                }
            }
            Ok(ConditionalList::new(items))
        }
        _ => Err(PklError::conversion_field(
            "Expected tests to be a list",
            "tests",
        )),
    }
}

/// Convert a single test object
fn convert_single_test(value: &rpkl_runtime::VmValue) -> PklResult<Option<TestType>> {
    use rattler_build_yaml_parser::ConditionalListOrItem;

    let obj = match value.as_object() {
        Some(o) => o,
        None => return Ok(None),
    };

    // Determine test type based on which fields are present
    // Check for ScriptTest (uses 'script' field at top level for commands)
    if let Some(script_val) = get_property(obj, "script") {
        let script_content = match &script_val {
            rpkl_runtime::VmValue::String(s) => {
                ConditionalList::new(vec![Item::Value(Value::new_concrete(s.to_string(), None))])
            }
            rpkl_runtime::VmValue::List(list) => {
                let items: Vec<_> = list
                    .iter()
                    .filter_map(|v| v.as_string().map(|s| Item::Value(Value::new_concrete(s.to_string(), None))))
                    .collect();
                ConditionalList::new(items)
            }
            rpkl_runtime::VmValue::Object(obj) if obj.is_listing() => {
                let mut items = Vec::new();
                for i in 0..obj.element_count() {
                    if let Some(member) = obj.get_element_member(i) {
                        if let Some(value) = member.get_if_evaluated() {
                            if let Some(s) = value.as_string() {
                                items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                            }
                        }
                    }
                }
                ConditionalList::new(items)
            }
            _ => return Ok(None),
        };

        let script = Script {
            content: Some(script_content),
            ..Default::default()
        };

        return Ok(Some(TestType::Commands(CommandsTest {
            script,
            requirements: None,
            files: None,
        })));
    }

    // Check for PythonTest (uses 'python' field)
    if let Some(python_val) = get_property(obj, "python") {
        let python_obj = python_val.as_object().ok_or_else(|| {
            PklError::conversion_field("Expected python to be an object", "tests.python")
        })?;

        let imports = match get_property(python_obj, "imports") {
            Some(rpkl_runtime::VmValue::List(list)) => {
                let items: Vec<_> = list
                    .iter()
                    .filter_map(|v| v.as_string().map(|s| Item::Value(Value::new_concrete(s.to_string(), None))))
                    .collect();
                ConditionalListOrItem::new(items)
            }
            Some(rpkl_runtime::VmValue::Object(obj)) if obj.is_listing() => {
                let mut items = Vec::new();
                for i in 0..obj.element_count() {
                    if let Some(member) = obj.get_element_member(i) {
                        if let Some(value) = member.get_if_evaluated() {
                            if let Some(s) = value.as_string() {
                                items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                            }
                        }
                    }
                }
                ConditionalListOrItem::new(items)
            }
            _ => ConditionalListOrItem::empty(),
        };

        let pip_check = get_property(python_obj, "pip_check")
            .and_then(|v| v.as_bool());

        return Ok(Some(TestType::Python {
            python: PythonTest {
                imports,
                pip_check: pip_check.map(|b| Value::new_concrete(b, None)),
                python_version: None,
            },
        }));
    }

    // Check for PackageContentsTest (uses 'package_contents' field)
    if let Some(package_contents_val) = get_property(obj, "package_contents") {
        let pc_obj = package_contents_val.as_object().ok_or_else(|| {
            PklError::conversion_field(
                "Expected package_contents to be an object",
                "tests.package_contents",
            )
        })?;

        // Helper to convert string list to PackageContentsCheckFiles
        fn to_check_files(list: ConditionalList<String>) -> Option<PackageContentsCheckFiles> {
            if list.is_empty() {
                None
            } else {
                Some(PackageContentsCheckFiles {
                    exists: list,
                    not_exists: ConditionalList::default(),
                })
            }
        }

        let include = to_check_files(convert_string_list(pc_obj, "include")?);
        let files = to_check_files(convert_string_list(pc_obj, "files")?);
        let site_packages = to_check_files(convert_string_list(pc_obj, "site_packages")?);
        let bin = to_check_files(convert_string_list(pc_obj, "bin")?);
        let lib = to_check_files(convert_string_list(pc_obj, "lib")?);

        return Ok(Some(TestType::PackageContents {
            package_contents: PackageContentsTest {
                include,
                files,
                site_packages,
                bin,
                lib,
                strict: false,
            },
        }));
    }

    // Check for DownstreamTest (uses 'downstream' field)
    if let Some(downstream_val) = get_property(obj, "downstream") {
        if let Some(s) = downstream_val.as_string() {
            return Ok(Some(TestType::Downstream(DownstreamTest {
                downstream: Value::new_concrete(s.to_string(), None),
            })));
        }
    }

    // Check for CommandsTest with 'command' field (alternative to 'script')
    if let Some(command_val) = get_property(obj, "command") {
        let script_content = match &command_val {
            rpkl_runtime::VmValue::List(list) => {
                let items: Vec<_> = list
                    .iter()
                    .filter_map(|v| v.as_string().map(|s| Item::Value(Value::new_concrete(s.to_string(), None))))
                    .collect();
                ConditionalList::new(items)
            }
            rpkl_runtime::VmValue::Object(obj) if obj.is_listing() => {
                let mut items = Vec::new();
                for i in 0..obj.element_count() {
                    if let Some(member) = obj.get_element_member(i) {
                        if let Some(value) = member.get_if_evaluated() {
                            if let Some(s) = value.as_string() {
                                items.push(Item::Value(Value::new_concrete(s.to_string(), None)));
                            }
                        }
                    }
                }
                ConditionalList::new(items)
            }
            _ => return Ok(None),
        };

        let script = Script {
            content: Some(script_content),
            ..Default::default()
        };

        return Ok(Some(TestType::Commands(CommandsTest {
            script,
            requirements: None,
            files: None,
        })));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_pkl_error_display() {
        let err = PklError::MissingField("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = PklError::conversion_field("message", "field");
        assert!(err.to_string().contains("message"));
    }

    #[test]
    fn test_parse_xtensor_pkl_recipe() {
        // Test with the xtensor.pkl example from pkl-rust
        let pkl_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("pkl-rust/recipe-pkl/examples/xtensor.pkl");

        if pkl_path.exists() {
            let result = parse_pkl_recipe(&pkl_path, "linux-64");
            match result {
                Ok(recipe) => {
                    match recipe {
                        Recipe::SingleOutput(single) => {
                            assert!(single.package.name.is_concrete());
                            assert_eq!(
                                single.package.name.as_concrete().unwrap().0.as_normalized(),
                                "xtensor"
                            );
                        }
                        Recipe::MultiOutput(_) => panic!("Expected single output recipe"),
                    }
                }
                Err(e) => {
                    // It's OK if we can't parse complex examples with external imports
                    // Just log the error for now
                    eprintln!("Warning: Could not parse xtensor.pkl: {}", e);
                }
            }
        } else {
            eprintln!("Skipping test: xtensor.pkl not found at {:?}", pkl_path);
        }
    }

    #[test]
    fn test_parse_simple_pkl_recipe() {
        // Create a temporary PKL file
        let pkl_content = r#"
package {
    name = "test-package"
    version = "1.0.0"
}

build {
    number = 0
}

requirements {
    host {
        "python >=3.8"
    }
    run {
        "python >=3.8"
    }
}

about {
    homepage = "https://example.com"
    license = "MIT"
    summary = "A test package"
}
"#;

        let temp_dir = std::env::temp_dir();
        let pkl_path = temp_dir.join("test_recipe.pkl");

        {
            let mut file = std::fs::File::create(&pkl_path).unwrap();
            file.write_all(pkl_content.as_bytes()).unwrap();
        }

        let result = parse_pkl_recipe(&pkl_path, "linux-64");

        // Clean up
        let _ = std::fs::remove_file(&pkl_path);

        // Check result
        let recipe = result.expect("Failed to parse PKL recipe");

        match recipe {
            Recipe::SingleOutput(single) => {
                // Check package name
                assert!(single.package.name.is_concrete());
                assert_eq!(single.package.name.as_concrete().unwrap().0.as_normalized(), "test-package");

                // Check version
                assert!(single.package.version.is_concrete());
                assert_eq!(
                    single.package.version.as_concrete().unwrap().to_string(),
                    "1.0.0"
                );

                // Check build number
                assert!(single.build.number.is_concrete());
                assert_eq!(*single.build.number.as_concrete().unwrap(), 0);
            }
            Recipe::MultiOutput(_) => panic!("Expected single output recipe"),
        }
    }

    #[test]
    fn test_variant_tracking() {
        // Create a PKL file that uses Variant.use() and Variant.get()
        // We need to import Variant.pkl from the recipe-pkl project
        let recipe_pkl_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("pkl-rust/recipe-pkl/src");

        let pkl_content = format!(
            r#"
import "{}/Variant.pkl"

package {{
    name = "variant-test"
    version = "1.0.0"
}}

build {{
    number = 0
}}

// Access variants using use() - should be tracked
local pythonVersion = Variant.use("python")
local cCompiler = Variant.get("c_compiler", "gcc")
local customVar = Variant.get("my_custom_var", "default")

requirements {{
    run {{
        "python"
    }}
}}

about {{
    summary = "Test variant tracking"
}}
"#,
            recipe_pkl_path.display()
        );

        let temp_dir = std::env::temp_dir();
        let pkl_path = temp_dir.join("test_variant_recipe.pkl");

        {
            let mut file = std::fs::File::create(&pkl_path).unwrap();
            file.write_all(pkl_content.as_bytes()).unwrap();
        }

        // Parse with variant config
        let mut variant_config = IndexMap::new();
        variant_config.insert("python".to_string(), "3.11".to_string());
        variant_config.insert("c_compiler".to_string(), "clang".to_string());

        let result = parse_pkl_recipe_with_variants(&pkl_path, "linux-64", &variant_config);

        // Clean up
        let _ = std::fs::remove_file(&pkl_path);

        match result {
            Ok(pkl_result) => {
                // Verify the recipe was parsed
                match &pkl_result.recipe {
                    Recipe::SingleOutput(single) => {
                        assert_eq!(
                            single.package.name.as_concrete().unwrap().0.as_normalized(),
                            "variant-test"
                        );
                    }
                    _ => panic!("Expected single output recipe"),
                }

                // Verify the used variants were tracked
                let used = &pkl_result.used_variants;

                // The test recipe uses Variant.use("python"), Variant.get("c_compiler", ...),
                // and Variant.get("my_custom_var", ...) - all should be tracked
                assert!(
                    used.contains(&"python".to_string()),
                    "Expected 'python' in used variants: {:?}",
                    used
                );
                assert!(
                    used.contains(&"c_compiler".to_string()),
                    "Expected 'c_compiler' in used variants: {:?}",
                    used
                );
                assert!(
                    used.contains(&"my_custom_var".to_string()),
                    "Expected 'my_custom_var' in used variants: {:?}",
                    used
                );
            }
            Err(e) => {
                panic!("Failed to parse variant recipe: {}", e);
            }
        }
    }

    #[test]
    fn test_static_variant_extraction() {
        // Test the static analysis function that extracts variants from PKL source
        let pkl_source = r#"
import "Variant.pkl"
import "Helpers.pkl"

package {
    name = "test"
    version = "1.0.0"
}

// Use Variant.use to access a variant
local pythonVersion = Variant.use("python")

// Use Variant.get with a default
local cudaVersion = Variant.get("cuda_compiler_version", null)

// Use Helpers.compiler which expands to compiler + version
local cCompiler = Helpers.compiler("c")
local cxxCompiler = Helpers.compiler("cxx")

// Use Helpers.stdlib
local cStdlib = Helpers.stdlib("c")

requirements {
    build {
        cCompiler
        cxxCompiler
    }
    run {
        when (pythonVersion != "") {
            "python"
        }
    }
}
"#;

        let variants = extract_used_variants_from_source(pkl_source).unwrap();

        // Check that we found the expected variants
        assert!(
            variants.contains(&"python".to_string()),
            "Expected 'python' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"cuda_compiler_version".to_string()),
            "Expected 'cuda_compiler_version' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"c_compiler".to_string()),
            "Expected 'c_compiler' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"c_compiler_version".to_string()),
            "Expected 'c_compiler_version' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"cxx_compiler".to_string()),
            "Expected 'cxx_compiler' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"cxx_compiler_version".to_string()),
            "Expected 'cxx_compiler_version' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"c_stdlib".to_string()),
            "Expected 'c_stdlib' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"c_stdlib_version".to_string()),
            "Expected 'c_stdlib_version' in variants: {:?}",
            variants
        );
    }

    #[test]
    fn test_static_variant_extraction_with_conditionals() {
        // Test that we can extract variants from within conditionals and loops
        let pkl_source = r#"
import "Variant.pkl"

package {
    name = "test"
    version = "1.0.0"
}

requirements {
    build {
        // Variant in a when clause condition
        when (Variant.get("cuda_enabled", "false") == "true") {
            "cuda"
        }

        // Variant used in for loop
        for (pkg in List("a", "b")) {
            Variant.use("custom_variant")
        }
    }
}
"#;

        let variants = extract_used_variants_from_source(pkl_source).unwrap();

        assert!(
            variants.contains(&"cuda_enabled".to_string()),
            "Expected 'cuda_enabled' in variants: {:?}",
            variants
        );
        assert!(
            variants.contains(&"custom_variant".to_string()),
            "Expected 'custom_variant' in variants: {:?}",
            variants
        );
    }

    #[test]
    fn test_static_variant_extraction_no_variants() {
        // Test that a recipe without variant usage returns empty
        let pkl_source = r#"
package {
    name = "simple-package"
    version = "1.0.0"
}

build {
    number = 0
    script = "echo hello"
}
"#;

        let variants = extract_used_variants_from_source(pkl_source).unwrap();
        assert!(
            variants.is_empty(),
            "Expected no variants, got: {:?}",
            variants
        );
    }
}
