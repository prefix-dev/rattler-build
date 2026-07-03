//! Stage 1 About - evaluated package metadata with concrete values

use rattler_build_types::LateBoundPath;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{stage0::License, stage1::GlobVec};

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
    /// Holds both ordinary globs and late-bound paths (e.g.
    /// `${{ PREFIX }}/share/licenses/LICENSE`) as a single unit, so the
    /// rendered recipe exposes one `license_file` key. `None` means the key was
    /// absent from the recipe; `Some` (possibly empty) means it was explicitly
    /// set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_file: Option<LicenseFiles>,

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

/// The evaluated `about.license_file` entries.
///
/// A single `license_file` recipe key can mix ordinary globs (matched against
/// the work and recipe directories during packaging) with late-bound paths
/// that reference build directory variables (e.g.
/// `${{ PREFIX }}/share/licenses/LICENSE`, resolved once the build directories
/// are known). Both kinds are stored together here and (de)serialize to a
/// single `license_file` key, but they are kept apart internally because they
/// are resolved differently at packaging time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LicenseFiles {
    /// Ordinary relative/absolute glob patterns.
    globs: GlobVec,
    /// Entries referencing late-bound build directory variables.
    late_bound: Vec<LateBoundPath>,
}

impl LicenseFiles {
    /// Create a new set of license files from globs and late-bound paths.
    pub fn new(globs: GlobVec, late_bound: Vec<LateBoundPath>) -> Self {
        Self { globs, late_bound }
    }

    /// The ordinary glob patterns.
    pub fn globs(&self) -> &GlobVec {
        &self.globs
    }

    /// The late-bound paths (e.g. `${{ PREFIX }}/...`).
    pub fn late_bound(&self) -> &[LateBoundPath] {
        &self.late_bound
    }

    /// Returns `true` if there are no glob patterns and no late-bound paths.
    pub fn is_empty(&self) -> bool {
        self.globs.is_empty() && self.late_bound.is_empty()
    }
}

/// The serialized shape of `about.license_file`: either a plain list of
/// patterns or an include/exclude map. Entries may be ordinary globs or
/// late-bound paths (e.g. `${{ PREFIX }}/...`); they are routed to the right
/// internal field on deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum LicenseFilesRepr {
    List(Vec<String>),
    Map {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        include: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        exclude: Vec<String>,
    },
}

impl Serialize for LicenseFiles {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Merge the ordinary globs and the late-bound paths back into a single
        // list (or include/exclude map, if there are excludes).
        let mut include: Vec<String> = self
            .globs
            .include_globs()
            .iter()
            .map(|g| g.source().to_string())
            .collect();
        include.extend(self.late_bound.iter().map(|p| p.as_str().to_string()));

        let exclude: Vec<String> = self
            .globs
            .exclude_globs()
            .iter()
            .map(|g| g.source().to_string())
            .collect();

        let repr = if exclude.is_empty() {
            LicenseFilesRepr::List(include)
        } else {
            LicenseFilesRepr::Map { include, exclude }
        };
        repr.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LicenseFiles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = LicenseFilesRepr::deserialize(deserializer)?;
        let (include, exclude) = match repr {
            LicenseFilesRepr::List(list) => (list, Vec::new()),
            LicenseFilesRepr::Map { include, exclude } => (include, exclude),
        };

        // Route each entry to globs or late-bound paths, mirroring the split
        // done during recipe evaluation.
        let mut glob_sources = Vec::new();
        let mut late_bound = Vec::new();
        for entry in include {
            let path = LateBoundPath::new(entry);
            if path.is_late_bound() {
                late_bound.push(path);
            } else {
                glob_sources.push(path.as_str().to_string());
            }
        }

        let globs = GlobVec::from_strings(glob_sources, exclude)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        Ok(LicenseFiles { globs, late_bound })
    }
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
    fn test_license_files_serialize_single_key() {
        // A mix of ordinary globs and late-bound paths must render as a single
        // `license_file` list, never a separate late-bound key.
        let license_file = LicenseFiles::new(
            GlobVec::from_strings(vec!["LICENSE".to_string()], Vec::new()).unwrap(),
            vec![LateBoundPath::new("${{ PREFIX }}/share/licenses/LICENSE")],
        );
        let about = About {
            license_file: Some(license_file),
            ..Default::default()
        };

        let yaml = serde_yaml::to_string(&about).unwrap();
        assert!(
            !yaml.contains("late_bound"),
            "rendered recipe leaked a late-bound key:\n{yaml}"
        );
        assert_eq!(yaml.matches("license_file").count(), 1);

        // Round-trips back into the split internal representation.
        let parsed: About = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, about);
        let lf = parsed.license_file.unwrap();
        assert_eq!(lf.globs().include_globs().len(), 1);
        assert_eq!(lf.late_bound().len(), 1);
    }

    #[test]
    fn test_license_files_serialize_with_exclude() {
        let license_file = LicenseFiles::new(
            GlobVec::from_strings(
                vec!["licenses/*".to_string()],
                vec!["licenses/skip".to_string()],
            )
            .unwrap(),
            vec![LateBoundPath::new("${{ SRC_DIR }}/LICENSE")],
        );
        let about = About {
            license_file: Some(license_file.clone()),
            ..Default::default()
        };

        let yaml = serde_yaml::to_string(&about).unwrap();
        assert!(!yaml.contains("late_bound"));
        let parsed: About = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.license_file, Some(license_file));
    }
}
