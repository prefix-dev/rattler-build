use std::sync::Arc;

use minijinja::{
    Value,
    value::{Kwargs, Object, from_args},
};

#[derive(Debug)]
pub(crate) struct Env;
impl std::fmt::Display for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Env")
    }
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
impl Env {
    fn get(&self, env_var: &str, kwargs: Kwargs) -> Result<Value, minijinja::Error> {
        let default_value = kwargs.get::<String>("default").ok();
        kwargs.assert_all_used()?;

        match default_value {
            Some(default_value) => Ok(Value::from(default_value)),
            None => Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!(
                    "Environment variable {env_var} not found (env access not available in WASM)"
                ),
            )),
        }
    }

    fn exists(&self, _env_var: &str) -> Result<Value, minijinja::Error> {
        Ok(Value::from(false))
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
