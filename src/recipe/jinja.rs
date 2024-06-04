//! Module for types and functions related to miniJinja setup for recipes.

use std::process::Command;
use std::sync::Arc;
use std::{collections::BTreeMap, str::FromStr};

use minijinja::value::{Kwargs, Object};
use minijinja::{Environment, Value};
use rattler_conda_types::{PackageName, ParseStrictness, Platform, Version, VersionSpec};

use crate::render::pin::PinArgs;
pub use crate::render::pin::{Pin, PinExpression};
pub use crate::selectors::SelectorConfig;

use super::parser::{Dependency, PinCompatible, PinSubpackage};

/// A type that hold the miniJinja environment and context for Jinja template processing.
#[derive(Debug, Clone)]
pub struct Jinja<'a> {
    env: Environment<'a>,
    context: BTreeMap<String, Value>,
}

impl<'a> Jinja<'a> {
    /// Create a new Jinja instance with the given selector configuration.
    pub fn new(config: SelectorConfig) -> Self {
        let env = set_jinja(&config);
        let context = config.into_context();
        Self { env, context }
    }

    /// Get a reference to the miniJinja environment.
    pub fn env(&self) -> &Environment<'a> {
        &self.env
    }

    /// Get a mutable reference to the miniJinja environment.
    ///
    /// This is useful for adding custom functions to the environment.
    pub fn env_mut(&mut self) -> &mut Environment<'a> {
        &mut self.env
    }

    /// Get a reference to the miniJinja context.
    pub fn context(&self) -> &BTreeMap<String, Value> {
        &self.context
    }

    /// Get a mutable reference to the miniJinja context.
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
        let expr = self.render_str(str)?;
        if expr.is_empty() {
            return Ok(Value::UNDEFINED);
        }
        let expr = self.env.compile_expression(&expr)?;
        expr.eval(self.context())
    }
}

impl Default for Jinja<'_> {
    fn default() -> Self {
        Self {
            env: set_jinja(&SelectorConfig::default()),
            context: BTreeMap::new(),
        }
    }
}

impl<'a> Extend<(String, Value)> for Jinja<'a> {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, iter: T) {
        self.context.extend(iter);
    }
}

fn jinja_pin_function(
    name: String,
    kwargs: Kwargs,
    internal_repr: &str,
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

    let pin_expr_from_value =
        |pin_expr: &minijinja::value::Value| -> Result<Option<PinExpression>, minijinja::Error> {
            if pin_expr.is_none() {
                Ok(None)
            } else {
                let pin_expr = PinExpression::from_str(&pin_expr.to_string()).map_err(|e| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::SyntaxError,
                        format!("Invalid pin expression: {}", e),
                    )
                })?;
                Ok(Some(pin_expr))
            }
        };

    if let Ok(max_pin) = kwargs.get::<Value>("max_pin") {
        pin.args.max_pin = pin_expr_from_value(&max_pin)?;
    }
    if let Ok(min_pin) = kwargs.get::<Value>("min_pin") {
        pin.args.min_pin = pin_expr_from_value(&min_pin)?;
    }

    if let Ok(lower_bound) = kwargs.get::<String>("lower_bound") {
        pin.args.lower_bound = Some(lower_bound.parse().map_err(|e| {
            minijinja::Error::new(
                minijinja::ErrorKind::SyntaxError,
                format!("Invalid lower bound: {}", e),
            )
        })?);
    }
    if let Ok(upper_bound) = kwargs.get::<String>("upper_bound") {
        pin.args.upper_bound = Some(upper_bound.parse().map_err(|e| {
            minijinja::Error::new(
                minijinja::ErrorKind::SyntaxError,
                format!("Invalid upper bound: {}", e),
            )
        })?);
    }
    if let Ok(exact) = kwargs.get::<bool>("exact") {
        pin.args.exact = exact;
    }

    if internal_repr == "__PIN_SUBPACKAGE" {
        Ok(
            serde_json::to_string(&Dependency::PinSubpackage(PinSubpackage {
                pin_subpackage: pin,
            }))
            .unwrap(),
        )
    } else {
        Ok(
            serde_json::to_string(&Dependency::PinCompatible(PinCompatible {
                pin_compatible: pin,
            }))
            .unwrap(),
        )
    }
}

fn default_compiler(platform: Platform, language: &str) -> Option<String> {
    if platform.is_windows() {
        match language {
            "c" => Some("vs2017"),
            "cxx" => Some("vs2017"),
            "fortran" => Some("gfortran"),
            "rust" => Some("rust"),
            _ => None,
        }
    } else if platform.is_osx() {
        match language {
            "c" => Some("clang"),
            "cxx" => Some("clangxx"),
            "fortran" => Some("gfortran"),
            "rust" => Some("rust"),
            _ => None,
        }
    } else {
        match language {
            "c" => Some("gcc"),
            "cxx" => Some("gxx"),
            "fortran" => Some("gfortran"),
            "rust" => Some("rust"),
            _ => None,
        }
    }
    .map(|s| s.to_string())
}

fn compiler_stdlib_eval(
    lang: &str,
    platform: Platform,
    variant: &Arc<BTreeMap<String, String>>,
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
        .get(&variant_key)
        .cloned()
        .or_else(|| default_fn(platform, lang))
    {
        // check if we also have a compiler version
        if let Some(version) = variant.get(&variant_key_version) {
            Some(format!("{name}_{platform} {version}"))
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

    env.add_filter("split", |s: String, sep: Option<String>| -> Vec<String> {
        s.split(sep.as_deref().unwrap_or(" "))
            .map(|s| s.to_string())
            .collect()
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
    env.add_filter("slice", minijinja::filters::slice);
    env.add_filter("batch", minijinja::filters::batch);
    env.add_filter("sort", minijinja::filters::sort);
    env.add_filter("trim", minijinja::filters::trim);
    env.add_filter("unique", minijinja::filters::unique);
}

fn set_jinja(config: &SelectorConfig) -> minijinja::Environment<'static> {
    let SelectorConfig {
        target_platform,
        build_platform,
        variant,
        experimental,
        allow_undefined,
        ..
    } = config.clone();

    let mut env = Environment::empty();
    default_tests(&mut env);
    default_filters(&mut env);

    // Ok to unwrap here because we know that the syntax is valid
    env.set_syntax(minijinja::Syntax {
        block_start: "{%".into(),
        block_end: "%}".into(),
        variable_start: "${{".into(),
        variable_end: "}}".into(),
        comment_start: "#{{".into(),
        comment_end: "}}#".into(),
    })
    .expect("is tested to be correct");

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
        use rattler_conda_types::Arch;
        let arch = build_platform.arch().or_else(|| target_platform.arch());
        let arch_str = arch.map(|arch| format!("{arch}"));

        let cdt_arch = if let Some(s) = variant_clone.get("cdt_arch") {
            s.as_str()
        } else {
            match arch {
                Some(Arch::X86) => "i686",
                _ => arch_str
                    .as_ref()
                    .ok_or_else(|| {
                        minijinja::Error::new(
                            minijinja::ErrorKind::UndefinedError,
                            "No target or build architecture provided.",
                        )
                    })?
                    .as_str(),
            }
        };

        let cdt_name = variant_clone.get("cdt_name").map_or_else(
            || match arch {
                Some(Arch::S390X | Arch::Aarch64 | Arch::Ppc64le | Arch::Ppc64) => "cos7",
                _ => "cos6",
            },
            String::as_str,
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
        jinja_pin_function(name, kwargs, "__PIN_SUBPACKAGE")
    });

    env.add_function("pin_compatible", |name: String, kwargs: Kwargs| {
        jinja_pin_function(name, kwargs, "__PIN_COMPATIBLE")
    });

    env.add_function("load_from_file", move |path: String| {
        if !experimental {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "Experimental feature: provide the `--experimental` flag to enable this feature",
            ));
        }
        let src = std::fs::read_to_string(&path).map_err(|e| {
            minijinja::Error::new(minijinja::ErrorKind::UndefinedError, e.to_string())
        })?;
        // tracing::info!("loading from path: {path}");
        let filename = path
            .split('/')
            .last()
            .expect("unreachable: split will always atleast return empty string");
        // tracing::info!("loading filename: {filename}");
        let value: minijinja::Value = match filename.split_once('.') {
            Some((_, "yaml")) | Some((_, "yml")) => serde_yaml::from_str(&src).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::CannotDeserialize, e.to_string())
            })?,
            Some((_, "json")) => serde_json::from_str(&src).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::CannotDeserialize, e.to_string())
            })?,
            Some((_, "toml")) => toml::from_str(&src).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::CannotDeserialize, e.to_string())
            })?,
            _ => Value::from(src),
        };
        Ok(value)
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

impl Object for Git {
    fn kind(&self) -> minijinja::value::ObjectKind<'_> {
        minijinja::value::ObjectKind::Plain
    }

    fn call_method(
        &self,
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
        match name {
            "head_rev" => {
                let mut args = args.iter();
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`head_hash` requires at least one argument",
                    ));
                };
                if args.next().is_some() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`head_hash` only accepts one argument",
                    ));
                }
                let Some(src) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`head_hash` requires a string argument",
                    ));
                };
                let output = Command::new("git")
                    .args(["ls-remote", src, "HEAD"])
                    .output()
                    .map_err(|e| {
                        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                    })?;
                let value = if !output.status.success() {
                    Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ))?
                } else {
                    String::from_utf8(output.stdout)
                        .map_err(|e| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                e.to_string(),
                            )
                        })?
                        .lines()
                        .next()
                        .and_then(|s| s.split_ascii_whitespace().nth(0))
                        .ok_or_else(|| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                "Failed to get the HEAD".to_string(),
                            )
                        })?
                        .to_string()
                };
                Ok(Value::from(value))
            }
            "latest_tag_rev" => {
                let mut args = args.iter();
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`latest_tag_rev` requires at least one argument",
                    ));
                };
                if args.next().is_some() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`latest_tag_rev` only accepts one argument",
                    ));
                }
                let Some(src) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`latest_tag_rev` requires a string argument",
                    ));
                };
                let output = Command::new("git")
                    .args(["ls-remote", "--sort=v:refname", "--tags", src])
                    .output()
                    .map_err(|e| {
                        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                    })?;
                let value = if !output.status.success() {
                    Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ))?
                } else {
                    String::from_utf8(output.stdout)
                        .map_err(|e| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                e.to_string(),
                            )
                        })?
                        .lines()
                        .last()
                        .and_then(|s| s.split_ascii_whitespace().nth(0))
                        .ok_or_else(|| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                "Failed to get the latest tag".to_string(),
                            )
                        })?
                        .to_string()
                };
                Ok(Value::from(value))
            }
            "latest_tag" => {
                let mut args = args.iter();
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`latest_tag` requires at least one argument",
                    ));
                };
                if args.next().is_some() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`latest_tag` only accepts one argument",
                    ));
                }
                let Some(src) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`latest_tag` requires a string argument",
                    ));
                };
                let output = Command::new("git")
                    .args(["ls-remote", "--sort=v:refname", "--tags", src])
                    .output()
                    .map_err(|e| {
                        minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                    })?;
                let value = if !output.status.success() {
                    Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ))?
                } else {
                    String::from_utf8(output.stdout)
                        .map_err(|e| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                e.to_string(),
                            )
                        })?
                        .lines()
                        .last()
                        .and_then(|s| s.split_ascii_whitespace().nth(1))
                        .and_then(|s| s.strip_prefix("refs/tags/"))
                        .map(|s| s.trim_end_matches("^{}"))
                        .ok_or_else(|| {
                            minijinja::Error::new(
                                minijinja::ErrorKind::InvalidOperation,
                                "Failed to get the latest tag".to_string(),
                            )
                        })?
                        .to_string()
                };
                Ok(Value::from(value))
            }
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

impl Object for Env {
    fn kind(&self) -> minijinja::value::ObjectKind<'_> {
        minijinja::value::ObjectKind::Plain
    }

    fn call_method(
        &self,
        _state: &minijinja::State,
        name: &str,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        match name {
            "get" => {
                let mut args = args.iter();
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`get` requires at least one argument",
                    ));
                };
                if args.next().is_some() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`get` only accepts one argument",
                    ));
                }
                let Some(key) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`get` requires a string argument",
                    ));
                };
                match std::env::var(key) {
                    Ok(r) => Ok(Value::from(r)),
                    Err(e) => Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        e.to_string(),
                    )),
                }
            }
            "get_default" => {
                let mut args = args.iter();
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`get_default` requires at least two arguments",
                    ));
                };
                let Some(key) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`get_default` requires string arguments",
                    ));
                };
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`get_default` requires at least two arguments",
                    ));
                };
                if args.next().is_some() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`get_default` only accepts two arguments",
                    ));
                }
                let Some(default) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`get_default` requires string arguments",
                    ));
                };
                let ret = std::env::var(key).unwrap_or_else(|_| default.to_string());
                Ok(Value::from(ret))
            }
            "exists" => {
                let mut args = args.iter();
                let Some(arg) = args.next() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::MissingArgument,
                        "`exists` requires at least one argument",
                    ));
                };
                if args.next().is_some() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`exists` only accepts one argument",
                    ));
                }
                let Some(key) = arg.as_str() else {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "`exists` requires a string argument",
                    ));
                };
                Ok(Value::from(std::env::var(key).is_ok()))
            }
            name => Err(minijinja::Error::new(
                minijinja::ErrorKind::UnknownMethod,
                format!("object has no method named {name}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    // git version is too old in cross container for aarch64
    #[cfg(not(all(
        any(target_arch = "aarch64", target_arch = "powerpc64"),
        target_os = "linux"
    )))]
    use std::path::Path;

    use rattler_conda_types::Platform;

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
        _ = std::fs::create_dir_all(&dir).unwrap();
        f(&dir);
        _ = std::fs::remove_dir_all(dir).unwrap();
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
        std::fs::write(path.join(".git/config"), git_config)?;
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
            std::fs::write(path.as_ref().join("README.md"), "init")?;
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
                jinja_wo_experimental.eval(&format!("git.latest_tag({:?})", path)).err().expect("test 2").to_string(),
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
        std::fs::write(&path, "{ \"hello\": \"world\" }").unwrap();
        assert_eq!(
            jinja.eval(&format!("load_from_file('{}')['hello']", path_str)).expect("test 1").as_str(),
            Some("world"),
        );

        let path = temp_dir.path().join("test.yaml");
        std::fs::write(&path, "hello: world").unwrap();
        let path_str = to_forward_slash_lossy(&path);
        assert_eq!(
            jinja.eval(&format!("load_from_file('{}')['hello']", path_str)).expect("test 2").as_str(),
            Some("world"),
        );

        let path = temp_dir.path().join("test.toml");
        let path_str = to_forward_slash_lossy(&path);
        std::fs::write(&path, "hello = 'world'").unwrap();
        assert_eq!(
            jinja.eval(&format!("load_from_file('{}')['hello']", path_str)).expect("test 2").as_str(),
            Some("world"),
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
        let variant = BTreeMap::from_iter(vec![("python".to_string(), "3.7".to_string())]);

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
        let variant = BTreeMap::from_iter(vec![("python".to_string(), "3.7.* *_cpython".to_string())]);

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
            std::env::set_var(key.as_ref(), value.as_ref());
            f();
            std::env::set_var(key.as_ref(), old_value);
        } else {
            std::env::set_var(key.as_ref(), value.as_ref());
            f();
            std::env::remove_var(key.as_ref());
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
            "{\"pin_subpackage\":{\"name\":\"foo\",\"min_pin\":\"x.x.x.x.x.x\",\"max_pin\":\"x\"}}"
        );
        assert_eq!(
            ps("max_pin=None"),
            "{\"pin_subpackage\":{\"name\":\"foo\",\"min_pin\":\"x.x.x.x.x.x\",\"max_pin\":null}}"
        );
        assert_eq!(
            ps("max_pin=None, min_pin=None"),
            "{\"pin_subpackage\":{\"name\":\"foo\",\"min_pin\":null,\"max_pin\":null}}"
        );
        assert_eq!(ps("lower_bound='1.2.3'"), "{\"pin_subpackage\":{\"name\":\"foo\",\"min_pin\":\"x.x.x.x.x.x\",\"max_pin\":\"x\",\"lower_bound\":\"1.2.3\"}}");
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

            jinja.eval(&format!("var | {func}")).unwrap().to_string()
        };

        assert_eq!(split_test("foo bar", None), "[\"foo\", \"bar\"]");
        assert_eq!(split_test("foobar", None), "[\"foobar\"]");
        assert_eq!(split_test("1.2.3", Some(".")), "[\"1\", \"2\", \"3\"]");
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
                    .eval("env.get_default('RANDOM_JINJA_ENV_VAR', 'true')")
                    .expect("test 3")
                    .as_str(),
                Some("false")
            );
            assert_eq!(
                jinja
                    .eval("env.get_default('RANDOM_JINJA_ENV_VAR2', 'true')")
                    .expect("test 4")
                    .as_str(),
                Some("true")
            );
            assert!(jinja
                .eval("env.exists('RANDOM_JINJA_ENV_VAR')")
                .expect("test 5")
                .is_true());
            assert!(!jinja
                .eval("env.exists('RANDOM_JINJA_ENV_VAR2')")
                .expect("test 6")
                .is_true());
        });
    }
}
