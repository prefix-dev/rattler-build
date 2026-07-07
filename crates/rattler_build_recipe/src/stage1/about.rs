//! Stage 1 About - evaluated package metadata with concrete values

use rattler_build_types::LateBoundGlobVec;
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

    /// License file patterns.
    ///
    /// A single list that mixes ordinary globs and late-bound patterns (e.g.
    /// `${{ PREFIX }}/share/licenses/LICENSE`), so the rendered recipe exposes
    /// one `license_file` key. `None` means the key was absent from the recipe;
    /// `Some` (possibly empty) means it was explicitly set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_file: Option<LateBoundGlobVec>,

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
            && self.license_file.is_none()
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

    #[test]
    fn test_license_file_serialize_single_key() {
        // A mix of ordinary globs and late-bound patterns must render as a
        // single `license_file` list, never a separate late-bound key.
        let about = About {
            license_file: Some(
                LateBoundGlobVec::from_sources(
                    vec![
                        "LICENSE".to_string(),
                        "${{ PREFIX }}/share/licenses/LICENSE".to_string(),
                    ],
                    Vec::new(),
                )
                .unwrap(),
            ),
            ..Default::default()
        };

        let yaml = serde_yaml::to_string(&about).unwrap();
        assert!(
            !yaml.contains("late_bound"),
            "rendered recipe leaked a late-bound key:\n{yaml}"
        );
        assert_eq!(yaml.matches("license_file").count(), 1);

        // Round-trips back into the same value.
        let parsed: About = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, about);
        let lf = parsed.license_file.unwrap();
        assert_eq!(lf.ordinary_globs().include_globs().len(), 1);
        assert_eq!(lf.late_bound().count(), 1);
    }
}
