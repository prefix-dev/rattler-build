use std::sync::Arc;

use minijinja::{
    Value,
    value::{Kwargs, Object, from_args},
};

/// Look up an environment variable by name, returning `None` on WASM.
fn lookup_env_var(name: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var(name).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        None
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

        match lookup_env_var(env_var).or(default_value) {
            Some(value) => Ok(Value::from(value)),
            None => Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!("Environment variable {env_var} not found"),
            )),
        }
    }

    fn exists(&self, env_var: &str) -> Result<Value, minijinja::Error> {
        Ok(Value::from(lookup_env_var(env_var).is_some()))
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
