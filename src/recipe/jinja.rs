//! Module for types and functions related to miniJinja setup for recipes.

use fs_err as fs;
use indexmap::IndexMap;
use minijinja::syntax::SyntaxConfig;
use std::io::Read;
use std::process::Command;
use std::sync::Arc;
use std::{collections::BTreeMap, str::FromStr};

use minijinja::value::{Kwargs, Object, from_args};
use minijinja::{Environment, Value};
use rattler_conda_types::{Arch, PackageName, ParseStrictness, Platform, Version, VersionSpec};

use crate::normalized_key::NormalizedKey;
use crate::render::pin::PinArgs;
pub use crate::render::pin::{Pin, PinExpression};
pub use crate::selectors::SelectorConfig;

use super::parser::{Dependency, PinCompatible, PinSubpackage};
use super::variable::Variable;

/// The internal representation of the pin function.
pub enum InternalRepr {
    /// The pin function is used to pin a subpackage.
    PinSubpackage,
    /// The pin function is used to pin a compatible package.
    PinCompatible,
}

impl InternalRepr {
    fn to_json(&self, pin: &Pin) -> String {
        match self {
            InternalRepr::PinSubpackage => {
                serde_json::to_string(&Dependency::PinSubpackage(PinSubpackage {
                    pin_subpackage: pin.clone(),
                }))
                .unwrap()
            }
            InternalRepr::PinCompatible => {
                serde_json::to_string(&Dependency::PinCompatible(PinCompatible {
                    pin_compatible: pin.clone(),
                }))
                .unwrap()
            }
        }
    }
}

/// A type that hold the minijinja environment and context for Jinja template processing.
#[derive(Debug, Clone)]
pub struct Jinja {
    env: Environment<'static>,
    context: BTreeMap<String, Value>,
}

impl Jinja {
    /// Create a new Jinja instance with the given selector configuration.
    pub fn new(config: SelectorConfig) -> Self {
        let env = set_jinja(&config);
        let context = config.into_context();
        Self { env, context }
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
    pub fn render_str(&self, template: &str) -> Result<String, minijinja::Error> {
        self.env.render_str(template, &self.context)
    }

    /// Render, compile and evaluate a expr string with the current context.
    pub fn eval(&self, str: &str) -> Result<Value, minijinja::Error> {
        let expr = self.env.compile_expression(str)?;
        expr.eval(self.context())
    }
}

impl Default for Jinja {
    fn default() -> Self {
        Self {
            env: set_jinja(&SelectorConfig::default()),
            context: BTreeMap::new(),
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
) -> Result<String, minijinja::Error> {
    let variant_key = format!("{lang}_{prefix}");
    let variant_key_version = format!("{lang}_{prefix}_version");

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

fn set_jinja(config: &SelectorConfig) -> minijinja::Environment<'static> {
    let SelectorConfig {
        target_platform,
        host_platform,
        build_platform,
        variant,
        experimental,
        allow_undefined,
        recipe_path,
        ..
    } = config.clone();

    let mut env = Environment::empty();
    // env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
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
    env.add_function("cdt", move |package_name: String| {
        let arch = host_platform.arch().or_else(|| build_platform.arch());
        let arch_str = arch.map(|arch| format!("{arch}"));

        let cdt_arch = if let Some(s) = variant_clone.get(&"cdt_arch".into()) {
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

        let cdt_name = variant_clone.get(&"cdt_name".into()).map_or_else(
            || match arch {
                Some(Arch::S390X | Arch::Aarch64 | Arch::Ppc64le | Arch::Ppc64) => {
                    "cos7".to_string()
                }
                _ => "cos6".to_string(),
            },
            |s| s.to_string(),
        );

        let res = package_name.split_once(' ').map_or_else(
            || format!("{package_name}-{cdt_name}-{cdt_arch}"),
            |(name, ver_build)| format!("{name}-{cdt_name}-{cdt_arch} {ver_build}"),
        );

        Ok(res)
    });

    // "${{ PREFIX }}" delay the expansion. -> $PREFIX on unix and %PREFIX% on windows?
    let variant_clone = variant.clone();
    env.add_function("compiler", move |lang: String| {
        compiler_stdlib_eval(&lang, target_platform, &variant_clone, "compiler")
    });

    let variant_clone = variant.clone();
    env.add_function("stdlib", move |lang: String| {
        let res = compiler_stdlib_eval(&lang, target_platform, &variant_clone, "stdlib");
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

        if let Some(recipe_path) = recipe_path.as_ref() {
            if let Some(parent) = recipe_path.parent() {
                let relative_path = parent.join(&path);
                if let Ok(value) = read_and_parse_file(&relative_path) {
                    return Ok(value);
                }
            }
        }

        let file_path = std::path::Path::new(&path);
        read_and_parse_file(file_path)
    });

    env
}

#[derive(Debug)]
pub(crate) struct Git {
    pub(crate) experimental: bool,
}

impl std::fmt::Display for Git {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Git")
    }
}

fn get_command_output(command: &str, args: &[&str]) -> Result<String, minijinja::Error> {
    let output = Command::new(command).args(args).output().map_err(|e| {
        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
    })?;

    if !output.status.success() {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    } else {
        Ok(String::from_utf8(output.stdout).map_err(|e| {
            minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
        })?)
    }
}

impl Git {
    fn head_rev(&self, src: &str) -> Result<Value, minijinja::Error> {
        let result = get_command_output("git", &["ls-remote", src, "HEAD"])?
            .lines()
            .next()
            .and_then(|s| s.split_ascii_whitespace().nth(0))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "Failed to get the HEAD".to_string(),
                )
            })?
            .to_string();
        Ok(Value::from(result))
    }

    fn latest_tag_rev(&self, src: &str) -> Result<Value, minijinja::Error> {
        let result = get_command_output("git", &["ls-remote", "--tags", "--sort=-v:refname", src])?
            .lines()
            .next()
            .and_then(|s| s.split_ascii_whitespace().nth(0))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "Failed to get the latest tag".to_string(),
                )
            })?
            .to_string();
        Ok(Value::from(result))
    }

    fn latest_tag(&self, src: &str) -> Result<Value, minijinja::Error> {
        let result = get_command_output("git", &["ls-remote", "--tags", "--sort=-v:refname", src])?
            .lines()
            .next()
            .and_then(|s| s.split_ascii_whitespace().nth(1))
            .and_then(|s| s.strip_prefix("refs/tags/"))
            .map(|s| s.trim_end_matches("^{}"))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "Failed to get the latest tag".to_string(),
                )
            })?
            .to_string();
        Ok(Value::from(result))
    }
}

impl Object for Git {
    fn call_method(
        self: &Arc<Self>,
        _state: &minijinja::State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        if !self.experimental {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "Experimental feature: provide the `--experimental` flag to enable this feature",
            ));
        }
        let (src,) = from_args(args)?;
        match name {
            "head_rev" => self.head_rev(src),
            "latest_tag_rev" => self.latest_tag_rev(src),
            "latest_tag" => self.latest_tag(src),
            name => Err(minijinja::Error::new(
                minijinja::ErrorKind::UnknownMethod,
                format!("object has no method named {name}"),
            )),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Env;
impl std::fmt::Display for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Env")
    }
}

impl Env {
    fn get(&self, env_var: &str, kwargs: Kwargs) -> Result<Value, minijinja::Error> {
        let default_value = kwargs.get::<String>("default").ok();
        kwargs.assert_all_used()?;

        match std::env::var(env_var) {
            Ok(r) => Ok(Value::from(r)),
            Err(_) => match default_value {
                Some(default_value) => Ok(Value::from(default_value)),
                None => Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("Environment variable {env_var} not found"),
                )),
            },
        }
    }

    fn exists(&self, env_var: &str) -> Result<Value, minijinja::Error> {
        Ok(Value::from(std::env::var(env_var).is_ok()))
    }
}

impl Object for Env {
    fn call_method(
        self: &Arc<Self>,
        _state: &minijinja::State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        match name {
            "get" => {
                let (args, kwargs) = from_args(args)?;
                self.get(args, kwargs)
            }
            "exists" => {
                let name: (&str,) = from_args(args)?;
                self.exists(name.0)
            }
            _ => Err(minijinja::Error::from(minijinja::ErrorKind::UnknownMethod)),
        }
    }
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
        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            experimental: true,
            ..Default::default()
        };
        let options_wo_experimental = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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
        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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

        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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

        let options_wo_experimental = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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
        let options = SelectorConfig {
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
        let options = SelectorConfig {
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
        let options = SelectorConfig {
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
        let options = SelectorConfig {
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
        let options = SelectorConfig {
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
        let options = SelectorConfig {
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

        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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

        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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
        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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
        let options = SelectorConfig::default();

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
        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
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
