//! Stage 1 About - evaluated package metadata with concrete values

use serde::{Deserialize, Serialize};
use url::Url;

use crate::stage0::License;

/// Evaluated package metadata with all templates and conditionals resolved
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct About {
    /// Package homepage URL (validated)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<Url>,

    /// Repository URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<Url>,

    /// Documentation URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<Url>,

    /// License expression (validated SPDX)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,

    /// License file paths
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub license_file: Vec<String>,

    /// License family (e.g., MIT, BSD, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_family: Option<String>,

    /// Package summary/description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Longer package description
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
            && self.license_family.is_none()
            && self.summary.is_none()
            && self.description.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_about_creation() {
        let about = About::new();
        assert!(about.is_empty());
    }

    #[test]
    fn test_about_with_fields() {
        let about = About {
            homepage: Some(Url::parse("https://example.com").unwrap()),
            license: Some(License::from_str("MIT").unwrap()),
            summary: Some("A test package".to_string()),
            ..Default::default()
        };

        assert!(!about.is_empty());
        assert_eq!(
            about.homepage.as_ref().map(|u| u.as_str()),
            Some("https://example.com/")
        );
        assert_eq!(
            about.license.as_ref().map(|l| l.to_string()),
            Some("MIT".to_string())
        );
        assert_eq!(about.summary, Some("A test package".to_string()));
        assert_eq!(about.repository, None);
    }
}
