//! Stage 1 About - evaluated package metadata with concrete values

use spdx::Expression;
use url::Url;

/// Evaluated package metadata with all templates and conditionals resolved
#[derive(Debug, Clone, Default, PartialEq)]
pub struct About {
    /// Package homepage URL (validated)
    pub homepage: Option<Url>,

    /// Repository URL
    pub repository: Option<Url>,

    /// Documentation URL
    pub documentation: Option<Url>,

    /// License expression (validated SPDX)
    pub license: Option<Expression>,

    /// License file paths
    pub license_file: Vec<String>,

    /// Package summary/description
    pub summary: Option<String>,

    /// Longer package description
    pub description: Option<String>,
}

impl About {
    /// Create a new empty About section
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the About section is empty (all fields are None/empty)
    pub fn is_empty(&self) -> bool {
        self.homepage.is_none()
            && self.repository.is_none()
            && self.documentation.is_none()
            && self.license.is_none()
            && self.license_file.is_empty()
            && self.summary.is_none()
            && self.description.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_about_creation() {
        let about = About::new();
        assert!(about.is_empty());
    }

    #[test]
    fn test_about_with_fields() {
        let about = About {
            homepage: Some(Url::parse("https://example.com").unwrap()),
            license: Some(Expression::parse("MIT").unwrap()),
            summary: Some("A test package".to_string()),
            ..Default::default()
        };

        assert!(!about.is_empty());
        assert_eq!(
            about.homepage.as_ref().map(|u| u.as_str()),
            Some("https://example.com/")
        );
        assert_eq!(about.license.as_ref().map(|l| l.as_ref()), Some("MIT"));
        assert_eq!(about.summary, Some("A test package".to_string()));
        assert_eq!(about.repository, None);
    }
}
