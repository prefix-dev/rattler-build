use std::fmt::Display;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Stage0 Extra - free-form metadata that can contain templates and conditionals
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct Extra {
    /// Free-form extra metadata with Jinja template support
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra: IndexMap<String, serde_value::Value>,
}

impl Display for Extra {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{ extra: {} fields }}", self.extra.len())
    }
}

impl Extra {
    /// Collect all variables used in template expressions
    pub fn used_variables(&self) -> Vec<String> {
        Vec::new()
    }
}
