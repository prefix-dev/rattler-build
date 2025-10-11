//! Stage 1 Extra - evaluated extra metadata with concrete values

/// Evaluated extra metadata with all templates and conditionals resolved
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extra {
    /// List of recipe maintainers
    pub recipe_maintainers: Vec<String>,
}

impl Extra {
    /// Create a new empty Extra section
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the Extra section is empty
    pub fn is_empty(&self) -> bool {
        self.recipe_maintainers.is_empty()
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
    fn test_extra_with_maintainers() {
        let extra = Extra {
            recipe_maintainers: vec![
                "Alice <alice@example.com>".to_string(),
                "Bob <bob@example.com>".to_string(),
            ],
        };

        assert!(!extra.is_empty());
        assert_eq!(extra.recipe_maintainers.len(), 2);
    }
}
