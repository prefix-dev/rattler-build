//! Module for types and functions related to miniJinja setup for recipes.
//!
use std::{collections::BTreeMap, str::FromStr};

use minijinja::{Environment, Value};
use rattler_conda_types::Version;

pub use crate::render::pin::{Pin, PinExpression};
pub use crate::selectors::SelectorConfig;

/// A type that hold the miniJinja environment and context for Jinja template processing.
#[derive(Debug, Clone)]
pub struct Jinja<'a> {
    env: Environment<'a>,
    context: BTreeMap<String, Value>,
}

impl<'a> Jinja<'a> {
    /// Create a new Jinja instance with the given selector configuration.
    pub fn new(config: SelectorConfig) -> Self {
        let env = set_jinja();
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
            env: set_jinja(),
            context: BTreeMap::new(),
        }
    }
}

impl<'a> Extend<(String, Value)> for Jinja<'a> {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, iter: T) {
        self.context.extend(iter);
    }
}

fn set_jinja() -> minijinja::Environment<'static> {
    use rattler_conda_types::version_spec::VersionSpec;
    let mut env = minijinja::Environment::new();
    env.set_syntax(minijinja::Syntax {
        block_start: "{%".into(),
        block_end: "%}".into(),
        variable_start: "${{".into(),
        variable_end: "}}".into(),
        comment_start: "#{{".into(),
        comment_end: "}}#".into(),
    })
    .unwrap();

    env.add_function("cmp", |a: &Value, spec: &str| {
        if let Some(version) = a.as_str() {
            // check if version matches spec
            let version = Version::from_str(version).unwrap();
            let version_spec = VersionSpec::from_str(spec).unwrap();
            Ok(version_spec.matches(&version))
        } else {
            // if a is undefined, we are currently searching for all variants and thus return true
            Ok(true)
        }
    });

    env.add_function("compiler", |lang: String| {
        // we translate the compiler into a YAML string
        Ok(format!("__COMPILER {}", lang.to_lowercase()))
    });

    env.add_function("pin_subpackage", |name: String, kwargs: Option<Value>| {
        use rattler_conda_types::PackageName;

        let name = PackageName::try_from(name).map_err(|e| {
            minijinja::Error::new(
                minijinja::ErrorKind::SyntaxError,
                format!("Invalid package name in pin_subpackage: {}", e),
            )
        })?;

        // we translate the compiler into a YAML string
        let mut pin_subpackage = Pin {
            name,
            max_pin: None,
            min_pin: None,
            exact: false,
        };

        let pin_expr_from_value = |pin_expr: &minijinja::value::Value| {
            PinExpression::from_str(&pin_expr.to_string()).map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::SyntaxError,
                    format!("Invalid pin expression: {}", e),
                )
            })
        };

        if let Some(kwargs) = kwargs {
            let max_pin = kwargs.get_attr("max_pin")?;
            if max_pin != minijinja::value::Value::UNDEFINED {
                let pin_expr = pin_expr_from_value(&max_pin)?;
                pin_subpackage.max_pin = Some(pin_expr);
            }
            let min = kwargs.get_attr("min_pin")?;
            if min != minijinja::value::Value::UNDEFINED {
                let pin_expr = pin_expr_from_value(&min)?;
                pin_subpackage.min_pin = Some(pin_expr);
            }
            let exact = kwargs.get_attr("exact")?;
            if exact != minijinja::value::Value::UNDEFINED {
                pin_subpackage.exact = exact.is_true();
            }
        }

        Ok(format!(
            "__PIN_SUBPACKAGE {}",
            pin_subpackage.internal_repr()
        ))
    });

    env
}

#[cfg(test)]
mod tests {
    use rattler_conda_types::Platform;

    use super::*;

    #[test]
    #[rustfmt::skip]
    fn eval() {
        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant: BTreeMap::new(),
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
            build_platform: Platform::Linux64,
            variant: BTreeMap::new(),
        };

        let jinja = Jinja::new(options);

        assert!(jinja.eval("${{ true if win }}").expect("test 1").is_true());
    }

    #[test]
    #[rustfmt::skip]
    fn eval_cmp() {
        let variant = BTreeMap::from_iter(vec![("python".to_string(), "3.7".to_string())]);

        let options = SelectorConfig {
            target_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant,
        };
        let jinja = Jinja::new(options);

        assert!(jinja.eval("cmp(python, '==3.7')").expect("test 1").is_true());
        assert!(jinja.eval("cmp(python, '>=3.7')").expect("test 2").is_true());
        assert!(jinja.eval("cmp(python, '>=3.7,<3.9')").expect("test 3").is_true());

        assert!(!jinja.eval("cmp(python, '!=3.7')").expect("test 4").is_true());
        assert!(!jinja.eval("cmp(python, '<3.7')").expect("test 5").is_true());
        assert!(!jinja.eval("cmp(python, '>3.5,<3.7')").expect("test 6").is_true());
    }
}
