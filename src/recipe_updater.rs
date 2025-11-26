//! Recipe updating functionality for --update-sha256 and --update-version flags

use fs_err as fs;
use rattler_digest::Sha256Hash;
use regex::Regex;
use std::path::Path;

/// Updates a recipe file with new SHA256 checksums and/or version
pub struct RecipeUpdater {
    recipe_path: std::path::PathBuf,
    yaml_content: String,
}

impl RecipeUpdater {
    /// Load a recipe file for updating
    pub fn load(recipe_path: impl AsRef<Path>) -> Result<Self, RecipeUpdaterError> {
        let recipe_path = recipe_path.as_ref().to_path_buf();
        let yaml_content = fs::read_to_string(&recipe_path).map_err(|e| {
            RecipeUpdaterError::IoError(format!("Failed to read recipe file: {}", e))
        })?;

        Ok(Self {
            recipe_path,
            yaml_content,
        })
    }

    /// Update SHA256 checksums for URL sources
    pub fn update_sha256(
        &mut self,
        new_checksums: &[(String, Sha256Hash)],
    ) -> Result<(), RecipeUpdaterError> {
        // Compile regex outside the loop
        let sha256_pattern = Regex::new(r"sha256:\s*([a-fA-F0-9]{64})")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;

        // For each URL/checksum pair, find and replace the corresponding SHA256 in the text
        for (url, new_hash) in new_checksums {
            // First, we need to find which SHA256 corresponds to this URL
            // We'll look for the URL in the content, then find the nearest sha256 field
            if self.yaml_content.contains(url) {
                // Replace the first SHA256 hash we find with the new one
                if sha256_pattern.is_match(&self.yaml_content) {
                    let new_hash_str = format!("{:x}", new_hash);
                    self.yaml_content = sha256_pattern
                        .replace(
                            &self.yaml_content,
                            format!("sha256: {}", new_hash_str).as_str(),
                        )
                        .to_string();
                    tracing::debug!("Updated sha256 for URL: {}", url);
                    break; // Only replace the first match to avoid updating multiple sources
                }
            }
        }
        Ok(())
    }

    /// Update SHA256 checksums in order (replaces nth SHA256 with nth checksum)
    ///
    /// This is useful when updating recipes where URLs contain Jinja2 templates
    /// that don't match the expanded URLs.
    pub fn update_sha256_in_order(
        &mut self,
        new_checksums: &[Sha256Hash],
    ) -> Result<(), RecipeUpdaterError> {
        let sha256_pattern = Regex::new(r"sha256:\s*([a-fA-F0-9]{64})")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;

        let mut content = self.yaml_content.clone();

        // Replace each SHA256 match with the corresponding new checksum
        for new_hash in new_checksums {
            if sha256_pattern.is_match(&content) {
                let new_hash_str = format!("{:x}", new_hash);
                content = sha256_pattern
                    .replace(&content, format!("sha256: {}", new_hash_str).as_str())
                    .to_string();
                tracing::debug!("Updated sha256 to: {}", new_hash_str);
            } else {
                break;
            }
        }

        self.yaml_content = content;
        Ok(())
    }

    /// Update version in the package section and URLs
    pub fn update_version(&mut self, new_version: &str) -> Result<(), RecipeUpdaterError> {
        // Update package version - find the first "version:" under "package:" section
        let version_pattern = Regex::new(r#"(?m)^(\s*version:\s*)(["']?)([^"'\n]+)(["']?)$"#)
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;

        if version_pattern.is_match(&self.yaml_content) {
            self.yaml_content = version_pattern
                .replace(
                    &self.yaml_content,
                    format!("${{1}}${{2}}{}${{4}}", new_version).as_str(),
                )
                .to_string();
            tracing::debug!("Updated package version to: {}", new_version);
        }

        // Update URLs with version placeholders
        let url_pattern = Regex::new(r"(?m)^(\s*url:\s*)(.+)$")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;

        let mut updated_content = self.yaml_content.clone();
        for captures in url_pattern.captures_iter(&self.yaml_content) {
            let full_match = captures.get(0).unwrap().as_str();
            let indent = captures.get(1).unwrap().as_str();
            let url_part = captures.get(2).unwrap().as_str();

            let updated_url = Self::update_url_version(url_part.trim(), new_version)?;
            if updated_url != url_part.trim() {
                let new_line = format!("{}{}", indent, updated_url);
                updated_content = updated_content.replace(full_match, &new_line);
                tracing::debug!("Updated URL with new version: {}", updated_url);
            }
        }
        self.yaml_content = updated_content;

        Ok(())
    }

    fn update_url_version(url_str: &str, new_version: &str) -> Result<String, RecipeUpdaterError> {
        // Try different patterns for version replacement in URLs

        // GitHub archive URLs: /archive/v1.2.3.tar.gz
        let github_archive_re =
            Regex::new(r"/archive/v(\d+\.\d+\.\d+(?:\.\d+)?)(\.tar\.(gz|bz2|xz)|\.zip)")
                .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;
        if github_archive_re.is_match(url_str) {
            let result = github_archive_re
                .replace(url_str, format!("/archive/v{}$2", new_version).as_str())
                .to_string();
            return Ok(result);
        }

        // Archive names: package-1.2.3.tar.gz
        let archive_re = Regex::new(r"([^/]+-)(\d+\.\d+\.\d+(?:\.\d+)?)(\.(tar\.(gz|bz2|xz)|zip))")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;
        if let Some(captures) = archive_re.captures(url_str) {
            let prefix = captures.get(1).unwrap().as_str();
            let extension = captures.get(3).unwrap().as_str();
            let full_match = captures.get(0).unwrap();
            let replacement = format!("{}{}{}", prefix, new_version, extension);
            let result = url_str.replace(full_match.as_str(), &replacement);
            return Ok(result);
        }

        // GitHub releases: /v1.2.3/
        let github_release_re = Regex::new(r"/v(\d+\.\d+\.\d+(?:\.\d+)?)/")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;
        if github_release_re.is_match(url_str) {
            let result = github_release_re
                .replace(url_str, format!("/v{}/", new_version).as_str())
                .to_string();
            return Ok(result);
        }

        // GitHub API tarballs: /tarball/v1.2.3
        let tarball_re = Regex::new(r"/tarball/v(\d+\.\d+\.\d+(?:\.\d+)?)")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;
        if tarball_re.is_match(url_str) {
            let result = tarball_re
                .replace(url_str, format!("/tarball/v{}", new_version).as_str())
                .to_string();
            return Ok(result);
        }

        // Version at end of path: v1.2.3
        let end_version_re = Regex::new(r"/v(\d+\.\d+\.\d+(?:\.\d+)?)$")
            .map_err(|e| RecipeUpdaterError::RegexError(e.to_string()))?;
        if end_version_re.is_match(url_str) {
            let result = end_version_re
                .replace(url_str, format!("/v{}", new_version).as_str())
                .to_string();
            return Ok(result);
        }

        // If no pattern matched, return the original URL
        Ok(url_str.to_string())
    }

    /// Save the updated recipe back to the file
    pub fn save(&self) -> Result<(), RecipeUpdaterError> {
        fs::write(&self.recipe_path, &self.yaml_content).map_err(|e| {
            RecipeUpdaterError::IoError(format!("Failed to write recipe file: {}", e))
        })?;

        tracing::debug!("Saved recipe to: {}", self.recipe_path.display());
        Ok(())
    }

    /// Get the recipe file path
    pub fn path(&self) -> &Path {
        &self.recipe_path
    }
}

/// Errors that can occur during recipe updating
#[derive(Debug, thiserror::Error)]
pub enum RecipeUpdaterError {
    /// I/O operation failed
    #[error("I/O error: {0}")]
    IoError(String),

    /// Regular expression compilation or execution failed
    #[error("Regex error: {0}")]
    RegexError(String),
}

/// Calculate SHA256 hash of a file
pub fn calculate_sha256(file_path: &Path) -> Result<Sha256Hash, RecipeUpdaterError> {
    let content = fs::read(file_path).map_err(|e| {
        RecipeUpdaterError::IoError(format!("Failed to read file for hashing: {}", e))
    })?;

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_update_version_in_url() {
        let test_cases = vec![
            (
                "https://github.com/user/repo/archive/v1.2.3.tar.gz",
                "2.0.0",
                "https://github.com/user/repo/archive/v2.0.0.tar.gz",
            ),
            (
                "https://files.pythonhosted.org/packages/source/p/package/package-1.2.3.tar.gz",
                "2.0.0",
                "https://files.pythonhosted.org/packages/source/p/package/package-2.0.0.tar.gz",
            ),
            (
                "https://api.github.com/repos/user/repo/tarball/v1.2.3",
                "2.0.0",
                "https://api.github.com/repos/user/repo/tarball/v2.0.0",
            ),
        ];

        for (original, new_version, expected) in test_cases {
            let result = RecipeUpdater::update_url_version(original, new_version).unwrap();
            assert_eq!(result, expected, "Failed for URL: {}", original);
        }
    }

    #[test]
    fn test_recipe_update() {
        let recipe_content = r#"schema_version: 1
package:
  name: test-package
  version: "1.0.0"
source:
  url: https://github.com/user/repo/archive/v1.0.0.tar.gz
  sha256: 1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef
build:
  number: 0
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(recipe_content.as_bytes()).unwrap();

        let mut updater = RecipeUpdater::load(temp_file.path()).unwrap();

        // Test version update
        updater.update_version("2.0.0").unwrap();

        // Check that version was updated in the text content
        assert!(updater.yaml_content.contains("version: \"2.0.0\""));

        // Check that URL was updated in the text content
        assert!(
            updater
                .yaml_content
                .contains("https://github.com/user/repo/archive/v2.0.0.tar.gz")
        );

        // Verify the original URL is no longer present
        assert!(
            !updater
                .yaml_content
                .contains("https://github.com/user/repo/archive/v1.0.0.tar.gz")
        );
        assert!(!updater.yaml_content.contains("version: \"1.0.0\""));
    }

    #[test]
    fn test_formatting_preservation() {
        let recipe_content = r#"# This is a comment
schema_version: 1

package:
  name: test-package  # inline comment
  version: "1.0.0"

source:
  url: https://github.com/user/repo/archive/v1.0.0.tar.gz
  sha256: 1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef

build:
  number: 0  # another comment
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(recipe_content.as_bytes()).unwrap();

        let mut updater = RecipeUpdater::load(temp_file.path()).unwrap();

        // Test that comments and spacing are preserved
        updater.update_version("2.0.0").unwrap();

        // Check that formatting is preserved
        assert!(updater.yaml_content.contains("# This is a comment"));
        assert!(updater.yaml_content.contains("# inline comment"));
        assert!(updater.yaml_content.contains("# another comment"));

        // Check that the version was updated while preserving quotes
        assert!(updater.yaml_content.contains("version: \"2.0.0\""));

        // Check that URL was updated
        assert!(
            updater
                .yaml_content
                .contains("https://github.com/user/repo/archive/v2.0.0.tar.gz")
        );
    }
}
