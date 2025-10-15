//! This module defines the `Variable` type, which represents a variable in a recipe.
//! Variables can be strings, numbers, booleans, or lists of these types.
use minijinja::Value;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};

/// This represents a variable in a recipe. It is a wrapper around a `minijinja::Value`,
/// but more constrained (it can only be a string, a number, a boolean, or a list of these types).
#[derive(Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct Variable(Value);

impl From<Variable> for Value {
    fn from(variable: Variable) -> Self {
        variable.0
    }
}

impl AsRef<Value> for Variable {
    fn as_ref(&self) -> &Value {
        &self.0
    }
}

impl From<bool> for Variable {
    fn from(value: bool) -> Self {
        Variable(Value::from_serialize(value))
    }
}

impl From<i64> for Variable {
    fn from(value: i64) -> Self {
        Variable(Value::from_serialize(value))
    }
}

impl From<String> for Variable {
    fn from(value: String) -> Self {
        Variable(Value::from_safe_string(value))
    }
}

impl From<&str> for Variable {
    fn from(value: &str) -> Self {
        Variable(Value::from_safe_string(value.to_string()))
    }
}

impl From<Vec<Variable>> for Variable {
    fn from(value: Vec<Variable>) -> Self {
        Variable(Value::from_serialize(value))
    }
}

impl Variable {
    /// Create a variable from a string
    pub fn from_string(value: &str) -> Self {
        Variable(Value::from_safe_string(value.to_string()))
    }
}

impl Display for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Debug for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(s) = self.0.as_str() {
            write!(f, "\"{}\"", s)
        } else {
            write!(f, "{:?}", self.0)
        }
    }
}
