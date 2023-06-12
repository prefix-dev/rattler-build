use std::str::FromStr;

use minijinja::{value::Value, Environment, Syntax};
use rattler_conda_types::{Version, VersionSpec};

mod functions {
    use std::str::FromStr;

    use minijinja::Error;

    use crate::render::pin::{Pin, PinExpression};

    pub fn compiler(lang: String) -> Result<String, Error> {
        // we translate the compiler into a YAML string
        Ok(format!("__COMPILER {}", lang.to_lowercase()))
    }

    pub fn pin_subpackage(
        name: String,
        kwargs: Option<minijinja::value::Value>,
    ) -> Result<String, Error> {
        // we translate the compiler into a YAML string
        let mut pin_subpackage = Pin {
            name,
            max_pin: None,
            min_pin: None,
            exact: false,
        };

        let pin_expr_from_value = |pin_expr: &minijinja::value::Value| {
            PinExpression::from_str(&pin_expr.to_string()).map_err(|e| {
                Error::new(
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
    }
}

/// Get a jinja environment with the correct syntax and functions
pub fn jinja_environment() -> Environment<'static> {
    let mut env = Environment::new();

    env.set_syntax(Syntax {
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

    env.add_function("compiler", functions::compiler);
    env.add_function("pin_subpackage", functions::pin_subpackage);

    env
}
