//! Recipe bumping functionality for updating versions and checksums.
//!
//! This module provides functionality to:
//! - Bump a recipe to a specific version
//! - Auto-detect new versions from various providers (GitHub, PyPI, etc.)
//! - Update SHA256 checksums automatically

use fs_err as fs;
use indexmap::IndexMap;
use minijinja::Value;
use rattler_conda_types::Platform;
use rattler_digest::Sha256Hash;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

use crate::recipe::jinja::Jinja;
use crate::selectors::SelectorConfig;

/// Errors that can occur during recipe bumping
#[derive(Debug, Error)]
pub enum BumpRecipeError {
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
    /// The version from the context section (the raw, literal value)
    pub version: Option<String>,
    /// The build number (from context or build.number)
    pub build_number: Option<BuildNumber>,
    /// The source URL(s) - these are the raw templates
    pub source_urls: Vec<String>,
    /// The SHA256 checksum(s)
    pub sha256_checksums: Vec<String>,
    /// Raw context templates (key -> raw value, may contain Jinja expressions)
    pub raw_context: IndexMap<String, String>,
}

/// Location and value of the build number
#[derive(Debug, Clone)]
pub struct BuildNumber {
    /// The current build number value
    pub value: u64,
    /// Where the build number is defined
    pub location: BuildNumberLocation,
}

/// Where the build number is defined in the recipe
#[derive(Debug, Clone, PartialEq)]
pub enum BuildNumberLocation {
    /// In context section as "number"
    ContextNumber,
    /// In context section as "build_number"
    ContextBuildNumber,
    /// In build.number
    BuildSection,
}

impl RecipeContext {
    /// Parse recipe context from a YAML file
    pub fn from_recipe_file(path: &Path) -> Result<Self, BumpRecipeError> {
        let content = fs::read_to_string(path)?;
        Self::from_yaml_content(&content)
    }

    /// Parse recipe context from YAML content using the YAML structure
    pub fn from_yaml_content(content: &str) -> Result<Self, BumpRecipeError> {
        let mut ctx = RecipeContext::default();

        // Parse as raw YAML (no Jinja rendering)
        let yaml: YamlValue = serde_yaml::from_str(content)
            .map_err(|e| BumpRecipeError::RecipeParse(format!("YAML parse error: {}", e)))?;

        // Extract context variables (store ALL values, including Jinja templates)
        if let Some(context) = yaml.get("context").and_then(|v| v.as_mapping()) {
            for (key, value) in context {
                if let Some(key_str) = key.as_str() {
                    // Get the string value, handling both quoted and unquoted
                    let value_str = match value {
                        YamlValue::String(s) => s.clone(),
                        YamlValue::Number(n) => n.to_string(),
                        YamlValue::Bool(b) => b.to_string(),
                        _ => continue, // Skip complex values
                    };

                    // Store the raw value
                    ctx.raw_context
                        .insert(key_str.to_string(), value_str.clone());

                    // Only set version if it's a literal value (not a Jinja template)
                    if key_str == "version"
                        && !value_str.starts_with('$')
                        && !value_str.starts_with('{')
                    {
                        ctx.version = Some(value_str);
                    }

                    // Check for build number in context (number or build_number)
                    if (key_str == "number" || key_str == "build_number")
                        && let Some(num) = value.as_u64()
                    {
                        let location = if key_str == "number" {
                            BuildNumberLocation::ContextNumber
                        } else {
                            BuildNumberLocation::ContextBuildNumber
                        };
                        ctx.build_number = Some(BuildNumber {
                            value: num,
                            location,
                        });
                    }
                }
            }
        }

        // Extract build.number if not found in context
        if ctx.build_number.is_none()
            && let Some(build) = yaml.get("build").and_then(|v| v.as_mapping())
            && let Some(num) = build.get("number").and_then(|v| v.as_u64())
        {
            ctx.build_number = Some(BuildNumber {
                value: num,
                location: BuildNumberLocation::BuildSection,
            });
        }

        // Extract source URLs from the source section
        Self::extract_sources(&yaml, &mut ctx)?;

        Ok(ctx)
    }

    /// Extract source URLs and SHA256 checksums from the YAML structure
    fn extract_sources(yaml: &YamlValue, ctx: &mut RecipeContext) -> Result<(), BumpRecipeError> {
        let source = match yaml.get("source") {
            Some(s) => s,
            None => return Ok(()), // No source section
        };

        // Handle both single source and list of sources
        let sources: Vec<&YamlValue> = if source.is_sequence() {
            source.as_sequence().unwrap().iter().collect()
        } else {
            vec![source]
        };

        for src in sources {
            // Extract URL(s) - can be a single string or a list
            if let Some(url) = src.get("url") {
                match url {
                    YamlValue::String(s) => {
                        ctx.source_urls.push(s.clone());
                    }
                    YamlValue::Sequence(urls) => {
                        for u in urls {
                            if let YamlValue::String(s) = u {
                                ctx.source_urls.push(s.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Extract SHA256 checksum
            if let Some(YamlValue::String(sha)) = src.get("sha256") {
                ctx.sha256_checksums.push(sha.clone());
            }
        }

        Ok(())
    }
}

/// Detect the version provider from a rendered URL
///
/// This function expects a fully-rendered URL (Jinja templates already resolved).
pub fn detect_provider(url: &str) -> Result<VersionProvider, BumpRecipeError> {
    // GitHub patterns
    if let Some(caps) = Regex::new(r"github\.com/([^/]+)/([^/]+)/archive")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::GitHub {
            owner: caps[1].to_string(),
            repo: caps[2].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"github\.com/([^/]+)/([^/]+)/releases/download")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::GitHub {
            owner: caps[1].to_string(),
            repo: caps[2].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"api\.github\.com/repos/([^/]+)/([^/]+)/tarball")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::GitHub {
            owner: caps[1].to_string(),
            repo: caps[2].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"raw\.githubusercontent\.com/([^/]+)/([^/]+)")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::GitHub {
            owner: caps[1].to_string(),
            repo: caps[2].to_string(),
        });
    }

    // PyPI patterns
    if let Some(caps) = Regex::new(r"pypi\.io/packages/source/./([^/]+)")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::PyPI {
            package: caps[1].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"files\.pythonhosted\.org/packages/source/./([^/]+)")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::PyPI {
            package: caps[1].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"pypi\.org/packages/source/./([^/]+)")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::PyPI {
            package: caps[1].to_string(),
        });
    }

    // crates.io patterns
    if let Some(caps) = Regex::new(r"crates\.io/api/v1/crates/([^/]+)")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::CratesIo {
            crate_name: caps[1].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"static\.crates\.io/crates/([^/]+)")
        .unwrap()
        .captures(url)
    {
        return Ok(VersionProvider::CratesIo {
            crate_name: caps[1].to_string(),
        });
    }

    // Fall back to generic provider
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

/// Reset the build number to 0 in the recipe content
///
/// Uses regex to find and replace the build number while preserving formatting.
fn reset_build_number(content: &str, build_num: &BuildNumber) -> String {
    let pattern = match build_num.location {
        BuildNumberLocation::ContextNumber => {
            // Match "number: <value>" in context section
            format!(r"(\s+number:\s*){}", build_num.value)
        }
        BuildNumberLocation::ContextBuildNumber => {
            // Match "build_number: <value>" in context section
            format!(r"(\s+build_number:\s*){}", build_num.value)
        }
        BuildNumberLocation::BuildSection => {
            // Match "number: <value>" in build section
            format!(r"(\s+number:\s*){}", build_num.value)
        }
    };

    Regex::new(&pattern)
        .map(|re| re.replace(content, "${1}0").to_string())
        .unwrap_or_else(|_| content.to_string())
}

/// Build a URL with a specific version using proper Jinja rendering
///
/// This function uses the real Jinja rendering engine with the full context
/// from the recipe, ensuring all variables (not just version) are properly
/// substituted. It supports context variables that depend on other variables,
/// such as `version_with_underscore: ${{ version | replace('.', '_') }}`.
pub fn build_url_with_version(
    url_template: &str,
    version: &str,
    raw_context: &IndexMap<String, String>,
) -> Result<String, BumpRecipeError> {
    // Create a SelectorConfig with default platform settings
    let selector_config = SelectorConfig {
        target_platform: Platform::current(),
        host_platform: Platform::current(),
        build_platform: Platform::current(),
        hash: None,
        variant: BTreeMap::new(),
        experimental: false,
        allow_undefined: true,
        recipe_path: None,
    };

    // Create Jinja instance
    let mut jinja = Jinja::new(selector_config);

    // First, set the version (so other context vars can reference it)
    jinja
        .context_mut()
        .insert("version".to_string(), Value::from(version));

    // Render context variables in order, so templates can reference earlier vars
    // This handles cases like: version_with_underscore: ${{ version | replace('.', '_') }}
    for (key, raw_value) in raw_context {
        // Skip version since we already set it with the new value
        if key == "version" {
            continue;
        }

        // Try to render the value (it might be a Jinja template)
        let rendered_value = if raw_value.contains("${{") || raw_value.contains("{{") {
            jinja
                .render_str(raw_value)
                .unwrap_or_else(|_| raw_value.clone())
        } else {
            raw_value.clone()
        };

        jinja
            .context_mut()
            .insert(key.clone(), Value::from(rendered_value));
    }

    // Render the URL template
    jinja
        .render_str(url_template)
        .map_err(|e| BumpRecipeError::RecipeParse(format!("Failed to render URL: {}", e)))
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
    keep_build_number: bool,
) -> Result<BumpResult, BumpRecipeError> {
    // Parse the recipe to get current version and URLs
    let ctx = RecipeContext::from_recipe_file(recipe_path)?;

    let old_version = ctx
        .version
        .clone()
        .ok_or(BumpRecipeError::VersionNotFound)?;

    // Detect provider from the first URL (render it first to substitute variables)
    let source_url_template = ctx
        .source_urls
        .first()
        .ok_or(BumpRecipeError::NoSourceUrl)?;

    // Render the URL with the current context to detect the provider
    let rendered_url = build_url_with_version(source_url_template, &old_version, &ctx.raw_context)?;
    let provider = detect_provider(&rendered_url)?;
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

    for url_template in &ctx.source_urls {
        let new_url = build_url_with_version(url_template, &new_version, &ctx.raw_context)?;
        tracing::debug!("Resolved URL: {}", new_url);

        tracing::info!("Fetching {}", new_url);
        let sha256 = fetch_sha256(client, &new_url).await?;
        let sha256_str = format!("{:x}", sha256);
        tracing::debug!("Fetched SHA256: {}", sha256_str);

        new_sha256s.push(sha256_str);
    }

    if !dry_run {
        // Read the recipe file content
        let mut content = fs::read_to_string(recipe_path)?;

        // Simple string replacement: replace old version with new version
        // This preserves all formatting, comments, etc.
        content = content.replace(&old_version, &new_version);

        // Replace SHA256 checksums in order
        for (old_sha, new_sha) in ctx.sha256_checksums.iter().zip(new_sha256s.iter()) {
            content = content.replacen(old_sha, new_sha, 1);
        }

        // Reset build number to 0 (unless --keep-build-number is set)
        if !keep_build_number
            && let Some(build_num) = &ctx.build_number
            && build_num.value != 0
        {
            content = reset_build_number(&content, build_num);
            tracing::debug!("Reset build number from {} to 0", build_num.value);
        }

        // Write the updated content back
        fs::write(recipe_path, &content)?;

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

    let source_url_template = ctx
        .source_urls
        .first()
        .ok_or(BumpRecipeError::NoSourceUrl)?;

    // Render the URL with the current context to detect the provider
    let rendered_url =
        build_url_with_version(source_url_template, &current_version, &ctx.raw_context)?;
    let provider = detect_provider(&rendered_url)?;

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
            let context = IndexMap::new();
            let result = build_url_with_version(template, version, &context).unwrap();
            assert_eq!(result, expected, "Failed for template: {}", template);
        }
    }

    #[test]
    fn test_build_url_with_full_context() {
        // Test that other context variables are also rendered
        let mut context = IndexMap::new();
        context.insert("name".to_string(), "mypackage".to_string());
        context.insert("version".to_string(), "1.0.0".to_string());

        let template = "https://example.com/${{ name }}/${{ name }}-${{ version }}.tar.gz";
        let result = build_url_with_version(template, "2.0.0", &context).unwrap();

        // The version should be overridden to 2.0.0, but name should use context value
        assert_eq!(
            result,
            "https://example.com/mypackage/mypackage-2.0.0.tar.gz"
        );
    }

    #[test]
    fn test_build_url_with_derived_context() {
        // Test context variables that depend on other variables (like version_underscore)
        let mut context = IndexMap::new();
        context.insert("name".to_string(), "mypackage".to_string());
        context.insert("version".to_string(), "1.0.0".to_string());
        context.insert(
            "version_underscore".to_string(),
            "${{ version | replace('.', '_') }}".to_string(),
        );

        let template = "https://example.com/${{ name }}-${{ version_underscore }}.tar.gz";
        let result = build_url_with_version(template, "2.0.0", &context).unwrap();

        // The version_underscore should be derived from the new version (2.0.0 -> 2_0_0)
        assert_eq!(result, "https://example.com/mypackage-2_0_0.tar.gz");
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

    #[test]
    fn test_parse_build_number_from_build_section() {
        let yaml = r#"
context:
  version: "1.0.0"

build:
  number: 5
"#;

        let ctx = RecipeContext::from_yaml_content(yaml).unwrap();
        assert!(ctx.build_number.is_some());
        let build_num = ctx.build_number.unwrap();
        assert_eq!(build_num.value, 5);
        assert_eq!(build_num.location, BuildNumberLocation::BuildSection);
    }

    #[test]
    fn test_parse_build_number_from_context() {
        let yaml = r#"
context:
  version: "1.0.0"
  build_number: 3

build:
  number: ${{ build_number }}
"#;

        let ctx = RecipeContext::from_yaml_content(yaml).unwrap();
        assert!(ctx.build_number.is_some());
        let build_num = ctx.build_number.unwrap();
        assert_eq!(build_num.value, 3);
        assert_eq!(build_num.location, BuildNumberLocation::ContextBuildNumber);
    }

    #[test]
    fn test_reset_build_number() {
        let content = r#"
context:
  version: "1.0.0"

build:
  number: 5
"#;

        let build_num = BuildNumber {
            value: 5,
            location: BuildNumberLocation::BuildSection,
        };

        let result = reset_build_number(content, &build_num);
        assert!(result.contains("number: 0"));
        assert!(!result.contains("number: 5"));
    }

    #[test]
    fn test_reset_build_number_in_context() {
        let content = r#"
context:
  version: "1.0.0"
  build_number: 7

build:
  number: ${{ build_number }}
"#;

        let build_num = BuildNumber {
            value: 7,
            location: BuildNumberLocation::ContextBuildNumber,
        };

        let result = reset_build_number(content, &build_num);
        assert!(result.contains("build_number: 0"));
        assert!(!result.contains("build_number: 7"));
        // The ${{ build_number }} reference should be preserved
        assert!(result.contains("${{ build_number }}"));
    }
}
