//! Recipe bumping functionality for updating versions and checksums.
//!
//! This module provides functionality to:
//! - Bump a recipe to a specific version
//! - Auto-detect new versions from various providers (GitHub, PyPI, etc.)
//! - Update SHA256 checksums automatically

use fs_err as fs;
use rattler_digest::Sha256Hash;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

use crate::recipe_updater::{RecipeUpdater, RecipeUpdaterError};

/// Errors that can occur during recipe bumping
#[derive(Debug, Error)]
pub enum BumpRecipeError {
    /// Recipe updater error
    #[error("Recipe update failed: {0}")]
    RecipeUpdater(#[from] RecipeUpdaterError),

    /// Failed to parse URL
    #[error("Failed to parse URL: {0}")]
    UrlParse(#[from] url::ParseError),

    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Failed to detect version provider
    #[error("Could not detect version provider from URL: {0}")]
    UnknownProvider(String),

    /// Failed to fetch new version
    #[error("Failed to fetch new version: {0}")]
    VersionFetch(String),

    /// Failed to parse recipe
    #[error("Failed to parse recipe: {0}")]
    RecipeParse(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// No source URL found
    #[error("No source URL found in recipe")]
    NoSourceUrl,

    /// Version not found
    #[error("Version not found in recipe")]
    VersionNotFound,

    /// No new version available
    #[error("No new version available (current: {0})")]
    NoNewVersion(String),
}

/// Supported version providers
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionProvider {
    /// GitHub releases
    GitHub {
        /// The repository owner (user or organization)
        owner: String,
        /// The repository name
        repo: String,
    },
    /// PyPI packages
    PyPI {
        /// The package name on PyPI
        package: String,
    },
    /// crates.io packages
    CratesIo {
        /// The crate name on crates.io
        crate_name: String,
    },
    /// Generic URL (will try HEAD requests with incremented versions)
    Generic {
        /// The URL template with version placeholder
        url_template: String,
    },
}

impl std::fmt::Display for VersionProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionProvider::GitHub { owner, repo } => write!(f, "GitHub ({}/{})", owner, repo),
            VersionProvider::PyPI { package } => write!(f, "PyPI ({})", package),
            VersionProvider::CratesIo { crate_name } => write!(f, "crates.io ({})", crate_name),
            VersionProvider::Generic { url_template } => write!(f, "Generic ({})", url_template),
        }
    }
}

/// GitHub release information
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
}

/// PyPI package information
#[derive(Debug, Deserialize)]
struct PyPIInfo {
    info: PyPIPackageInfo,
}

#[derive(Debug, Deserialize)]
struct PyPIPackageInfo {
    version: String,
}

/// crates.io crate information
#[derive(Debug, Deserialize)]
struct CratesIoInfo {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    max_stable_version: String,
}

/// Recipe context containing version and other variables
#[derive(Debug, Default)]
pub struct RecipeContext {
    /// The version from the context section
    pub version: Option<String>,
    /// The source URL(s)
    pub source_urls: Vec<String>,
    /// The SHA256 checksum(s)
    pub sha256_checksums: Vec<String>,
}

impl RecipeContext {
    /// Parse recipe context from a YAML file
    pub fn from_recipe_file(path: &Path) -> Result<Self, BumpRecipeError> {
        let content = fs::read_to_string(path)?;
        Self::from_yaml_content(&content)
    }

    /// Parse recipe context from YAML content
    pub fn from_yaml_content(content: &str) -> Result<Self, BumpRecipeError> {
        let mut ctx = RecipeContext::default();

        // Extract version from context section
        let version_re = Regex::new(r#"(?m)^\s*version:\s*["']?([^"'\s\n]+)["']?"#)
            .map_err(|e| BumpRecipeError::RecipeParse(e.to_string()))?;

        if let Some(caps) = version_re.captures(content) {
            ctx.version = Some(caps[1].to_string());
        }

        // Extract source URLs (handles both `url:` and `- url:` in lists)
        // Match the entire URL value, which may contain Jinja2 templates
        let url_re = Regex::new(r#"(?m)^\s*-?\s*url:\s*(.+)$"#)
            .map_err(|e| BumpRecipeError::RecipeParse(e.to_string()))?;

        for caps in url_re.captures_iter(content) {
            let url = caps[1].trim();
            // Remove surrounding quotes if present
            let url = url.trim_matches('"').trim_matches('\'');
            ctx.source_urls.push(url.to_string());
        }

        // Extract SHA256 checksums
        let sha_re = Regex::new(r"(?m)^\s*sha256:\s*([a-fA-F0-9]{64})")
            .map_err(|e| BumpRecipeError::RecipeParse(e.to_string()))?;

        for caps in sha_re.captures_iter(content) {
            ctx.sha256_checksums.push(caps[1].to_string());
        }

        Ok(ctx)
    }
}

/// Detect the version provider from a URL
pub fn detect_provider(url: &str) -> Result<VersionProvider, BumpRecipeError> {
    // Try to parse as URL, handling Jinja2 templates
    let clean_url = url
        .replace("${{", "")
        .replace("}}", "")
        .replace("{{ ", "")
        .replace(" }}", "");

    // GitHub patterns
    let github_patterns = [
        // https://github.com/owner/repo/archive/...
        Regex::new(r"github\.com/([^/]+)/([^/]+)/archive").unwrap(),
        // https://github.com/owner/repo/releases/download/...
        Regex::new(r"github\.com/([^/]+)/([^/]+)/releases/download").unwrap(),
        // https://api.github.com/repos/owner/repo/tarball/...
        Regex::new(r"api\.github\.com/repos/([^/]+)/([^/]+)/tarball").unwrap(),
        // Raw githubusercontent
        Regex::new(r"raw\.githubusercontent\.com/([^/]+)/([^/]+)").unwrap(),
    ];

    for pattern in &github_patterns {
        if let Some(caps) = pattern.captures(&clean_url) {
            return Ok(VersionProvider::GitHub {
                owner: caps[1].to_string(),
                repo: caps[2].to_string(),
            });
        }
    }

    // PyPI patterns
    let pypi_patterns = [
        // https://pypi.io/packages/source/p/package/...
        Regex::new(r"pypi\.io/packages/source/./([^/]+)").unwrap(),
        // https://files.pythonhosted.org/packages/source/p/package/...
        Regex::new(r"files\.pythonhosted\.org/packages/source/./([^/]+)").unwrap(),
        // https://pypi.org/packages/source/p/package/...
        Regex::new(r"pypi\.org/packages/source/./([^/]+)").unwrap(),
    ];

    for pattern in &pypi_patterns {
        if let Some(caps) = pattern.captures(&clean_url) {
            return Ok(VersionProvider::PyPI {
                package: caps[1].to_string(),
            });
        }
    }

    // crates.io patterns
    let crates_pattern = Regex::new(r"crates\.io/api/v1/crates/([^/]+)").unwrap();
    if let Some(caps) = crates_pattern.captures(&clean_url) {
        return Ok(VersionProvider::CratesIo {
            crate_name: caps[1].to_string(),
        });
    }

    // Also check for static.crates.io
    let static_crates_pattern = Regex::new(r"static\.crates\.io/crates/([^/]+)").unwrap();
    if let Some(caps) = static_crates_pattern.captures(&clean_url) {
        return Ok(VersionProvider::CratesIo {
            crate_name: caps[1].to_string(),
        });
    }

    // Fall back to generic provider with the URL template
    Ok(VersionProvider::Generic {
        url_template: url.to_string(),
    })
}

/// Fetch the latest version from a provider
pub async fn fetch_latest_version(
    client: &Client,
    provider: &VersionProvider,
    include_prerelease: bool,
) -> Result<String, BumpRecipeError> {
    match provider {
        VersionProvider::GitHub { owner, repo } => {
            fetch_github_latest_version(client, owner, repo, include_prerelease).await
        }
        VersionProvider::PyPI { package } => fetch_pypi_latest_version(client, package).await,
        VersionProvider::CratesIo { crate_name } => {
            fetch_crates_io_latest_version(client, crate_name).await
        }
        VersionProvider::Generic { .. } => Err(BumpRecipeError::VersionFetch(
            "Cannot auto-detect version for generic URLs. Please specify --version manually."
                .to_string(),
        )),
    }
}

async fn fetch_github_latest_version(
    client: &Client,
    owner: &str,
    repo: &str,
    include_prerelease: bool,
) -> Result<String, BumpRecipeError> {
    let url = format!("https://api.github.com/repos/{}/{}/releases", owner, repo);

    let response = client
        .get(&url)
        .header("User-Agent", "rattler-build")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(BumpRecipeError::VersionFetch(format!(
            "GitHub API returned status {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }

    let releases: Vec<GitHubRelease> = response.json().await?;

    // Find the first non-draft, non-prerelease (unless include_prerelease) release
    for release in releases {
        if release.draft {
            continue;
        }
        if !include_prerelease && release.prerelease {
            continue;
        }

        // Strip leading 'v' if present
        let version = release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
            .to_string();

        return Ok(version);
    }

    // If no releases found, try tags endpoint
    let tags_url = format!("https://api.github.com/repos/{}/{}/tags", owner, repo);

    let response = client
        .get(&tags_url)
        .header("User-Agent", "rattler-build")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?;

    if response.status().is_success() {
        #[derive(Deserialize)]
        struct Tag {
            name: String,
        }

        let tags: Vec<Tag> = response.json().await?;
        if let Some(tag) = tags.first() {
            let version = tag.name.strip_prefix('v').unwrap_or(&tag.name).to_string();
            return Ok(version);
        }
    }

    Err(BumpRecipeError::VersionFetch(
        "No releases or tags found".to_string(),
    ))
}

async fn fetch_pypi_latest_version(
    client: &Client,
    package: &str,
) -> Result<String, BumpRecipeError> {
    let url = format!("https://pypi.org/pypi/{}/json", package);

    let response = client
        .get(&url)
        .header("User-Agent", "rattler-build")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(BumpRecipeError::VersionFetch(format!(
            "PyPI API returned status {}",
            response.status()
        )));
    }

    let info: PyPIInfo = response.json().await?;
    Ok(info.info.version)
}

async fn fetch_crates_io_latest_version(
    client: &Client,
    crate_name: &str,
) -> Result<String, BumpRecipeError> {
    let url = format!("https://crates.io/api/v1/crates/{}", crate_name);

    let response = client
        .get(&url)
        .header("User-Agent", "rattler-build")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(BumpRecipeError::VersionFetch(format!(
            "crates.io API returned status {}",
            response.status()
        )));
    }

    let info: CratesIoInfo = response.json().await?;
    Ok(info.crate_info.max_stable_version)
}

/// Fetch SHA256 checksum for a URL
pub async fn fetch_sha256(client: &Client, url: &str) -> Result<Sha256Hash, BumpRecipeError> {
    let response = client
        .get(url)
        .header("User-Agent", "rattler-build")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(BumpRecipeError::VersionFetch(format!(
            "Failed to download source (HTTP {}): {}",
            response.status(),
            url
        )));
    }

    let bytes = response.bytes().await?;

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hasher.finalize())
}

/// Build a URL with a specific version by replacing the version placeholder
pub fn build_url_with_version(url_template: &str, version: &str) -> String {
    // Replace Jinja2 version placeholders
    url_template
        .replace("${{ version }}", version)
        .replace("${{version}}", version)
        .replace("{{ version }}", version)
        .replace("{{version}}", version)
}

/// Result of a recipe bump operation
#[derive(Debug)]
pub struct BumpResult {
    /// The old version
    pub old_version: String,
    /// The new version
    pub new_version: String,
    /// The old SHA256 checksums
    pub old_sha256: Vec<String>,
    /// The new SHA256 checksums
    pub new_sha256: Vec<String>,
    /// The detected provider
    pub provider: Option<VersionProvider>,
}

/// Bump a recipe to a new version
pub async fn bump_recipe(
    recipe_path: &Path,
    new_version: Option<&str>,
    client: &Client,
    include_prerelease: bool,
    dry_run: bool,
) -> Result<BumpResult, BumpRecipeError> {
    // Parse the recipe to get current version and URLs
    let ctx = RecipeContext::from_recipe_file(recipe_path)?;

    let old_version = ctx
        .version
        .clone()
        .ok_or(BumpRecipeError::VersionNotFound)?;

    // Detect provider from the first URL
    let source_url = ctx
        .source_urls
        .first()
        .ok_or(BumpRecipeError::NoSourceUrl)?;

    let provider = detect_provider(source_url)?;
    tracing::debug!("Detected version provider: {}", provider);

    // Determine the new version
    let new_version = if let Some(v) = new_version {
        v.to_string()
    } else {
        tracing::debug!("Auto-detecting latest version...");
        fetch_latest_version(client, &provider, include_prerelease).await?
    };

    // Check if version is actually new
    if new_version == old_version {
        return Err(BumpRecipeError::NoNewVersion(old_version));
    }

    tracing::info!("Bumping {} -> {}", old_version, new_version);

    // Build URLs with new version and fetch SHA256
    let mut new_sha256s = Vec::new();
    let mut new_sha256_hashes = Vec::new();

    for url_template in &ctx.source_urls {
        let new_url = build_url_with_version(url_template, &new_version);
        tracing::debug!("Resolved URL: {}", new_url);

        tracing::info!("Fetching {}", new_url);
        let sha256 = fetch_sha256(client, &new_url).await?;
        tracing::debug!("Fetched SHA256: {:x}", sha256);

        new_sha256s.push(format!("{:x}", sha256));
        new_sha256_hashes.push(sha256);
    }

    if !dry_run {
        // Load and update the recipe
        let mut updater = RecipeUpdater::load(recipe_path)?;

        // Update version
        updater.update_version(&new_version)?;

        // Update SHA256 checksums in order (handles Jinja2 template URLs)
        updater.update_sha256_in_order(&new_sha256_hashes)?;

        // Save the updated recipe
        updater.save()?;

        tracing::info!("Updated {}", recipe_path.display());
    } else {
        tracing::info!("Dry run - no changes written");
    }

    Ok(BumpResult {
        old_version,
        new_version,
        old_sha256: ctx.sha256_checksums,
        new_sha256: new_sha256s,
        provider: Some(provider),
    })
}

/// Check if a newer version is available without modifying the recipe
pub async fn check_for_updates(
    recipe_path: &Path,
    client: &Client,
    include_prerelease: bool,
) -> Result<Option<String>, BumpRecipeError> {
    let ctx = RecipeContext::from_recipe_file(recipe_path)?;

    let current_version = ctx
        .version
        .clone()
        .ok_or(BumpRecipeError::VersionNotFound)?;

    let source_url = ctx
        .source_urls
        .first()
        .ok_or(BumpRecipeError::NoSourceUrl)?;

    let provider = detect_provider(source_url)?;

    match fetch_latest_version(client, &provider, include_prerelease).await {
        Ok(latest_version) => {
            if latest_version != current_version {
                Ok(Some(latest_version))
            } else {
                Ok(None)
            }
        }
        Err(e) => {
            tracing::warn!("Could not fetch latest version: {}", e);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_github_provider() {
        let test_cases = vec![
            (
                "https://github.com/owner/repo/archive/v1.0.0.tar.gz",
                VersionProvider::GitHub {
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                },
            ),
            (
                "https://github.com/owner/repo/releases/download/v1.0.0/file.tar.gz",
                VersionProvider::GitHub {
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                },
            ),
            (
                "https://api.github.com/repos/owner/repo/tarball/v1.0.0",
                VersionProvider::GitHub {
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                },
            ),
        ];

        for (url, expected) in test_cases {
            let provider = detect_provider(url).unwrap();
            assert_eq!(provider, expected, "Failed for URL: {}", url);
        }
    }

    #[test]
    fn test_detect_pypi_provider() {
        let test_cases = vec![
            (
                "https://pypi.io/packages/source/p/package/package-1.0.0.tar.gz",
                VersionProvider::PyPI {
                    package: "package".to_string(),
                },
            ),
            (
                "https://files.pythonhosted.org/packages/source/r/requests/requests-2.28.0.tar.gz",
                VersionProvider::PyPI {
                    package: "requests".to_string(),
                },
            ),
        ];

        for (url, expected) in test_cases {
            let provider = detect_provider(url).unwrap();
            assert_eq!(provider, expected, "Failed for URL: {}", url);
        }
    }

    #[test]
    fn test_detect_crates_io_provider() {
        let url = "https://crates.io/api/v1/crates/serde/1.0.0/download";
        let provider = detect_provider(url).unwrap();
        assert_eq!(
            provider,
            VersionProvider::CratesIo {
                crate_name: "serde".to_string()
            }
        );
    }

    #[test]
    fn test_detect_generic_provider() {
        let url = "https://example.com/files/package-1.0.0.tar.gz";
        let provider = detect_provider(url).unwrap();
        assert!(matches!(provider, VersionProvider::Generic { .. }));
    }

    #[test]
    fn test_build_url_with_version() {
        let test_cases = vec![
            (
                "https://github.com/owner/repo/archive/v${{ version }}.tar.gz",
                "2.0.0",
                "https://github.com/owner/repo/archive/v2.0.0.tar.gz",
            ),
            (
                "https://pypi.io/packages/source/p/pkg/pkg-${{version}}.tar.gz",
                "1.2.3",
                "https://pypi.io/packages/source/p/pkg/pkg-1.2.3.tar.gz",
            ),
        ];

        for (template, version, expected) in test_cases {
            let result = build_url_with_version(template, version);
            assert_eq!(result, expected, "Failed for template: {}", template);
        }
    }

    #[test]
    fn test_parse_recipe_context() {
        let yaml = r#"
context:
  version: "1.2.3"

package:
  name: test-package
  version: ${{ version }}

source:
  url: https://github.com/owner/repo/archive/v${{ version }}.tar.gz
  sha256: 1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef

build:
  number: 0
"#;

        let ctx = RecipeContext::from_yaml_content(yaml).unwrap();
        assert_eq!(ctx.version, Some("1.2.3".to_string()));
        assert_eq!(ctx.source_urls.len(), 1);
        assert!(ctx.source_urls[0].contains("github.com"));
        assert_eq!(ctx.sha256_checksums.len(), 1);
    }

    #[test]
    fn test_parse_recipe_context_with_jinja_version() {
        let yaml = r#"
context:
  version: "2.0.0"

package:
  name: example
  version: ${{ version }}

source:
  - url: https://example.com/pkg-${{ version }}.tar.gz
    sha256: abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890
  - url: https://example.com/extra-${{ version }}.tar.gz
    sha256: 1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef
"#;

        let ctx = RecipeContext::from_yaml_content(yaml).unwrap();
        assert_eq!(ctx.version, Some("2.0.0".to_string()));
        assert_eq!(ctx.source_urls.len(), 2);
        assert_eq!(ctx.sha256_checksums.len(), 2);
    }
}
