//! Main source cache implementation

use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::{
    builder::ProgressHandler,
    error::CacheError,
    index::{CacheEntry, CacheIndex, SourceType},
    lock::LockManager,
    source::{AttestationVerification, Checksum, GitSource, Source, UrlSource},
};
use rattler_build_networking::BaseClient;
use rattler_git::resolver::GitResolver;

/// Auto-derive a PyPI provenance URL from a PyPI source URL.
///
/// Detects URLs from `pypi.io` and `files.pythonhosted.org` and constructs
/// the corresponding `https://pypi.org/integrity/{project}/{version}/{filename}/provenance` URL.
fn derive_pypi_provenance_url(source_url: &url::Url) -> Option<url::Url> {
    let host = source_url.host_str()?;
    if host != "pypi.io" && host != "files.pythonhosted.org" {
        return None;
    }

    // PyPI URLs look like:
    // https://pypi.io/packages/source/f/flask/flask-3.1.1.tar.gz
    // https://files.pythonhosted.org/packages/source/f/flask/flask-3.1.1.tar.gz
    // or with hashes:
    // https://files.pythonhosted.org/packages/ab/cd/.../flask-3.1.1.tar.gz
    let path = source_url.path();
    let filename = path.rsplit('/').next()?;

    // Extract project name and version from filename
    // Filenames are typically: {project}-{version}.tar.gz or {project}-{version}.whl etc.
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".tar.bz2"))
        .or_else(|| filename.strip_suffix(".zip"))
        .or_else(|| filename.strip_suffix(".whl"))?;

    // Split on the last '-' to separate project from version
    let (project, version) = stem.rsplit_once('-')?;

    // Normalize project name (PEP 503: replace [-_.] with -)
    let normalized_project = project.to_lowercase().replace(['-', '_', '.'], "-");

    let provenance_url = format!(
        "https://pypi.org/integrity/{}/{}/{}/provenance",
        normalized_project, version, filename
    );
    url::Url::parse(&provenance_url).ok()
}

/// Result of parsing an attestation response.
struct ParsedAttestations {
    bundles: Vec<sigstore_types::Bundle>,
    /// Whether these bundles were converted from PyPI PEP 740 provenance format.
    /// PyPI-converted bundles lack canonicalized rekor bodies so transparency log
    /// verification must be skipped.
    from_pypi: bool,
}

/// Parse an attestation response into one or more sigstore bundles.
///
/// Handles two formats:
/// 1. **Standard sigstore bundle** (`.sigstore.json`): has a `mediaType` field,
///    parsed directly via `Bundle::from_json`.
/// 2. **PyPI PEP 740 provenance response**: has an `attestation_bundles` array,
///    each containing `attestations` that are converted to sigstore bundles.
fn parse_attestation_response(json_str: &str) -> Result<ParsedAttestations, CacheError> {
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| CacheError::InvalidAttestationBundle(format!("Invalid JSON: {}", e)))?;

    // If it has a "mediaType" field, it's a standard sigstore bundle
    if value.get("mediaType").is_some() {
        let bundle = sigstore_types::Bundle::from_json(json_str).map_err(|e| {
            CacheError::InvalidAttestationBundle(format!("Failed to parse sigstore bundle: {}", e))
        })?;
        return Ok(ParsedAttestations {
            bundles: vec![bundle],
            from_pypi: false,
        });
    }

    // Otherwise, try to parse as a PyPI provenance response
    if let Some(attestation_bundles) = value.get("attestation_bundles").and_then(|v| v.as_array()) {
        let mut bundles = Vec::new();
        for ab in attestation_bundles {
            if let Some(attestations) = ab.get("attestations").and_then(|v| v.as_array()) {
                for attestation in attestations {
                    let bundle = convert_pypi_attestation_to_bundle(attestation)?;
                    bundles.push(bundle);
                }
            }
        }
        if bundles.is_empty() {
            return Err(CacheError::InvalidAttestationBundle(
                "PyPI provenance response contains no attestations".to_string(),
            ));
        }
        return Ok(ParsedAttestations {
            bundles,
            from_pypi: true,
        });
    }

    Err(CacheError::InvalidAttestationBundle(
        "Unrecognized attestation format: expected sigstore bundle or PyPI provenance response"
            .to_string(),
    ))
}

/// Convert a PyPI PEP 740 attestation object to a sigstore v0.3 bundle.
///
/// PyPI attestation format:
/// ```json
/// {
///   "version": 1,
///   "verification_material": {
///     "certificate": "<base64(DER)>",
///     "transparency_entries": [{ ... }]
///   },
///   "envelope": {
///     "statement": "<base64(in-toto JSON)>",
///     "signature": "<base64(sig)>"
///   }
/// }
/// ```
fn convert_pypi_attestation_to_bundle(
    attestation: &serde_json::Value,
) -> Result<sigstore_types::Bundle, CacheError> {
    let err = |msg: &str| CacheError::InvalidAttestationBundle(msg.to_string());

    let envelope = attestation
        .get("envelope")
        .ok_or_else(|| err("missing 'envelope'"))?;
    let verification_material = attestation
        .get("verification_material")
        .ok_or_else(|| err("missing 'verification_material'"))?;

    let statement = envelope
        .get("statement")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("missing 'envelope.statement'"))?;
    let signature = envelope
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("missing 'envelope.signature'"))?;
    let certificate = verification_material
        .get("certificate")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("missing 'verification_material.certificate'"))?;

    // PyPI transparency_entries already use the sigstore bundle v0.3 JSON format
    // (camelCase field names, same structure), so pass them through directly.
    let tlog_entries: Vec<serde_json::Value> = verification_material
        .get("transparency_entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Construct a sigstore v0.3 bundle JSON
    let bundle_json = serde_json::json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {
            "certificate": { "rawBytes": certificate },
            "tlogEntries": tlog_entries,
            "timestampVerificationData": {}
        },
        "dsseEnvelope": {
            "payload": statement,
            "payloadType": "application/vnd.in-toto+json",
            "signatures": [{ "sig": signature }]
        }
    });

    let bundle_str = serde_json::to_string(&bundle_json)
        .map_err(|e| err(&format!("Failed to serialize bundle: {}", e)))?;

    sigstore_types::Bundle::from_json(&bundle_str)
        .map_err(|e| err(&format!("Failed to parse converted bundle: {}", e)))
}

/// Result of fetching a source from the cache
#[derive(Debug, Clone)]
pub struct SourceResult {
    /// Path to the fetched source
    pub path: PathBuf,
    /// For git sources, the resolved commit SHA
    pub git_commit: Option<String>,
}

/// The main source cache that handles Git, URL, and Path sources
pub struct SourceCache {
    cache_dir: PathBuf,
    index: CacheIndex,
    lock_manager: LockManager,
    client: BaseClient,
    git_resolver: GitResolver,
    progress_handler: Option<Box<dyn ProgressHandler>>,
}

impl SourceCache {
    /// Create a new source cache
    pub async fn new(
        cache_dir: PathBuf,
        client: BaseClient,
        progress_handler: Option<Box<dyn ProgressHandler>>,
    ) -> Result<Self, CacheError> {
        let index = CacheIndex::new(cache_dir.clone()).await?;
        let lock_manager = LockManager::new(&cache_dir).await?;

        let cache = Self {
            cache_dir,
            index,
            lock_manager,
            client,
            git_resolver: GitResolver::default(),
            progress_handler,
        };

        Ok(cache)
    }

    /// Get a source from the cache or fetch it if not present
    pub async fn get_source(&self, source: &Source) -> Result<SourceResult, CacheError> {
        match source {
            Source::Git(git_source) => self.get_git_source(git_source).await,
            Source::Url(url_source) => self.get_url_source(url_source).await,
            Source::Path(path) => {
                // Path sources are not cached, just return the path
                Ok(SourceResult {
                    path: path.clone(),
                    git_commit: None,
                })
            }
        }
    }

    /// Get a Git source from the cache or clone it if not present
    async fn get_git_source(&self, source: &GitSource) -> Result<SourceResult, CacheError> {
        let git_url = source.to_git_url();
        let key =
            CacheIndex::generate_git_cache_key(source.url.as_ref(), &source.reference.to_string());

        // Acquire lock for this cache entry
        let _lock = self.lock_manager.acquire(&key).await?;

        // Check if we have it in cache
        if let Some(entry) = self.index.get(&key).await {
            let cache_path = self.index.get_cache_path(&entry);
            if cache_path.exists() {
                // Update access time
                self.index.touch(&key).await?;
                tracing::info!("Found git source in cache: {}", cache_path.display());
                return Ok(SourceResult {
                    path: cache_path,
                    git_commit: entry.git_commit.clone(),
                });
            }
        }

        // Use rattler_git to fetch the repository
        tracing::info!("Fetching git repository: {}", git_url);
        let git_cache = self.cache_dir.join("git");
        fs_err::tokio::create_dir_all(&git_cache).await?;

        let fetch_result = self
            .git_resolver
            .fetch(
                git_url.clone(),
                self.client.get_client().clone(),
                git_cache,
                None,
            )
            .await
            .map_err(|e| CacheError::Git(format!("Git fetch failed: {}", e)))?;

        let repo_path = fetch_result.path().to_path_buf();
        let commit_hash = fetch_result.commit().to_string();

        // Verify expected commit if specified
        if let Some(expected) = &source.expected_commit {
            if commit_hash != *expected {
                return Err(CacheError::GitCommitMismatch {
                    expected: expected.clone(),
                    actual: commit_hash,
                    rev: source.reference.to_string(),
                });
            }
            tracing::info!("Verified expected commit: {}", expected);
        }

        // Handle submodules if needed (defaults to true)
        if source.submodules {
            self.git_submodule_update(&repo_path).await?;
        }

        // Handle LFS if needed
        if source.lfs {
            self.git_lfs_pull(&repo_path, &source.url).await?;
        }

        // Create cache entry
        let entry = CacheEntry {
            source_type: SourceType::Git,
            url: source.url.to_string(),
            checksum: None,
            checksum_type: None,
            actual_filename: None,
            git_commit: Some(commit_hash.clone()),
            git_rev: Some(source.reference.to_string()),
            cache_path: repo_path
                .strip_prefix(&self.cache_dir)
                .unwrap_or(&repo_path)
                .to_path_buf(),
            extracted_path: None,
            last_accessed: chrono::Utc::now(),
            created: chrono::Utc::now(),
            lock_file: Some(_lock.path().to_path_buf()),
            attestation_verified: false,
        };

        self.index.insert(key, entry).await?;

        Ok(SourceResult {
            path: repo_path,
            git_commit: Some(commit_hash),
        })
    }

    /// Initialize and recursively update git submodules
    async fn git_submodule_update(&self, repo_path: &Path) -> Result<(), CacheError> {
        let output = tokio::process::Command::new("git")
            .current_dir(repo_path)
            .arg("submodule")
            .arg("update")
            .arg("--init")
            .arg("--recursive")
            .output()
            .await
            .map_err(|e| CacheError::Git(format!("Failed to update git submodules: {}", e)))?;

        if !output.status.success() {
            return Err(CacheError::Git(format!(
                "Git submodule update failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Pull LFS files for a git repository
    async fn git_lfs_pull(
        &self,
        repo_path: &Path,
        source_url: &url::Url,
    ) -> Result<(), CacheError> {
        let output = tokio::process::Command::new("git")
            .current_dir(repo_path)
            .arg("lfs")
            .arg("version")
            .output()
            .await
            .map_err(|e| CacheError::Git(format!("git-lfs not installed: {}", e)))?;

        if !output.status.success() {
            return Err(CacheError::Git("git-lfs not installed".to_string()));
        }

        // Point git-lfs at the original source via `lfs.url` config.
        // The checkout's origin remote points to the local bare database
        // (set by `git clone --local`), which doesn't have LFS objects.
        // We use lfs.url rather than modifying origin so only LFS is affected.
        // For file:// URLs, convert to a plain local path because git-lfs does
        // not handle the file:// protocol correctly (especially on Windows).
        let lfs_url = if source_url.scheme() == "file" {
            source_url
                .to_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| source_url.as_str().to_string())
        } else {
            source_url.as_str().to_string()
        };

        let output = tokio::process::Command::new("git")
            .current_dir(repo_path)
            .arg("config")
            .arg("lfs.url")
            .arg(&lfs_url)
            .output()
            .await
            .map_err(|e| CacheError::Git(format!("Failed to configure lfs.url: {}", e)))?;

        if !output.status.success() {
            return Err(CacheError::Git(format!(
                "Failed to configure lfs.url: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Fetch LFS files from the configured lfs.url.
        let output = tokio::process::Command::new("git")
            .current_dir(repo_path)
            .arg("lfs")
            .arg("fetch")
            .output()
            .await
            .map_err(|e| CacheError::Git(format!("Failed to fetch LFS files: {}", e)))?;

        if !output.status.success() {
            return Err(CacheError::Git(format!(
                "LFS fetch failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Checkout LFS files
        let output = tokio::process::Command::new("git")
            .current_dir(repo_path)
            .arg("lfs")
            .arg("checkout")
            .output()
            .await
            .map_err(|e| CacheError::Git(format!("Failed to checkout LFS files: {}", e)))?;

        if !output.status.success() {
            return Err(CacheError::Git(format!(
                "LFS checkout failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Get a URL source from the cache or download it if not present
    async fn get_url_source(&self, source: &UrlSource) -> Result<SourceResult, CacheError> {
        // Try each URL until one succeeds
        let mut last_error = None;

        for url in &source.urls {
            match self
                .try_url(
                    url,
                    &source.checksums,
                    source.file_name.as_deref(),
                    source.attestation.as_ref(),
                )
                .await
            {
                Ok(path) => {
                    return Ok(SourceResult {
                        path,
                        git_commit: None,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch from {}: {}", url, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| CacheError::Other("No URLs provided".to_string())))
    }

    /// Try to get a single URL from cache or download it
    async fn try_url(
        &self,
        url: &url::Url,
        checksums: &[Checksum],
        file_name: Option<&str>,
        attestation: Option<&AttestationVerification>,
    ) -> Result<PathBuf, CacheError> {
        let key = CacheIndex::generate_cache_key(url, checksums);

        // Acquire lock for this cache entry
        let _lock = self.lock_manager.acquire(&key).await?;

        // Check if we have it in cache
        if let Some(entry) = self.index.get(&key).await {
            let cache_path = self.index.get_cache_path(&entry);

            // If extraction was done, return extracted path
            if let Some(extracted_path) = self.index.get_extracted_path(&entry)
                && extracted_path.exists()
            {
                // Re-verify attestation if configured but not yet verified for this entry
                if let Some(attestation_config) = attestation
                    && !entry.attestation_verified
                {
                    let archive_path = self.index.get_cache_path(&entry);
                    self.verify_attestation(&archive_path, url, attestation_config)
                        .await?;
                    self.index.set_attestation_verified(&key).await?;
                }
                self.index.touch(&key).await?;
                tracing::info!(
                    "Found extracted source in cache: {}",
                    extracted_path.display()
                );
                return Ok(extracted_path);
            }

            // Otherwise return the archive file
            if cache_path.exists() {
                // Validate all checksums if provided
                if !checksums.is_empty() {
                    let mismatch = checksums
                        .iter()
                        .find_map(|cs| cs.validate(&cache_path).err());
                    if mismatch.is_some() {
                        tracing::warn!("Checksum validation failed, re-downloading");
                        fs_err::tokio::remove_file(&cache_path).await?;
                    } else {
                        // Re-verify attestation if configured but not yet verified for this entry
                        if let Some(attestation_config) = attestation
                            && !entry.attestation_verified
                        {
                            self.verify_attestation(&cache_path, url, attestation_config)
                                .await?;
                            self.index.set_attestation_verified(&key).await?;
                        }
                        self.index.touch(&key).await?;
                        tracing::info!("Found source in cache: {}", cache_path.display());
                        return Ok(cache_path);
                    }
                } else {
                    // Re-verify attestation if configured but not yet verified for this entry
                    if let Some(attestation_config) = attestation
                        && !entry.attestation_verified
                    {
                        self.verify_attestation(&cache_path, url, attestation_config)
                            .await?;
                        self.index.set_attestation_verified(&key).await?;
                    }
                    self.index.touch(&key).await?;
                    tracing::info!("Found source in cache: {}", cache_path.display());
                    return Ok(cache_path);
                }
            }
        }

        // Download the file
        tracing::info!("Downloading from: {}", url);
        let (cache_path, actual_filename) = self.download_url(url, &key).await?;

        // Validate all checksums
        for cs in checksums {
            if let Err(mismatch) = cs.validate(&cache_path) {
                fs_err::tokio::remove_file(&cache_path).await?;
                return Err(CacheError::ValidationFailed {
                    path: cache_path,
                    expected: mismatch.expected,
                    actual: mismatch.actual,
                    kind: mismatch.kind.to_string(),
                });
            }
        }

        // Perform attestation verification if configured
        if let Some(attestation_config) = attestation {
            self.verify_attestation(&cache_path, url, attestation_config)
                .await?;
        }

        // Extract if needed and no explicit filename was provided
        let final_path = if file_name.is_none() && self.should_extract(&cache_path) {
            let extracted_dir = self.cache_dir.join(format!("{}_extracted", key));
            self.extract_archive(&cache_path, &extracted_dir).await?;
            Some(extracted_dir)
        } else {
            None
        };

        // Use the first checksum for the cache entry metadata
        let primary_checksum = checksums.first();

        // Create cache entry
        let entry = CacheEntry {
            source_type: SourceType::Url,
            url: url.to_string(),
            checksum: primary_checksum.map(|c| c.to_hex()),
            checksum_type: primary_checksum
                .map(|c| match c {
                    Checksum::Sha256(_) => "sha256",
                    Checksum::Md5(_) => "md5",
                })
                .map(String::from),
            actual_filename,
            git_commit: None,
            git_rev: None,
            cache_path: cache_path
                .strip_prefix(&self.cache_dir)
                .unwrap_or(&cache_path)
                .to_path_buf(),
            extracted_path: final_path
                .as_ref()
                .map(|p| p.strip_prefix(&self.cache_dir).unwrap_or(p).to_path_buf()),
            last_accessed: chrono::Utc::now(),
            created: chrono::Utc::now(),
            lock_file: Some(_lock.path().to_path_buf()),
            attestation_verified: attestation.is_some(),
        };

        self.index.insert(key, entry).await?;

        Ok(final_path.unwrap_or(cache_path))
    }

    /// Download a URL to the cache
    async fn download_url(
        &self,
        url: &url::Url,
        key: &str,
    ) -> Result<(PathBuf, Option<String>), CacheError> {
        // Determine filename
        let filename = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .unwrap_or("download");

        let cache_path = self.cache_dir.join(format!("{}_{}", key, filename));

        // Handle file:// URLs
        if url.scheme() == "file" {
            let source_path = url
                .to_file_path()
                .map_err(|_| CacheError::Other("Invalid file URL".to_string()))?;

            if !source_path.exists() {
                return Err(CacheError::FileNotFound(source_path));
            }

            fs_err::tokio::copy(&source_path, &cache_path).await?;
            return Ok((cache_path, Some(filename.to_string())));
        }

        // Download from HTTP/HTTPS - use the appropriate client based on SSL settings
        let response = self.client.for_host(url).get(url.clone()).send().await?;

        if !response.status().is_success() {
            return Err(CacheError::Download(
                response.error_for_status().unwrap_err(),
            ));
        }

        // Get actual filename from Content-Disposition header if present
        let actual_filename = response
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .and_then(extract_filename_from_header);

        // Get content length for progress reporting
        let total_size = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        // Notify progress handler
        if let Some(handler) = &self.progress_handler {
            handler.on_download_start(url.as_str(), total_size);
        }

        // Stream download to file
        let mut file = fs_err::tokio::File::create(&cache_path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0u64;

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;
            file.write_all(&chunk).await?;

            // Update progress
            if let Some(handler) = &self.progress_handler {
                handler.on_download_progress(url.as_str(), downloaded, total_size);
            }
        }

        file.flush().await?;

        // Notify completion
        if let Some(handler) = &self.progress_handler {
            handler.on_download_complete(url.as_str());
        }

        // If Content-Disposition provided a filename that differs from the URL's,
        // rename the cached file so downstream code can detect the archive format
        // from the file extension alone.
        let final_path = if let Some(ref actual) = actual_filename {
            let new_path = self.cache_dir.join(format!("{}_{}", key, actual));
            if new_path != cache_path {
                fs_err::tokio::rename(&cache_path, &new_path).await?;
                new_path
            } else {
                cache_path
            }
        } else {
            cache_path
        };

        Ok((final_path, actual_filename))
    }

    /// Check if a file should be extracted based on its filename extension
    pub(crate) fn should_extract(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        is_archive(name)
    }

    /// Extract an archive to a directory
    async fn extract_archive(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<(), CacheError> {
        // Notify progress handler
        if let Some(handler) = &self.progress_handler {
            handler.on_extraction_start(archive_path);
        }

        // Create a temporary directory for extraction
        let temp_dir = tempfile::tempdir_in(&self.cache_dir)
            .map_err(|e| CacheError::Other(format!("Failed to create temp dir: {}", e)))?;

        let name = archive_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Extract based on file type to temp directory
        if is_tarball(name) {
            extract_tar(archive_path, temp_dir.path())?;
        } else if name.ends_with(".zip") {
            extract_zip(archive_path, temp_dir.path())?;
        } else if name.ends_with(".7z") {
            extract_7z(archive_path, temp_dir.path())?;
        } else {
            return Err(CacheError::ExtractionError(format!(
                "Unsupported archive format: {}",
                name
            )));
        }

        // Strip root directory if needed and move to final location
        strip_and_move_extracted_dir(temp_dir.path(), target_dir).await?;

        // Notify completion
        if let Some(handler) = &self.progress_handler {
            handler.on_extraction_complete(archive_path);
        }

        Ok(())
    }

    /// Verify a downloaded file using attestation (sigstore-based).
    ///
    /// Determines the bundle URL (explicit or auto-derived for PyPI), downloads it,
    /// and verifies the artifact against the specified identity checks.
    ///
    /// For standard sigstore bundles (`.sigstore.json`), parses and verifies directly.
    /// For PyPI provenance responses, extracts attestation bundles and converts them
    /// to sigstore bundles for verification.
    ///
    /// Identity matching uses **prefix** semantics: the expected identity (e.g.
    /// `https://github.com/pallets/flask`) must be a prefix of the actual certificate
    /// identity (e.g. `https://github.com/pallets/flask/.github/workflows/release.yml@refs/tags/3.1.1`).
    async fn verify_attestation(
        &self,
        file_path: &Path,
        source_url: &url::Url,
        attestation_config: &AttestationVerification,
    ) -> Result<(), CacheError> {
        use sigstore_trust_root::TrustedRoot;
        use sigstore_verify::{VerificationPolicy, verify};

        // Determine bundle URL: explicit, or auto-derive from PyPI source URL
        let bundle_url = if let Some(url) = &attestation_config.bundle_url {
            Some(url.clone())
        } else {
            derive_pypi_provenance_url(source_url)
        };

        let bundle_url = bundle_url.ok_or_else(|| {
            CacheError::InvalidAttestationBundle(
                "No bundle_url provided and could not auto-derive one (not a PyPI source)"
                    .to_string(),
            )
        })?;

        tracing::info!("Downloading attestation bundle from {}", bundle_url);
        let response_json = self.download_attestation_bundle(&bundle_url).await?;

        // Load the production Sigstore trusted root (embedded, no network needed)
        let trusted_root = TrustedRoot::production().map_err(|e| {
            CacheError::SigstoreTrustRoot(format!("Failed to load Sigstore trusted root: {}", e))
        })?;

        // Read the artifact for verification
        let artifact_bytes = fs_err::tokio::read(file_path).await?;

        // Parse the response: could be a plain sigstore bundle or a PyPI provenance response
        let parsed = parse_attestation_response(&response_json)?;

        // For each required identity check, find a matching bundle and verify it
        for check in &attestation_config.identity_checks {

            let mut matched = false;
            let mut found_identities: Vec<String> = Vec::new();
            let mut verification_errors: Vec<String> = Vec::new();

            for bundle in &parsed.bundles {
                // Verify with just the issuer in the policy — we do prefix matching on identity ourselves.
                // For PyPI-converted bundles, skip tlog verification since we can't reconstruct
                // the canonicalized rekor body from the PEP 740 format.
                let mut policy = VerificationPolicy::default().require_issuer(check.issuer.clone());
                if parsed.from_pypi {
                    policy = policy.skip_tlog();
                }

                match verify(artifact_bytes.as_slice(), bundle, &policy, &trusted_root) {
                    Ok(result) => {
                        if let Some(ref actual_identity) = result.identity {
                            // Prefix match: expected identity must be a prefix of the actual identity
                            if actual_identity.starts_with(&check.identity) {
                                tracing::info!(
                                    "\u{2714} Attestation verified (identity={})",
                                    actual_identity,
                                );
                                matched = true;
                                break;
                            } else {
                                found_identities.push(actual_identity.clone());
                            }
                        }
                    }
                    Err(e) => {
                        verification_errors.push(e.to_string());
                    }
                }
            }

            if !matched {
                let mut msg = format!(
                    "attestation identity mismatch for publisher '{}'\n  expected identity prefix: {}\n  expected issuer: {}",
                    check
                        .identity
                        .trim_start_matches("https://github.com/")
                        .trim_start_matches("https://gitlab.com/"),
                    check.identity,
                    check.issuer,
                );
                if !found_identities.is_empty() {
                    msg.push_str("\n  found identities in attestation:");
                    for id in &found_identities {
                        msg.push_str(&format!("\n    - {}", id));
                    }
                }
                if !verification_errors.is_empty() {
                    for err in &verification_errors {
                        msg.push_str(&format!("\n  verification error: {}", err));
                    }
                }
                return Err(CacheError::AttestationVerification(msg));
            }
        }

        tracing::info!(
            "\u{2714} All attestation checks passed for {}",
            file_path
                .file_name()
                .map(|f| f.to_string_lossy())
                .unwrap_or_else(|| file_path.to_string_lossy())
        );
        Ok(())
    }

    /// Download an attestation bundle from a URL.
    ///
    /// Returns the raw response body — can be a standard sigstore bundle
    /// or a PyPI PEP 740 provenance response.
    async fn download_attestation_bundle(&self, url: &url::Url) -> Result<String, CacheError> {
        let response = self
            .client
            .for_host(url)
            .get(url.clone())
            .send()
            .await
            .map_err(|e| CacheError::AttestationBundleDownload {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(CacheError::AttestationBundleDownload {
                url: url.to_string(),
                reason: format!("HTTP error: {}", response.status()),
            });
        }

        response
            .text()
            .await
            .map_err(|e| CacheError::AttestationBundleDownload {
                url: url.to_string(),
                reason: format!("Failed to read response body: {}", e),
            })
    }

    /// Clean up stale locks (manual cleanup only, cache entries are kept indefinitely)
    pub async fn cleanup_stale_locks(&self) -> Result<(), CacheError> {
        self.lock_manager.cleanup_stale_locks().await?;
        Ok(())
    }

    /// Get cache statistics
    pub async fn stats(&self) -> Result<CacheStats, CacheError> {
        let entries = self.index.list_entries().await;
        let total_entries = entries.len();
        let mut total_size = 0u64;
        let mut git_entries = 0;
        let mut url_entries = 0;

        for (_, entry) in entries {
            match entry.source_type {
                SourceType::Git => git_entries += 1,
                SourceType::Url => url_entries += 1,
            }

            let path = self.index.get_cache_path(&entry);
            if let Ok(metadata) = fs_err::tokio::metadata(&path).await {
                if metadata.is_file() {
                    total_size += metadata.len();
                } else if metadata.is_dir() {
                    // Calculate directory size
                    total_size += calculate_dir_size(&path).await?;
                }
            }
        }

        Ok(CacheStats {
            total_entries,
            total_size,
            git_entries,
            url_entries,
        })
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_size: u64,
    pub git_entries: usize,
    pub url_entries: usize,
}

// Helper functions

pub(crate) fn extract_filename_from_header(header_value: &str) -> Option<String> {
    for part in header_value.split(';') {
        let part = part.trim();
        if part.starts_with("filename=") {
            let filename = part.strip_prefix("filename=")?;
            let filename = filename.trim_matches('"').trim_matches('\'');
            if !filename.is_empty() {
                // Strip any path components — only keep the base filename
                let filename = Path::new(filename)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(filename);
                return Some(filename.to_string());
            }
        }
    }
    None
}

pub(crate) fn is_archive(name: &str) -> bool {
    is_tarball(name) || name.ends_with(".zip") || name.ends_with(".7z")
}

/// Checks whether file has known tarball extension
pub fn is_tarball(file_name: &str) -> bool {
    [
        // Gzip
        ".tar.gz",
        ".tgz",
        ".taz",
        // Bzip2
        ".tar.bz2",
        ".tbz",
        ".tbz2",
        ".tz2",
        // Xz2
        ".tar.lzma",
        ".tlz",
        ".tar.xz",
        ".txz",
        // Zstd
        ".tar.zst",
        ".tzst",
        // Compress
        ".tar.Z",
        ".taZ",
        // Lzip
        ".tar.lz",
        // Lzop
        ".tar.lzo",
        // PlainTar
        ".tar",
    ]
    .iter()
    .any(|ext| file_name.ends_with(ext))
}

fn extract_tar(archive: &Path, target: &Path) -> Result<(), CacheError> {
    let file = fs_err::File::open(archive)
        .map_err(|e| CacheError::ExtractionError(format!("Failed to open archive: {}", e)))?;

    let name = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let mut archive = tar::Archive::new(GzDecoder::new(file));
        archive
            .unpack(target)
            .map_err(|e| CacheError::ExtractionError(format!("Failed to extract tar.gz: {}", e)))?;
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(file));
        archive.unpack(target).map_err(|e| {
            CacheError::ExtractionError(format!("Failed to extract tar.bz2: {}", e))
        })?;
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        let mut archive = tar::Archive::new(lzma_rust2::XzReader::new(file, true));
        archive
            .unpack(target)
            .map_err(|e| CacheError::ExtractionError(format!("Failed to extract tar.xz: {}", e)))?;
    } else if name.ends_with(".tar.zst") {
        let decoder = zstd::stream::read::Decoder::new(file).map_err(|e| {
            CacheError::ExtractionError(format!("Failed to create zstd decoder: {}", e))
        })?;
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(target).map_err(|e| {
            CacheError::ExtractionError(format!("Failed to extract tar.zst: {}", e))
        })?;
    } else {
        let mut archive = tar::Archive::new(file);
        archive
            .unpack(target)
            .map_err(|e| CacheError::ExtractionError(format!("Failed to extract tar: {}", e)))?;
    }

    Ok(())
}

fn extract_zip(archive: &Path, target: &Path) -> Result<(), CacheError> {
    let file = fs_err::File::open(archive)
        .map_err(|e| CacheError::ExtractionError(format!("Failed to open archive: {}", e)))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| CacheError::ExtractionError(format!("Failed to read zip: {}", e)))?;

    archive
        .extract(target)
        .map_err(|e| CacheError::ExtractionError(format!("Failed to extract zip: {}", e)))?;

    Ok(())
}

fn extract_7z(archive: &Path, target: &Path) -> Result<(), CacheError> {
    sevenz_rust2::decompress_file(archive, target)
        .map_err(|e| CacheError::ExtractionError(format!("Failed to extract 7z: {}", e)))?;

    Ok(())
}

async fn calculate_dir_size(dir: &Path) -> Result<u64, CacheError> {
    let mut total = 0u64;
    let mut entries = fs_err::tokio::read_dir(dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if metadata.is_file() {
            total += metadata.len();
        } else if metadata.is_dir() {
            total += Box::pin(calculate_dir_size(&entry.path())).await?;
        }
    }

    Ok(total)
}

/// Strip root directory if the extracted archive contains only a single top-level directory
async fn strip_and_move_extracted_dir(src: &Path, dest: &Path) -> Result<(), CacheError> {
    use fs_err as fs;

    // Read entries from source directory
    let mut entries = fs::read_dir(src)?;

    // Check if there's only one entry and if it's a directory
    let first_entry = entries.next();
    let second_entry = entries.next();

    let src_dir = match (first_entry, second_entry) {
        (Some(Ok(entry)), None) if entry.file_type()?.is_dir() => {
            // Single directory - we'll extract from inside it
            entry.path()
        }
        _ => {
            // Multiple entries or not a directory - use the source as-is
            src.to_path_buf()
        }
    };

    // Create destination directory
    fs::create_dir_all(dest)?;

    // Move all files from source directory to destination
    for entry in fs::read_dir(&src_dir)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        fs::rename(entry.path(), dest_path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_pypi_provenance_url_pypi_io() {
        let url =
            url::Url::parse("https://pypi.io/packages/source/f/flask/flask-3.1.1.tar.gz").unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask/3.1.1/flask-3.1.1.tar.gz/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_pythonhosted() {
        let url = url::Url::parse(
            "https://files.pythonhosted.org/packages/source/f/flask/flask-3.1.1.tar.gz",
        )
        .unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask/3.1.1/flask-3.1.1.tar.gz/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_normalizes_name() {
        let url =
            url::Url::parse("https://pypi.io/packages/source/F/Flask-CORS/Flask-CORS-4.0.0.tar.gz")
                .unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask-cors/4.0.0/Flask-CORS-4.0.0.tar.gz/provenance"
        );
    }

    #[test]
    fn test_derive_pypi_provenance_url_non_pypi() {
        let url =
            url::Url::parse("https://github.com/pallets/flask/archive/v3.1.1.tar.gz").unwrap();
        assert!(derive_pypi_provenance_url(&url).is_none());
    }

    #[test]
    fn test_derive_pypi_provenance_url_zip() {
        let url =
            url::Url::parse("https://pypi.io/packages/source/f/flask/flask-3.1.1.zip").unwrap();
        let result = derive_pypi_provenance_url(&url).unwrap();
        assert_eq!(
            result.as_str(),
            "https://pypi.org/integrity/flask/3.1.1/flask-3.1.1.zip/provenance"
        );
    }

    #[test]
    fn test_parse_attestation_response_sigstore_bundle() {
        // A sigstore bundle has a "mediaType" field
        let json = r#"{
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {
                "certificate": { "rawBytes": "dGVzdA==" },
                "tlogEntries": [],
                "timestampVerificationData": {}
            },
            "dsseEnvelope": {
                "payload": "dGVzdA==",
                "payloadType": "application/vnd.in-toto+json",
                "signatures": [{ "sig": "dGVzdA==" }]
            }
        }"#;
        let parsed = parse_attestation_response(json).unwrap();
        assert_eq!(parsed.bundles.len(), 1);
        assert!(!parsed.from_pypi);
    }

    #[test]
    fn test_parse_attestation_response_pypi_provenance() {
        // A PyPI provenance response has "attestation_bundles"
        let json = r#"{
            "version": 1,
            "attestation_bundles": [
                {
                    "publisher": { "kind": "GitHub", "repository": "pallets/flask" },
                    "attestations": [
                        {
                            "version": 1,
                            "envelope": {
                                "statement": "dGVzdA==",
                                "signature": "dGVzdA=="
                            },
                            "verification_material": {
                                "certificate": "dGVzdA==",
                                "transparency_entries": []
                            }
                        }
                    ]
                }
            ]
        }"#;
        let parsed = parse_attestation_response(json).unwrap();
        assert_eq!(parsed.bundles.len(), 1);
        assert!(parsed.from_pypi);
    }

    #[test]
    fn test_parse_attestation_response_unrecognized_format() {
        let json = r#"{ "foo": "bar" }"#;
        assert!(parse_attestation_response(json).is_err());
    }
}
