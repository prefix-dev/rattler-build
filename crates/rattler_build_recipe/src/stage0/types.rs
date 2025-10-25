//! Stage0 types for recipe parsing - templates and conditionals before evaluation
//!
//! This module re-exports types from rattler_build_yaml_parser.
//! Stage0 represents the initial parse state where values can be Jinja templates
//! or conditionals that haven't been evaluated yet.

// Re-export Jinja types
pub use rattler_build_jinja::{JinjaExpression, JinjaTemplate};

// Re-export all basic parsing types from the shared parser
pub use rattler_build_yaml_parser::{Conditional, ConditionalList, Item, ListOrItem, Value};

// Additional recipe-specific types below

use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};

/// Include or exclude patterns for file selection
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IncludeExclude<T = String> {
    /// Simple list of items
    List(ConditionalList<T>),
    /// Include/exclude mapping
    Mapping {
        /// Items to include
        #[serde(default)]
        include: ConditionalList<T>,
        /// Items to exclude
        #[serde(default)]
        exclude: ConditionalList<T>,
    },
}

impl<T> Default for IncludeExclude<T> {
    fn default() -> Self {
        Self::List(ConditionalList::default())
    }
}

impl<T: ToString + Debug> IncludeExclude<T> {
    /// Collect all variables used in this include/exclude pattern
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        match self {
            IncludeExclude::List(list) => {
                vars.extend(list.used_variables());
            }
            IncludeExclude::Mapping { include, exclude } => {
                vars.extend(include.used_variables());
                vars.extend(exclude.used_variables());
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}

/// Build script configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    /// Optional interpreter (e.g., "bash", "python")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Value<String>>,

    /// Environment variables for the script
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub env: indexmap::IndexMap<String, Value<String>>,

    /// Secrets to expose to the script
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,

    /// Inline script content - can be a string or array of commands with conditionals
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ConditionalList<String>>,

    /// File path to script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<Value<String>>,

    /// Working directory for the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<Value<String>>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            interpreter: None,
            env: indexmap::IndexMap::new(),
            secrets: Vec::new(),
            content: None,
            file: None,
            cwd: None,
        }
    }
}

impl Display for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.content.is_some() {
            write!(f, "Script(content: [...])")
        } else if let Some(file) = &self.file {
            write!(f, "Script(file: {})", file)
        } else {
            write!(f, "Script(default)")
        }
    }
}

impl Script {
    /// Check if this script is default (all fields empty/none)
    pub fn is_default(&self) -> bool {
        self.content.is_none()
            && self.file.is_none()
            && self.interpreter.is_none()
            && self.cwd.is_none()
            && self.env.is_empty()
            && self.secrets.is_empty()
    }

    /// Collect all variables used in this script
    pub fn used_variables(&self) -> Vec<String> {
        let Script {
            interpreter,
            env,
            secrets: _,
            content,
            file,
            cwd,
        } = self;

        let mut vars = Vec::new();

        if let Some(interpreter) = interpreter {
            vars.extend(interpreter.used_variables());
        }

        for value in env.values() {
            vars.extend(value.used_variables());
        }

        if let Some(content) = content {
            vars.extend(content.used_variables());
        }

        if let Some(file) = file {
            vars.extend(file.used_variables());
        }

        if let Some(cwd) = cwd {
            vars.extend(cwd.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }
}
