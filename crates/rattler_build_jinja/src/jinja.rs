//! Module for types and functions related to minijinja setup for recipes.
use fs_err as fs;
use indexmap::IndexMap;
use minijinja::{
    Environment, Value,
    syntax::SyntaxConfig,
    value::{Kwargs, Object},
};
use rattler_build_types::{NormalizedKey, Pin, PinArgs};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    str::FromStr as _,
};

use rattler_conda_types::{Arch, PackageName, ParseStrictness, Platform, Version, VersionSpec};
use strum::IntoEnumIterator as _;

use crate::variable::Variable;

pub use minijinja::UndefinedBehavior;

/// Known Jinja function names that are registered in the environment.
///
/// These are looked up in the context first (where they appear as "accessed" or "undefined"),
/// but are then resolved as functions from the environment.
const KNOWN_FUNCTIONS: &[&str] = &[
    "match",
    "cdt",
    "compiler",
    "stdlib",
    "pin_subpackage",
    "pin_compatible",
    "is_linux",
    "is_osx",
    "is_windows",
    "is_unix",
    "load_from_file",
    "git",
    "env",
    "cmp",
    "hash",
];

/// Configuration for Jinja template rendering in rattler-build
#[derive(Debug, Clone)]
pub struct JinjaConfig {
    /// The target platform for the build
    pub target_platform: Platform,
    /// The build platform (where the build is happening)
    pub build_platform: Platform,
    /// The host platform (where the package will run, defaults to target_platform if not set)
    pub host_platform: Platform,
    /// Variant configuration (compiler versions, etc.)
    pub variant: BTreeMap<NormalizedKey, Variable>,
    /// Whether experimental features are enabled
    pub experimental: bool,
    /// Path to the recipe file (for relative path resolution in load_from_file)
    pub recipe_path: Option<PathBuf>,
    /// Undefined behavior for minijinja (defaults to SemiStrict)
    pub undefined_behavior: UndefinedBehavior,
}

impl Default for JinjaConfig {
    fn default() -> Self {
        let current = Platform::current();
        Self {
            target_platform: current,
            build_platform: current,
            host_platform: current,
            variant: BTreeMap::new(),
            experimental: false,
            recipe_path: None,
            undefined_behavior: UndefinedBehavior::SemiStrict,
        }
    }
}

/// A wrapper around the context that tracks variable access
///
/// This allows us to know which variables were actually used during template rendering,
/// which is important for understanding which variables are used and which are undefined.
#[derive(Debug, Clone)]
struct TrackingContext {
    context: BTreeMap<String, Value>,
    accessed_variables: Arc<Mutex<HashSet<String>>>,
    undefined_variables: Arc<Mutex<HashSet<String>>>,
}

impl TrackingContext {
    fn new(
        context: BTreeMap<String, Value>,
        accessed_variables: Arc<Mutex<HashSet<String>>>,
        undefined_variables: Arc<Mutex<HashSet<String>>>,
    ) -> Self {
        Self {
            context,
            accessed_variables,
            undefined_variables,
        }
    }
}

impl Object for TrackingContext {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let key_str = key.as_str()?;

        // Track that this variable was accessed
        if let Ok(mut accessed) = self.accessed_variables.lock() {
            accessed.insert(key_str.to_string());
        }

        // Get the value from the context
        match self.context.get(key_str) {
            Some(v) => Some(v.clone()),
            None => {
                // Track that this variable was undefined
                if let Ok(mut undefined) = self.undefined_variables.lock() {
                    undefined.insert(key_str.to_string());
                }
                None
            }
        }
    }
}

/// The internal representation of the pin function.
pub enum InternalRepr {
    /// The pin function is used to pin a subpackage.
    PinSubpackage,
    /// The pin function is used to pin a compatible package.
    PinCompatible,
}

impl InternalRepr {
    fn to_json(&self, pin: &Pin) -> String {
        // Create a JSON object matching the expected stage1 Dependency format
        #[derive(serde::Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Internal {
            PinSubpackage(Pin),
            PinCompatible(Pin),
        }

        match self {
            InternalRepr::PinSubpackage => {
                serde_json::to_string(&Internal::PinSubpackage(pin.clone())).unwrap()
            }
            InternalRepr::PinCompatible => {
                serde_json::to_string(&Internal::PinCompatible(pin.clone())).unwrap()
            }
        }
    }
}

/// A type that hold the minijinja environment and context for Jinja template processing.
#[derive(Debug, Clone)]
pub struct Jinja {
    env: Environment<'static>,
    context: BTreeMap<String, Value>,
    /// Set of variables that were accessed during template rendering
    accessed_variables: Arc<Mutex<HashSet<String>>>,
    /// Set of variables that were accessed but undefined
    undefined_variables: Arc<Mutex<HashSet<String>>>,
}

impl Jinja {
    /// Create a new Jinja instance with the given configuration
    pub fn new(config: JinjaConfig) -> Self {
        let accessed_variables = Arc::new(Mutex::new(HashSet::new()));
        let undefined_variables = Arc::new(Mutex::new(HashSet::new()));
        let env = set_jinja(&config, accessed_variables.clone());
        let mut context = BTreeMap::new();

        // Add platform variables to context
        context.insert(
            "target_platform".to_string(),
            Value::from(config.target_platform.to_string()),
        );
        context.insert(
            "build_platform".to_string(),
            Value::from(config.build_platform.to_string()),
        );
        context.insert(
            "host_platform".to_string(),
            Value::from(config.host_platform.to_string()),
        );

        // Add common platform shortcuts
        context.insert(
            "unix".to_string(),
            Value::from(config.target_platform.is_unix()),
        );
        context.insert(
            "linux".to_string(),
            Value::from(config.target_platform.is_linux()),
        );
        context.insert(
            "osx".to_string(),
            Value::from(config.target_platform.is_osx()),
        );
        context.insert(
            "win".to_string(),
            Value::from(config.target_platform.is_windows()),
        );

        // Add architecture aliases (e.g., "x86_64", "aarch64", "ppc64le")
        // All known architectures are defined, with only the current target's architecture being true
        let current_arch = config.target_platform.arch();
        for arch in Arch::iter() {
            context.insert(arch.to_string(), Value::from(current_arch == Some(arch)));
        }

        // Add platform aliases (e.g., "linux64", "osx64", "win64")
        // These are the platform string with "-" removed (e.g., "linux-64" -> "linux64")
        // All known platforms get an alias, with only the current target_platform being true
        for platform in Platform::iter() {
            // Skip noarch and unknown platforms
            if matches!(platform, Platform::NoArch | Platform::Unknown) {
                continue;
            }
            let alias = platform.to_string().replace('-', "");
            context.insert(alias, Value::from(platform == config.target_platform));
        }

        // Add variant variables to context
        for (key, value) in &config.variant {
            context.insert(key.normalize(), value.clone().into());
        }

        Self {
            env,
            context,
            accessed_variables,
            undefined_variables,
        }
    }

    /// Add in the variables from the given context.
    pub fn with_context(mut self, context: &IndexMap<String, Variable>) -> Self {
        for (k, v) in context {
            self.context_mut().insert(k.clone(), v.clone().into());
        }
        self
    }

    /// Get a reference to the minijinja environment.
    pub fn env(&self) -> &Environment<'static> {
        &self.env
    }

    /// Get a mutable reference to the minijinja environment.
    ///
    /// This is useful for adding custom functions to the environment.
    pub fn env_mut(&mut self) -> &mut Environment<'static> {
        &mut self.env
    }

    /// Get a reference to the minijinja context.
    pub fn context(&self) -> &BTreeMap<String, Value> {
        &self.context
    }

    /// Get a mutable reference to the minijinja context.
    ///
    /// This is useful for adding custom variables to the context.
    pub fn context_mut(&mut self) -> &mut BTreeMap<String, Value> {
        &mut self.context
    }

    /// Render a template with the current context.
    ///
    /// This will track accessed and undefined variables during rendering using
    /// a TrackingContext object wrapper.
    pub fn render_str(&self, template: &str) -> Result<String, minijinja::Error> {
        // Create a TrackingContext that wraps our context
        let tracking_context = TrackingContext::new(
            self.context.clone(),
            self.accessed_variables.clone(),
            self.undefined_variables.clone(),
        );

        // Render with the tracking context as a minijinja Object
        // The Value::from_object wraps it in an Arc internally
        self.env
            .render_str(template, Value::from_object(tracking_context))
    }

    /// Render, compile and evaluate a expr string with the current context.
    ///
    /// This will track accessed and undefined variables during evaluation using
    /// a TrackingContext object wrapper.
    pub fn eval(&self, str: &str) -> Result<Value, minijinja::Error> {
        // Create a TrackingContext that wraps our context
        let tracking_context = TrackingContext::new(
            self.context.clone(),
            self.accessed_variables.clone(),
            self.undefined_variables.clone(),
        );

        let expr = self.env.compile_expression(str)?;
        expr.eval(Value::from_object(tracking_context))
    }

    /// Get the set of variables that were accessed during rendering
    pub fn accessed_variables(&self) -> HashSet<String> {
        self.accessed_variables
            .lock()
            .map(|accessed| accessed.clone())
            .unwrap_or_default()
    }

    /// Get the set of variables that were accessed during rendering,
    /// excluding known Jinja function names that are registered in the environment.
    ///
    /// This filters out functions like `compiler`, `pin_subpackage`, etc. which
    /// are looked up in the context first (appearing as "accessed"), but are
    /// actually resolved as functions from the environment.
    pub fn accessed_variables_excluding_functions(&self) -> HashSet<String> {
        self.accessed_variables
            .lock()
            .map(|accessed| {
                accessed
                    .iter()
                    .filter(|name| !KNOWN_FUNCTIONS.contains(&name.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get the set of variables that were accessed but undefined
    pub fn undefined_variables(&self) -> HashSet<String> {
        self.undefined_variables
            .lock()
            .map(|undefined| undefined.clone())
            .unwrap_or_default()
    }

    /// Get the set of variables that were accessed but undefined,
    /// excluding known Jinja function names that are registered in the environment.
    ///
    /// This is useful for error reporting because functions like `compiler`, `pin_subpackage`,
    /// etc. are looked up in the context first (where they appear as "undefined"), but are
    /// then resolved from the environment.
    pub fn undefined_variables_excluding_functions(&self) -> HashSet<String> {
        self.undefined_variables
            .lock()
            .map(|undefined| {
                undefined
                    .iter()
                    .filter(|name| !KNOWN_FUNCTIONS.contains(&name.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clear the accessed and undefined variables trackers
    pub fn clear_tracking(&self) {
        if let Ok(mut accessed) = self.accessed_variables.lock() {
            accessed.clear();
        }
        if let Ok(mut undefined) = self.undefined_variables.lock() {
            undefined.clear();
        }
    }
}

impl Extend<(String, Value)> for Jinja {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, iter: T) {
        self.context.extend(iter);
    }
}

fn jinja_pin_function(
    name: String,
    kwargs: Kwargs,
    internal_repr: InternalRepr,
) -> Result<String, minijinja::Error> {
    let name = PackageName::try_from(name).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::SyntaxError,
            format!("Invalid package name in pin_subpackage: {}", e),
        )
    })?;

    let mut pin = Pin {
        name,
        args: PinArgs::default(),
    };

    if let Ok(exact) = kwargs.get::<bool>("exact") {
        pin.args.exact = exact;
        // No more arguments should be accepted if `exact` is set
        kwargs.assert_all_used()?;
        pin.args.lower_bound = None;
        pin.args.upper_bound = None;
    }

    if let Ok(lower_bound) = kwargs.get::<Option<String>>("lower_bound") {
        if let Some(lower_bound) = lower_bound {
            pin.args.lower_bound = Some(lower_bound.parse().map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::SyntaxError,
                    format!("Invalid lower bound: {}", e),
                )
            })?);
        } else if kwargs.has("lower_bound") {
            pin.args.lower_bound = None;
        }
    }
    if let Ok(upper_bound) = kwargs.get::<Option<String>>("upper_bound") {
        if let Some(upper_bound) = upper_bound {
            pin.args.upper_bound = Some(upper_bound.parse().map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::SyntaxError,
                    format!("Invalid upper bound: {}", e),
                )
            })?);
        } else if kwargs.has("upper_bound") {
            pin.args.upper_bound = None;
        }
    }

    if let Ok(min_pin) = kwargs.get::<String>("min_pin") {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::SyntaxError,
            format!(
                "`min_pin` is not supported anymore. Please use `lower_bound='{}'` instead.",
                min_pin
            ),
        ));
    }

    if let Ok(max_pin) = kwargs.get::<String>("max_pin") {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::SyntaxError,
            format!(
                "`max_pin` is not supported anymore. Please use `upper_bound='{}'` instead.",
                max_pin
            ),
        ));
    }

    if let Ok(build) = kwargs.get::<String>("build") {
        // if exact & build are set this is an error
        if pin.args.exact {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::SyntaxError,
                "Cannot set `exact` and `build` at the same time.".to_string(),
            ));
        }
        pin.args.build = Some(build);
    }

    kwargs.assert_all_used()?;

    Ok(internal_repr.to_json(&pin))
}

fn default_compiler(platform: Platform, language: &str) -> Option<Variable> {
    Some(
        match language {
            // Platform agnostic compilers
            "fortran" => "gfortran",
            lang if !["c", "cxx"].contains(&lang) => lang,
            // Platform specific compilers
            _ => {
                if platform.is_windows() {
                    match language {
                        "c" => "vs2017",
                        "cxx" => "vs2017",
                        _ => unreachable!(),
                    }
                } else if platform.is_osx() {
                    match language {
                        "c" => "clang",
                        "cxx" => "clangxx",
                        _ => unreachable!(),
                    }
                } else if matches!(platform, Platform::EmscriptenWasm32) {
                    match language {
                        "c" => "emscripten",
                        "cxx" => "emscripten",
                        _ => unreachable!(),
                    }
                } else {
                    match language {
                        "c" => "gcc",
                        "cxx" => "gxx",
                        _ => unreachable!(),
                    }
                }
            }
        }
        .into(),
    )
}

fn compiler_stdlib_eval(
    lang: &str,
    platform: Platform,
    variant: &Arc<BTreeMap<NormalizedKey, Variable>>,
    prefix: &str,
    accessed_variables: &Arc<Mutex<HashSet<String>>>,
) -> Result<String, minijinja::Error> {
    let variant_key = NormalizedKey(format!("{lang}_{prefix}")).normalize();
    let variant_key_version = NormalizedKey(format!("{lang}_{prefix}_version")).normalize();

    // Track that we're accessing these variant keys
    if let Ok(mut accessed) = accessed_variables.lock() {
        accessed.insert(variant_key.clone());
        accessed.insert(variant_key_version.clone());
    }

    let default_fn = if prefix == "compiler" {
        default_compiler
    } else {
        |_: Platform, _: &str| None
    };

    let res = if let Some(name) = variant
        .get(&variant_key.into())
        .cloned()
        .or_else(|| default_fn(platform, lang))
    {
        // check if we also have a compiler version
        let name = name.to_string();
        if let Some(version) = variant.get(&variant_key_version.into()) {
            let version = version.to_string();
            if version.chars().all(|a| a.is_alphanumeric() || a == '.') {
                Some(format!("{name}_{platform} ={version}"))
            } else {
                Some(format!("{name}_{platform} {version}"))
            }
        } else {
            Some(format!("{name}_{platform}"))
        }
    } else {
        None
    };

    if let Some(res) = res {
        Ok(res)
    } else {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::UndefinedError,
            format!(
                "No {prefix} found for language: {lang}\nYou should add `{lang}_{prefix}` to your variant config file.",
            ),
        ))
    }
}

fn default_tests(env: &mut Environment) {
    env.add_test("undefined", minijinja::tests::is_undefined);
    env.add_test("defined", minijinja::tests::is_defined);
    env.add_test("none", minijinja::tests::is_none);
    env.add_test("safe", minijinja::tests::is_safe);
    env.add_test("escaped", minijinja::tests::is_safe);
    env.add_test("odd", minijinja::tests::is_odd);
    env.add_test("even", minijinja::tests::is_even);
    env.add_test("number", minijinja::tests::is_number);
    env.add_test("integer", minijinja::tests::is_integer);
    env.add_test("int", minijinja::tests::is_integer);
    env.add_test("float", minijinja::tests::is_float);
    env.add_test("string", minijinja::tests::is_string);
    env.add_test("sequence", minijinja::tests::is_sequence);
    env.add_test("boolean", minijinja::tests::is_boolean);
    env.add_test("startingwith", minijinja::tests::is_startingwith);
    env.add_test("endingwith", minijinja::tests::is_endingwith);

    // operators
    env.add_test("eq", minijinja::tests::is_eq);
    env.add_test("==", minijinja::tests::is_eq);

    env.add_test("ne", minijinja::tests::is_ne);
    env.add_test("!=", minijinja::tests::is_ne);

    env.add_test("lt", minijinja::tests::is_lt);
    env.add_test("<", minijinja::tests::is_lt);

    env.add_test("le", minijinja::tests::is_le);
    env.add_test("<=", minijinja::tests::is_le);

    env.add_test("gt", minijinja::tests::is_gt);
    env.add_test(">", minijinja::tests::is_gt);

    env.add_test("ge", minijinja::tests::is_ge);
    env.add_test(">=", minijinja::tests::is_ge);

    env.add_test("in", minijinja::tests::is_in);
    env.add_test("true", minijinja::tests::is_true);
    env.add_test("false", minijinja::tests::is_false);
}

fn default_filters(env: &mut Environment) {
    env.add_filter("version_to_buildstring", |s: String| {
        // we first split the string by whitespace and take the first part
        let s = s.split_whitespace().next().unwrap_or(&s);
        // we then split the string by . and take the first two parts
        let mut parts = s.split('.');
        let major = parts.next().unwrap_or("");
        let minor = parts.next().unwrap_or("");
        format!("{}{}", major, minor)
    });

    env.add_filter("replace", minijinja::filters::replace);
    env.add_filter("lower", minijinja::filters::lower);
    env.add_filter("upper", minijinja::filters::upper);
    env.add_filter("int", minijinja::filters::int);
    env.add_filter("abs", minijinja::filters::abs);
    env.add_filter("bool", minijinja::filters::bool);
    env.add_filter("default", minijinja::filters::default);
    env.add_filter("first", minijinja::filters::first);
    env.add_filter("last", minijinja::filters::last);
    env.add_filter("length", minijinja::filters::length);
    env.add_filter("list", minijinja::filters::list);
    env.add_filter("join", minijinja::filters::join);
    env.add_filter("min", minijinja::filters::min);
    env.add_filter("max", minijinja::filters::max);
    env.add_filter("reverse", minijinja::filters::reverse);
    env.add_filter("sort", minijinja::filters::sort);
    env.add_filter("trim", minijinja::filters::trim);
    env.add_filter("unique", minijinja::filters::unique);
    env.add_filter("split", minijinja::filters::split);
}

fn parse_platform(platform: &str) -> Result<Platform, minijinja::Error> {
    Platform::from_str(platform).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("Invalid platform: {e}"),
        )
    })
}

lazy_static::lazy_static! {
    /// The syntax config for MiniJinja / rattler-build
    pub static ref SYNTAX_CONFIG: SyntaxConfig = SyntaxConfig::builder()
        .block_delimiters("{%", "%}")
        .variable_delimiters("${{", "}}")
        .comment_delimiters("#{{", "}}")
        .build()
        .unwrap();
}

fn set_jinja(
    config: &JinjaConfig,
    accessed_variables: Arc<Mutex<HashSet<String>>>,
) -> minijinja::Environment<'static> {
    let JinjaConfig {
        target_platform,
        host_platform,
        build_platform,
        variant,
        experimental,
        recipe_path,
        undefined_behavior,
    } = config.clone();

    let mut env = Environment::empty();
    env.set_undefined_behavior(undefined_behavior);
    default_tests(&mut env);
    default_filters(&mut env);

    // Ok to unwrap here because we know that the syntax is valid
    env.set_syntax(SYNTAX_CONFIG.clone());

    let variant = Arc::new(variant.clone());

    // Deprecated function
    env.add_function(
        "cmp",
        |_: &Value, _: &Value| -> Result<(), minijinja::Error> {
            Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "`cmp` is not supported anymore. Please use `match` instead.",
            ))
        },
    );

    env.add_function("match", |a: &Value, spec: &str| {
        if let Some(variant) = a.as_str() {
            // check if version matches spec
            let (version, _) = variant.split_once(' ').unwrap_or((variant, ""));
            // remove trailing .* or *
            let version = version.trim_end_matches(".*").trim_end_matches('*');

            let version = Version::from_str(version).map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::CannotDeserialize,
                    format!("Failed to deserialize `version`: {}", e),
                )
            })?;
            let version_spec =
                VersionSpec::from_str(spec, ParseStrictness::Strict).map_err(|e| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::SyntaxError,
                        format!("Bad syntax for `spec`: {}", e),
                    )
                })?;
            Ok(version_spec.matches(&version))
        } else {
            // if a is undefined, we are currently searching for all variants and thus return true
            Ok(true)
        }
    });

    let variant_clone = variant.clone();
    let accessed_clone = accessed_variables.clone();
    env.add_function("cdt", move |package_name: String| {
        let arch = host_platform.arch().or_else(|| build_platform.arch());
        let arch_str = arch.map(|arch| format!("{arch}"));

        // Track access to cdt_arch if it exists in the variant
        let cdt_arch = if let Some(s) = variant_clone.get(&"cdt_arch".into()) {
            if let Ok(mut accessed) = accessed_clone.lock() {
                accessed.insert("cdt_arch".to_string());
            }
            s.to_string()
        } else {
            match arch {
                Some(Arch::X86) => "i686".to_string(),
                _ => arch_str
                    .as_ref()
                    .ok_or_else(|| {
                        minijinja::Error::new(
                            minijinja::ErrorKind::UndefinedError,
                            "No target or build architecture provided.",
                        )
                    })?
                    .as_str()
                    .to_string(),
            }
        };

        // Track access to cdt_name if it exists in the variant
        let cdt_name = if let Some(s) = variant_clone.get(&"cdt_name".into()) {
            if let Ok(mut accessed) = accessed_clone.lock() {
                accessed.insert("cdt_name".to_string());
            }
            s.to_string()
        } else {
            match arch {
                Some(Arch::S390X | Arch::Aarch64 | Arch::Ppc64le | Arch::Ppc64) => {
                    "cos7".to_string()
                }
                _ => "cos6".to_string(),
            }
        };

        let res = package_name.split_once(' ').map_or_else(
            || format!("{package_name}-{cdt_name}-{cdt_arch}"),
            |(name, ver_build)| format!("{name}-{cdt_name}-{cdt_arch} {ver_build}"),
        );

        Ok(res)
    });

    let variant_clone = variant.clone();
    let accessed_clone = accessed_variables.clone();
    env.add_function("compiler", move |lang: String| {
        compiler_stdlib_eval(
            &lang,
            target_platform,
            &variant_clone,
            "compiler",
            &accessed_clone,
        )
    });

    let variant_clone = variant.clone();
    let accessed_clone = accessed_variables.clone();
    let allow_undefined = !matches!(
        config.undefined_behavior,
        UndefinedBehavior::Strict | UndefinedBehavior::SemiStrict
    );
    env.add_function("stdlib", move |lang: String| {
        let res = compiler_stdlib_eval(
            &lang,
            target_platform,
            &variant_clone,
            "stdlib",
            &accessed_clone,
        );
        if allow_undefined {
            Ok(res.unwrap_or_else(|_| "undefined".to_string()))
        } else {
            res
        }
    });

    env.add_function("pin_subpackage", |name: String, kwargs: Kwargs| {
        jinja_pin_function(name, kwargs, InternalRepr::PinSubpackage)
    });

    env.add_function("pin_compatible", |name: String, kwargs: Kwargs| {
        jinja_pin_function(name, kwargs, InternalRepr::PinCompatible)
    });

    // Add the is_... functions
    env.add_function("is_linux", |platform: &str| {
        Ok(parse_platform(platform)?.is_linux())
    });
    env.add_function("is_osx", |platform: &str| {
        Ok(parse_platform(platform)?.is_osx())
    });
    env.add_function("is_windows", |platform: &str| {
        Ok(parse_platform(platform)?.is_windows())
    });
    env.add_function("is_unix", |platform: &str| {
        Ok(parse_platform(platform)?.is_unix())
    });

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    enum FileFormat {
        Yaml,
        Json,
        Toml,
        Unknown,
    }

    // Helper function to determine the file format based on the file extension
    fn get_file_format(file_path: &std::path::Path) -> FileFormat {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext.to_lowercase().as_str() {
                "yaml" | "yml" => FileFormat::Yaml,
                "json" => FileFormat::Json,
                "toml" => FileFormat::Toml,
                _ => FileFormat::Unknown,
            })
            .unwrap_or(FileFormat::Unknown)
    }

    // Helper function to handle file errors
    fn handle_file_error(e: std::io::Error, file_path: &std::path::Path) -> minijinja::Error {
        minijinja::Error::new(
            minijinja::ErrorKind::UndefinedError,
            format!("Failed to open file '{}': {}", file_path.display(), e),
        )
    }

    // Helper function to handle file reading errors
    fn handle_read_error(e: std::io::Error, file_path: &std::path::Path) -> minijinja::Error {
        minijinja::Error::new(
            minijinja::ErrorKind::UndefinedError,
            format!("Failed to read file '{}': {}", file_path.display(), e),
        )
    }

    // Helper function to handle deserialization errors
    fn handle_deserialize_error(e: impl std::fmt::Display) -> minijinja::Error {
        minijinja::Error::new(minijinja::ErrorKind::CannotDeserialize, e.to_string())
    }

    // Read and parse the file based on its format
    fn read_and_parse_file(
        file_path: &std::path::Path,
    ) -> Result<minijinja::Value, minijinja::Error> {
        let file = fs::File::open(file_path).map_err(|e| handle_file_error(e, file_path))?;
        let mut reader = std::io::BufReader::new(file);

        match get_file_format(file_path) {
            FileFormat::Yaml => serde_yaml::from_reader(reader).map_err(handle_deserialize_error),
            FileFormat::Json => serde_json::from_reader(reader).map_err(handle_deserialize_error),
            FileFormat::Toml => {
                let mut content = String::new();
                reader
                    .read_to_string(&mut content)
                    .map_err(|e| handle_read_error(e, file_path))?;
                toml::from_str(&content).map_err(handle_deserialize_error)
            }
            FileFormat::Unknown => {
                let mut content = String::new();
                reader
                    .read_to_string(&mut content)
                    .map_err(|e| handle_read_error(e, file_path))?;
                Ok(Value::from(content))
            }
        }
    }
    // Check if the experimental feature is enabled
    fn check_experimental(experimental: bool) -> Result<(), minijinja::Error> {
        if !experimental {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "Experimental feature: provide the `--experimental` flag to enable this feature",
            ));
        }
        Ok(())
    }

    env.add_function("load_from_file", move |path: String| {
        check_experimental(experimental)?;

        if let Some(recipe_path) = recipe_path.as_ref()
            && let Some(parent) = recipe_path.parent()
        {
            let relative_path = parent.join(&path);
            if let Ok(value) = read_and_parse_file(&relative_path) {
                return Ok(value);
            }
        }

        let file_path = std::path::Path::new(&path);
        read_and_parse_file(file_path)
    });

    // Add env object
    env.add_global("env", Value::from_object(crate::env::Env));

    // Add git object (experimental)
    env.add_global("git", Value::from_object(crate::git::Git { experimental }));

    env
}

#[cfg(test)]
mod tests {
    // git version is too old in cross container for aarch64
    use fs_err as fs;
    use rattler_conda_types::Platform;
    #[cfg(not(all(
        any(target_arch = "aarch64", target_arch = "powerpc64"),
        target_os = "linux"
    )))]
    use std::path::Path;
    #[cfg(not(all(
        any(target_arch = "aarch64", target_arch = "powerpc64"),
        target_os = "linux"
    )))]
    use std::process::Command;

    use crate::utils::to_forward_slash_lossy;

    use super::*;

    // git version is too old in cross container for aarch64
    #[cfg(not(all(
        any(target_arch = "aarch64", target_arch = "powerpc64"),
        target_os = "linux"
    )))]
    fn with_temp_dir(key: &'static str, f: impl Fn(&std::path::Path)) {
        let tempdir = tempfile::tempdir().unwrap();
        let dir = tempdir.path().join(key);
        fs::create_dir_all(&dir).unwrap();
        f(&dir);
        fs::remove_dir_all(dir).unwrap();
    }

    // git version is too old in cross container for aarch64
    #[cfg(not(all(
        any(target_arch = "aarch64", target_arch = "powerpc64"),
        target_os = "linux"
    )))]
    fn git_setup(path: &Path) -> anyhow::Result<()> {
        let git_config = r#"
[user]
	name = John Doe
	email = johndoe@example.ne
"#;
        fs::write(path.join(".git/config"), git_config)?;
        Ok(())
    }

    // git version is too old in cross container for aarch64
    #[cfg(not(all(
        any(target_arch = "aarch64", target_arch = "powerpc64"),
        target_os = "linux"
    )))]
    fn create_repo_with_tag(path: impl AsRef<Path>, tag: impl AsRef<str>) -> anyhow::Result<()> {
        let git_with_args = |arg: &str, args: &[&str]| -> anyhow::Result<bool> {
            Ok(Command::new("git")
                .current_dir(&path)
                .arg(arg)
                .args(args)
                // .stderr(std::process::Stdio::inherit())
                // .stdout(std::process::Stdio::inherit())
                .output()?
                .status
                .success())
        };
        if git_with_args("init", &[])? {
            git_setup(path.as_ref())?;
            fs::write(path.as_ref().join("README.md"), "init")?;
            let git_add = git_with_args("add", &["."])?;
            let commit_created = git_with_args("commit", &["-m", "init", "--no-gpg-sign"])?;
            let tag_created = git_with_args("tag", &[tag.as_ref()])?;
            if !git_add || !commit_created || !tag_created {
                anyhow::bail!("failed to create add, commit or tag");
            }
        } else {
            anyhow::bail!("failed to create git repo");
        }
        Ok(())
    }

    #[test]
    #[rustfmt::skip]
    // git version is too old in cross container for aarch64
    #[cfg(not(all(any(target_arch = "aarch64", target_arch = "powerpc64"), target_os = "linux")))]
    fn eval_git() {
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            experimental: true,
            ..Default::default()
        };
        let options_wo_experimental = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            ..Default::default()
        };

        let jinja = Jinja::new(options);
        let jinja_wo_experimental = Jinja::new(options_wo_experimental);

        with_temp_dir("rattler_build_recipe_jinja_eval_git", |path| {
            create_repo_with_tag(path, "v0.1.0").expect("Failed to clone the git repo");
            assert_eq!(jinja.eval(&format!("git.latest_tag({:?})", path)).expect("test 0").as_str().unwrap(), "v0.1.0");
            assert_eq!(jinja.eval(&format!("git.latest_tag_rev({:?})", path)).expect("test 1 left").as_str().unwrap(), jinja.eval(&format!("git.head_rev({:?})", path)).expect("test 1 right").as_str().unwrap());
            assert_eq!(
                jinja_wo_experimental.eval(&format!("git.latest_tag({:?})", path)).expect_err("test 2").to_string(),
                "invalid operation: Experimental feature: provide the `--experimental` flag to enable this feature (in <expression>:1)",
            );
        });
    }

    #[test]
    #[rustfmt::skip]
    fn eval_load_from_file() {
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            experimental: true,
            ..Default::default()
        };

        let jinja = Jinja::new(options);

        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.json");
        let path_str = to_forward_slash_lossy(&path);
        fs::write(&path, "{ \"hello\": \"world\" }").unwrap();
        assert_eq!(
            jinja.eval(&format!("load_from_file('{}')['hello']", path_str)).expect("test 1").as_str(),
            Some("world"),
        );

        let path = temp_dir.path().join("test.yaml");
        fs::write(&path, "hello: world").unwrap();
        let path_str = to_forward_slash_lossy(&path);
        assert_eq!(
            jinja.eval(&format!("load_from_file('{}')['hello']", path_str)).expect("test 2").as_str(),
            Some("world"),
        );

        let path = temp_dir.path().join("test.toml");
        let path_str = to_forward_slash_lossy(&path);
        fs::write(&path, "hello = 'world'").unwrap();
        assert_eq!(
            jinja.eval(&format!("load_from_file('{}')['hello']", path_str)).expect("test 3").as_str(),
            Some("world"),
        );
    }

    #[test]
    fn eval_load_from_file_relative_to_recipe() {
        let temp_dir = tempfile::tempdir().unwrap();
        let recipe_path = temp_dir.path().join("recipe.yaml");
        fs::write(&recipe_path, "dummy: content").unwrap();

        let data_dir = temp_dir.path().join("data");
        fs::create_dir(&data_dir).unwrap();
        let json_path = data_dir.join("test.json");
        fs::write(&json_path, "{ \"hello\": \"world\" }").unwrap();

        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            experimental: true,
            recipe_path: Some(recipe_path),
            ..Default::default()
        };

        let jinja = Jinja::new(options);

        assert_eq!(
            jinja
                .eval("load_from_file('data/test.json')['hello']")
                .expect("test relative path")
                .as_str(),
            Some("world"),
        );
    }

    #[test]
    fn eval_load_from_file_relative_to_recipe_without_experimental() {
        let temp_dir = tempfile::tempdir().unwrap();
        let recipe_path = temp_dir.path().join("recipe.yaml");
        fs::write(&recipe_path, "dummy: content").unwrap();

        let data_dir = temp_dir.path().join("data");
        fs::create_dir(&data_dir).unwrap();
        let json_path = data_dir.join("test.json");
        fs::write(&json_path, "{ \"hello\": \"world\" }").unwrap();

        let options_wo_experimental = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            experimental: false,
            recipe_path: Some(recipe_path),
            ..Default::default()
        };

        let jinja_wo_experimental = Jinja::new(options_wo_experimental);

        assert_eq!(
            jinja_wo_experimental
                .eval("load_from_file('data/test.json')")
                .expect_err("should error without experimental flag")
                .to_string(),
            "invalid operation: Experimental feature: provide the `--experimental` flag to enable this feature (in <expression>:1)",
        );
    }

    #[test]
    #[rustfmt::skip]
    fn eval() {
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        let jinja = Jinja::new(options);

        assert!(jinja.eval("unix").expect("test 1").is_true());
        assert!(!jinja.eval("win").expect("test 2").is_true());
        assert!(!jinja.eval("osx").expect("test 3").is_true());
        assert!(jinja.eval("linux").expect("test 4").is_true());
        assert!(jinja.eval("unix and not win").expect("test 5").is_true());
        assert!(!jinja.eval("unix and not linux").expect("test 6").is_true());
        assert!(jinja.eval("(unix and not osx) or win").expect("test 7").is_true());
        assert!(jinja.eval("(unix and not osx) or win or osx").expect("test 8").is_true());
        assert!(jinja.eval("linux and x86_64").expect("test 9").is_true());
        assert!(!jinja.eval("linux and aarch64").expect("test 10").is_true());
    }

    #[test]
    #[should_panic]
    #[rustfmt::skip]
    fn eval2() {
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            ..Default::default()
        };

        let jinja = Jinja::new(options);

        assert!(jinja.eval("${{ true if win }}").expect("test 1").is_true());
    }

    #[test]
    fn eval_cdt_x86_64() {
        let variant = BTreeMap::new();
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        assert_eq!(
            jinja
                .eval("cdt('package_name v0.1.2')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos6-x86_64 v0.1.2"
        );
        assert_eq!(
            jinja
                .eval("cdt('package_name')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos6-x86_64"
        );
    }

    #[test]
    fn eval_cdt_x86() {
        let variant = BTreeMap::new();
        let options = JinjaConfig {
            target_platform: Platform::Linux32,
            host_platform: Platform::Linux32,
            build_platform: Platform::Linux32,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        assert_eq!(
            jinja
                .eval("cdt('package_name v0.1.2')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos6-i686 v0.1.2"
        );
        assert_eq!(
            jinja
                .eval("cdt('package_name')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos6-i686"
        );
    }

    #[test]
    fn eval_cdt_aarch64() {
        let variant = BTreeMap::new();
        let options = JinjaConfig {
            target_platform: Platform::LinuxAarch64,
            host_platform: Platform::LinuxAarch64,
            build_platform: Platform::LinuxAarch64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        assert_eq!(
            jinja
                .eval("cdt('package_name v0.1.2')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos7-aarch64 v0.1.2"
        );
        assert_eq!(
            jinja
                .eval("cdt('package_name')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos7-aarch64"
        );
    }

    #[test]
    fn eval_cdt_arm6() {
        let variant = BTreeMap::new();
        let options = JinjaConfig {
            target_platform: Platform::LinuxArmV6l,
            host_platform: Platform::LinuxArmV6l,
            build_platform: Platform::LinuxArmV6l,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        assert_eq!(
            jinja
                .eval("cdt('package_name v0.1.2')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos6-armv6l v0.1.2"
        );
        assert_eq!(
            jinja
                .eval("cdt('package_name')")
                .expect("test 1")
                .to_string()
                .as_str(),
            "package_name-cos6-armv6l"
        );
    }

    #[test]
    #[rustfmt::skip]
    fn eval_match() {
        let variant = BTreeMap::from_iter(vec![("python".into(), "3.7".into())]);

        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        assert!(jinja.eval("match(python, '==3.7')").expect("test 1").is_true());
        assert!(jinja.eval("match(python, '>=3.7')").expect("test 2").is_true());
        assert!(jinja.eval("match(python, '>=3.7,<3.9')").expect("test 3").is_true());

        assert!(!jinja.eval("match(python, '!=3.7')").expect("test 4").is_true());
        assert!(!jinja.eval("match(python, '<3.7')").expect("test 5").is_true());
        assert!(!jinja.eval("match(python, '>3.5,<3.7')").expect("test 6").is_true());
    }

    #[test]
    #[rustfmt::skip]
    fn eval_complicated_match() {
        let variant = BTreeMap::from_iter(vec![("python".into(), "3.7.* *_cpython".into())]);

        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        assert!(jinja.eval("match(python, '==3.7')").expect("test 1").is_true());
        assert!(jinja.eval("match(python, '>=3.7')").expect("test 2").is_true());
        assert!(jinja.eval("match(python, '>=3.7,<3.9')").expect("test 3").is_true());

        assert!(!jinja.eval("match(python, '!=3.7')").expect("test 4").is_true());
        assert!(!jinja.eval("match(python, '<3.7')").expect("test 5").is_true());
        assert!(!jinja.eval("match(python, '>3.5,<3.7')").expect("test 6").is_true());
    }

    fn with_env((key, value): (impl AsRef<str>, impl AsRef<str>), f: impl Fn()) {
        if let Ok(old_value) = std::env::var(key.as_ref()) {
            unsafe {
                std::env::set_var(key.as_ref(), value.as_ref());
            }
            f();
            unsafe {
                std::env::set_var(key.as_ref(), old_value);
            }
        } else {
            unsafe {
                std::env::set_var(key.as_ref(), value.as_ref());
            }
            f();
            unsafe {
                std::env::remove_var(key.as_ref());
            }
        }
    }
    #[test]
    fn eval_pin_subpackage() {
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            ..Default::default()
        };

        let jinja = Jinja::new(options);
        let ps = |s| {
            jinja
                .eval(&format!("pin_subpackage(\"foo\", {})", s))
                .unwrap()
                .to_string()
        };

        assert_eq!(
            ps(""),
            "{\"pin_subpackage\":{\"name\":\"foo\",\"lower_bound\":\"x.x.x.x.x.x\",\"upper_bound\":\"x\"}}"
        );
        assert_eq!(
            ps("upper_bound=None"),
            "{\"pin_subpackage\":{\"name\":\"foo\",\"lower_bound\":\"x.x.x.x.x.x\"}}"
        );
        assert_eq!(
            ps("upper_bound=None, lower_bound=None"),
            "{\"pin_subpackage\":{\"name\":\"foo\"}}"
        );
        assert_eq!(
            ps("lower_bound='1.2.3'"),
            "{\"pin_subpackage\":{\"name\":\"foo\",\"lower_bound\":\"1.2.3\",\"upper_bound\":\"x\"}}"
        );
    }

    #[test]
    fn test_split() {
        let options = JinjaConfig::default();

        let mut jinja = Jinja::new(options);
        let mut split_test = |s: &str, sep: Option<&str>| {
            jinja
                .context_mut()
                .insert("var".to_string(), Value::from_safe_string(s.to_string()));

            let func = if let Some(sep) = sep {
                format!("split('{}')", sep)
            } else {
                "split".to_string()
            };

            jinja
                .eval(&format!("var | {func} | list"))
                .unwrap()
                .to_string()
        };

        assert_eq!(split_test("foo bar", None), "[\"foo\", \"bar\"]");
        assert_eq!(split_test("foobar", None), "[\"foobar\"]");
        assert_eq!(split_test("1.2.3", Some(".")), "[\"1\", \"2\", \"3\"]");

        jinja.context_mut().insert(
            "var".to_string(),
            Value::from_safe_string("1.2.3".to_string()),
        );

        assert_eq!(
            jinja.eval("(var | split('.'))[2]").unwrap().to_string(),
            "3"
        );
    }

    #[test]
    fn eval_env() {
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        with_env(("RANDOM_JINJA_ENV_VAR", "false"), || {
            assert_eq!(
                jinja
                    .eval("env.get('RANDOM_JINJA_ENV_VAR')")
                    .expect("test 1")
                    .as_str(),
                Some("false")
            );
            assert!(jinja.eval("env.get('RANDOM_JINJA_ENV_VAR2')").is_err());
            assert_eq!(
                jinja
                    .eval("env.get('RANDOM_JINJA_ENV_VAR', default='true')")
                    .expect("test 3")
                    .as_str(),
                Some("false")
            );
            assert_eq!(
                jinja
                    .eval("env.get('RANDOM_JINJA_ENV_VAR2', default='true')")
                    .expect("test 4")
                    .as_str(),
                Some("true")
            );
            assert!(
                jinja
                    .eval("env.exists('RANDOM_JINJA_ENV_VAR')")
                    .expect("test 5")
                    .is_true()
            );
            assert!(
                !jinja
                    .eval("env.exists('RANDOM_JINJA_ENV_VAR2')")
                    .expect("test 6")
                    .is_true()
            );
        });
    }

    #[test]
    fn test_unavailable() {
        let jinja = Jinja::new(Default::default());
        assert!(jinja.eval("cmp(python, '==3.7')").is_err());
        assert!(jinja.eval("${{ \"foo\" | escape }}").is_err());
    }

    #[test]
    fn test_variable_tracking() {
        let mut jinja = Jinja::new(Default::default());
        jinja
            .context_mut()
            .insert("name".to_string(), Value::from("foo"));
        jinja
            .context_mut()
            .insert("version".to_string(), Value::from("1.0.0"));

        // Render a template
        let result = jinja.render_str("${{ name }}-${{ version }}");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "foo-1.0.0");

        // Check accessed variables
        let accessed = jinja.accessed_variables();
        assert_eq!(accessed.len(), 2);
        assert!(accessed.contains("name"));
        assert!(accessed.contains("version"));

        // No undefined variables
        assert_eq!(jinja.undefined_variables().len(), 0);
    }

    #[test]
    fn test_undefined_variable_tracking() {
        let mut jinja = Jinja::new(Default::default());
        jinja
            .context_mut()
            .insert("name".to_string(), Value::from("foo"));
        // Note: "version" is NOT defined

        // Set strict undefined behavior
        jinja
            .env_mut()
            .set_undefined_behavior(minijinja::UndefinedBehavior::Strict);

        // Try to render a template with undefined variable
        let result = jinja.render_str("${{ name }}-${{ version }}");
        assert!(result.is_err());

        // Check that both variables were accessed
        let accessed = jinja.accessed_variables();
        assert_eq!(accessed.len(), 2);
        assert!(accessed.contains("name"));
        assert!(accessed.contains("version"));

        // Check that only "version" is undefined
        let undefined = jinja.undefined_variables();
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains("version"));
    }

    #[test]
    fn test_multiple_undefined_variables_tracking() {
        let jinja = Jinja::new(Default::default());
        // No variables defined

        // Try to render a template with multiple undefined variables
        let result = jinja.render_str("${{ platform }} for ${{ arch }}");
        assert!(result.is_err());

        // Both undefined variables should be tracked
        let undefined = jinja.undefined_variables();
        assert_eq!(undefined.len(), 1);
        assert!(undefined.contains("platform"));
    }

    #[test]
    fn test_clear_tracking() {
        let mut jinja = Jinja::new(Default::default());
        jinja
            .context_mut()
            .insert("name".to_string(), Value::from("foo"));

        // Render a template
        let _ = jinja.render_str("${{ name }}");

        // Variables should be tracked
        assert!(!jinja.accessed_variables().is_empty());

        // Clear tracking
        jinja.clear_tracking();

        // Variables should be cleared
        assert!(jinja.accessed_variables().is_empty());
        assert!(jinja.undefined_variables().is_empty());
    }

    #[test]
    fn test_cdt_tracks_cdt_name_variable() {
        // Test that when cdt() function uses cdt_name from the variant,
        // it tracks the variable access
        let variant = BTreeMap::from_iter(vec![("cdt_name".into(), "conda".into())]);
        let options = JinjaConfig {
            target_platform: Platform::LinuxAarch64,
            host_platform: Platform::LinuxAarch64,
            build_platform: Platform::LinuxAarch64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        // Call cdt() which should use and track cdt_name
        let result = jinja.eval("cdt('mesa-libgbm')").expect("cdt evaluation");
        assert_eq!(result.to_string(), "mesa-libgbm-conda-aarch64");

        // cdt_name should be in the accessed variables
        let accessed = jinja.accessed_variables();
        assert!(
            accessed.contains("cdt_name"),
            "cdt_name should be tracked when used by cdt() function. Accessed: {:?}",
            accessed
        );
    }

    #[test]
    fn test_cdt_tracks_cdt_arch_variable() {
        // Test that when cdt() function uses cdt_arch from the variant,
        // it tracks the variable access
        let variant = BTreeMap::from_iter(vec![("cdt_arch".into(), "custom_arch".into())]);
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        // Call cdt() which should use and track cdt_arch
        let result = jinja.eval("cdt('mesa-libgbm')").expect("cdt evaluation");
        assert_eq!(result.to_string(), "mesa-libgbm-cos6-custom_arch");

        // cdt_arch should be in the accessed variables
        let accessed = jinja.accessed_variables();
        assert!(
            accessed.contains("cdt_arch"),
            "cdt_arch should be tracked when used by cdt() function. Accessed: {:?}",
            accessed
        );
    }

    #[test]
    fn test_cdt_does_not_track_when_using_default() {
        // Test that when cdt() uses default values (no cdt_name in variant),
        // it does NOT track the variable (since it wasn't actually read from the variant)
        let variant = BTreeMap::new();
        let options = JinjaConfig {
            target_platform: Platform::LinuxAarch64,
            host_platform: Platform::LinuxAarch64,
            build_platform: Platform::LinuxAarch64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        // Call cdt() which should use default cdt_name
        let result = jinja.eval("cdt('mesa-libgbm')").expect("cdt evaluation");
        assert_eq!(result.to_string(), "mesa-libgbm-cos7-aarch64");

        // cdt_name should NOT be in the accessed variables since it used the default
        let accessed = jinja.accessed_variables();
        assert!(
            !accessed.contains("cdt_name"),
            "cdt_name should NOT be tracked when using default value. Accessed: {:?}",
            accessed
        );
    }

    #[test]
    fn test_compiler_dash_underscore_variant_tracking() {
        // Test that when compiler("go-cgo") is called, the accessed variables
        // are tracked with underscores (go_cgo_compiler, go_cgo_compiler_version)
        // not dashes (go-cgo_compiler, go-cgo_compiler_version)
        // This is important because variant configs use normalized keys with underscores
        let variant = BTreeMap::from_iter(vec![
            ("go_cgo_compiler".into(), "go_cgo".into()),
            ("go_cgo_compiler_version".into(), "1.24".into()),
        ]);
        let options = JinjaConfig {
            target_platform: Platform::Linux64,
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant,
            ..Default::default()
        };
        let jinja = Jinja::new(options);

        // Call compiler("go-cgo") with a dash
        let result = jinja
            .eval("compiler('go-cgo')")
            .expect("compiler evaluation");
        assert_eq!(result.to_string(), "go_cgo_linux-64 =1.24");

        // The accessed variables should use underscores, not dashes
        let accessed = jinja.accessed_variables();
        assert!(
            accessed.contains("go_cgo_compiler"),
            "go_cgo_compiler should be tracked (with underscore). Accessed: {:?}",
            accessed
        );
        assert!(
            accessed.contains("go_cgo_compiler_version"),
            "go_cgo_compiler_version should be tracked (with underscore). Accessed: {:?}",
            accessed
        );
        // Ensure the dash versions are NOT tracked
        assert!(
            !accessed.contains("go-cgo_compiler"),
            "go-cgo_compiler (with dash) should NOT be tracked. Accessed: {:?}",
            accessed
        );
        assert!(
            !accessed.contains("go-cgo_compiler_version"),
            "go-cgo_compiler_version (with dash) should NOT be tracked. Accessed: {:?}",
            accessed
        );
    }

    #[test]
    fn test_default_compiler() {
        let platform = Platform::Linux64;
        assert_eq!(
            "gxx",
            default_compiler(platform, "cxx").unwrap().to_string()
        );
        assert_eq!(
            "cuda",
            default_compiler(platform, "cuda").unwrap().to_string()
        );
        assert_eq!("gcc", default_compiler(platform, "c").unwrap().to_string());

        let platform = Platform::Linux32;
        assert_eq!(
            "gxx",
            default_compiler(platform, "cxx").unwrap().to_string()
        );
        assert_eq!(
            "cuda",
            default_compiler(platform, "cuda").unwrap().to_string()
        );
        assert_eq!("gcc", default_compiler(platform, "c").unwrap().to_string());

        let platform = Platform::Win64;
        assert_eq!(
            "vs2017",
            default_compiler(platform, "cxx").unwrap().to_string()
        );
        assert_eq!(
            "vs2017",
            default_compiler(platform, "c").unwrap().to_string()
        );
        assert_eq!(
            "cuda",
            default_compiler(platform, "cuda").unwrap().to_string()
        );
    }
}
