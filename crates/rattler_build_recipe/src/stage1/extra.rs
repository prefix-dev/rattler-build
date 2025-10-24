//! Stage 1 Extra - evaluated extra metadata with concrete values

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Evaluated extra metadata with all templates and conditionals resolved
/// This is a free-form section that can contain any additional metadata
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Extra {
    /// Free-form extra metadata
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra: IndexMap<String, serde_value::Value>,
}

impl Extra {
    /// Create a new empty Extra section
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the Extra section is empty
    pub fn is_empty(&self) -> bool {
        self.extra.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extra_creation() {
        let extra = Extra::new();
        assert!(extra.is_empty());
    }

    #[test]
    fn test_extra_with_data() {
        let mut extra_map = IndexMap::new();
        extra_map.insert(
            "recipe-maintainers".to_string(),
            serde_value::Value::Seq(vec![
                serde_value::Value::String("Alice <alice@example.com>".to_string()),
                serde_value::Value::String("Bob <bob@example.com>".to_string()),
            ]),
        );
        extra_map.insert(
            "custom-field".to_string(),
            serde_value::Value::String("custom-value".to_string()),
        );

        let extra = Extra { extra: extra_map };

        assert!(!extra.is_empty());
        assert_eq!(extra.extra.len(), 2);
        assert!(extra.extra.contains_key("recipe-maintainers"));
        assert!(extra.extra.contains_key("custom-field"));
    }
}
