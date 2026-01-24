//! Main source cache implementation

use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::{
    builder::ProgressHandler,
    error::CacheError,
    index::{CacheEntry, CacheIndex, SourceType},
    lock::LockManager,
    source::{Checksum, GitSource, SigstoreVerification, Source, UrlSource},
};
use rattler_build_networking::BaseClient;
use rattler_git::resolver::GitResolver;
use sigstore_trust_root::TrustedRoot;
use sigstore_types::Bundle;
use sigstore_verify::{VerificationPolicy, Verifier};

/// PyPI PEP 740 attestation format - used at /integrity/.../.../provenance endpoints
mod pypi_attestation {
    use serde::Deserialize;

    /// Root structure of PyPI provenance response
    #[derive(Debug, Deserialize)]
    pub struct ProvenanceResponse {
        pub attestation_bundles: Vec<AttestationBundle>,
    }

    /// A bundle containing publisher info and attestations
    #[derive(Debug, Deserialize)]
    pub struct AttestationBundle {
        #[allow(dead_code)]
        pub publisher: Publisher,
        pub attestations: Vec<Attestation>,
    }

    /// Publisher information (GitHub, GitLab, etc.)
    #[derive(Debug, Deserialize)]
    pub struct Publisher {
        #[allow(dead_code)]
        pub kind: String,
        #[allow(dead_code)]
        #[serde(default)]
        pub repository: Option<String>,
    }

    /// Individual attestation - this is the sigstore bundle format
    #[derive(Debug, Deserialize)]
    pub struct Attestation {
        /// The DSSE envelope containing the signed statement (PyPI format)
        pub envelope: PyPiEnvelope,
        /// Verification material (certificate, transparency log entry, etc.)
        /// Note: PyPI uses snake_case (verification_material), sigstore uses camelCase
        pub verification_material: serde_json::Value,
    }

    /// PyPI-style DSSE envelope (different from standard sigstore format)
    #[derive(Debug, Deserialize)]
    pub struct PyPiEnvelope {
        /// Base64-encoded signature
        pub signature: String,
        /// Base64-encoded statement (in-toto statement)
        pub statement: String,
    }

    impl Attestation {
        /// Convert PyPI attestation to standard sigstore bundle JSON
        ///
        /// PyPI uses a simplified envelope format:
        /// ```json
        /// { "signature": "...", "statement": "..." }
        /// ```
        ///
        /// Sigstore expects the standard DSSE envelope format:
        /// ```json
        /// {
        ///   "payloadType": "application/vnd.in-toto+json",
        ///   "payload": "...",
        ///   "signatures": [{"sig": "..."}]
        /// }
        /// ```
        pub fn to_sigstore_bundle(&self) -> serde_json::Value {
            // Convert PyPI envelope to standard DSSE envelope format
            let dsse_envelope = serde_json::json!({
                "payloadType": "application/vnd.in-toto+json",
                "payload": self.envelope.statement,
                "signatures": [{
                    "sig": self.envelope.signature
                }]
            });

            // Convert verification_material from snake_case to camelCase
            let verification_material = convert_to_camel_case(&self.verification_material);

            serde_json::json!({
                "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
                "verificationMaterial": verification_material,
                "dsseEnvelope": dsse_envelope
            })
        }
    }

    /// Recursively convert JSON object keys from snake_case to camelCase
    /// Also handles special field name mappings and value transformations for PyPI -> sigstore conversion
    fn convert_to_camel_case(value: &serde_json::Value) -> serde_json::Value {
        convert_value_with_key(value, None)
    }

    /// Convert a value, with optional key context for special transformations
    fn convert_value_with_key(value: &serde_json::Value, key: Option<&str>) -> serde_json::Value {
        // Handle special value transformations based on key
        if let Some(k) = key {
            // Certificate needs to be wrapped in { "rawBytes": "..." }
            if k == "certificate" {
                if let serde_json::Value::String(cert_str) = value {
                    return serde_json::json!({
                        "rawBytes": cert_str
                    });
                }
            }
        }

        match value {
            serde_json::Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, val) in map {
                    let camel_key = convert_key_name(k);
                    new_map.insert(camel_key, convert_value_with_key(val, Some(k)));
                }
                serde_json::Value::Object(new_map)
            }
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter().map(|v| convert_value_with_key(v, key)).collect(),
            ),
            other => other.clone(),
        }
    }

    /// Convert a key name from PyPI format to sigstore format
    /// Handles special cases and general snake_case to camelCase conversion
    fn convert_key_name(key: &str) -> String {
        // Special mappings for PyPI -> sigstore field names
        match key {
            "transparency_entries" => "tlogEntries".to_string(),
            "canonicalized_body" => "canonicalizedBody".to_string(),
            "inclusion_promise" => "inclusionPromise".to_string(),
            "inclusion_proof" => "inclusionProof".to_string(),
            "signed_entry_timestamp" => "signedEntryTimestamp".to_string(),
            "integrated_time" => "integratedTime".to_string(),
            "kind_version" => "kindVersion".to_string(),
            "log_id" => "logId".to_string(),
            "log_index" => "logIndex".to_string(),
            "key_id" => "keyId".to_string(),
            "root_hash" => "rootHash".to_string(),
            "tree_size" => "treeSize".to_string(),
            // General conversion for other fields
            _ => snake_to_camel(key),
        }
    }

    /// Convert snake_case to camelCase
    fn snake_to_camel(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = false;

        for c in s.chars() {
            if c == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }

        result
    }
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
        tokio::fs::create_dir_all(&git_cache).await?;

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

        // Handle LFS if needed
        if source.lfs {
            self.git_lfs_pull(&repo_path).await?;
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
        };

        self.index.insert(key, entry).await?;

        Ok(SourceResult {
            path: repo_path,
            git_commit: Some(commit_hash),
        })
    }

    /// Pull LFS files for a git repository
    async fn git_lfs_pull(&self, repo_path: &Path) -> Result<(), CacheError> {
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

        // Fetch LFS files
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
                    source.checksum.as_ref(),
                    source.file_name.as_deref(),
                    source.sigstore.as_ref(),
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
        checksum: Option<&Checksum>,
        file_name: Option<&str>,
        sigstore: Option<&SigstoreVerification>,
    ) -> Result<PathBuf, CacheError> {
        let key = CacheIndex::generate_cache_key(url, checksum);

        // Acquire lock for this cache entry
        let _lock = self.lock_manager.acquire(&key).await?;

        // Check if we have it in cache
        if let Some(entry) = self.index.get(&key).await {
            let cache_path = self.index.get_cache_path(&entry);

            // If extraction was done, return extracted path
            if let Some(extracted_path) = self.index.get_extracted_path(&entry)
                && extracted_path.exists()
            {
                self.index.touch(&key).await?;
                tracing::info!(
                    "Found extracted source in cache: {}",
                    extracted_path.display()
                );
                return Ok(extracted_path);
            }

            // Otherwise return the archive file
            if cache_path.exists() {
                // Validate checksum if provided
                if let Some(cs) = checksum {
                    if !cs.validate(&cache_path) {
                        tracing::warn!("Checksum validation failed, re-downloading");
                        tokio::fs::remove_file(&cache_path).await?;
                    } else {
                        self.index.touch(&key).await?;
                        tracing::info!("Found source in cache: {}", cache_path.display());
                        return Ok(cache_path);
                    }
                } else {
                    self.index.touch(&key).await?;
                    tracing::info!("Found source in cache: {}", cache_path.display());
                    return Ok(cache_path);
                }
            }
        }

        // Download the file
        tracing::info!("Downloading from: {}", url);
        let (cache_path, actual_filename) = self.download_url(url, &key).await?;

        // Validate checksum
        if let Some(cs) = checksum
            && !cs.validate(&cache_path)
        {
            tokio::fs::remove_file(&cache_path).await?;
            return Err(CacheError::ValidationFailed { path: cache_path });
        }

        // Perform sigstore verification if configured
        if let Some(sigstore_config) = sigstore {
            self.verify_sigstore(&cache_path, sigstore_config).await?;
        }

        // Extract if needed and no explicit filename was provided
        let final_path = if file_name.is_none() && self.should_extract(&cache_path) {
            let extracted_dir = self.cache_dir.join(format!("{}_extracted", key));
            self.extract_archive(&cache_path, &extracted_dir).await?;
            Some(extracted_dir)
        } else {
            None
        };

        // Create cache entry
        let entry = CacheEntry {
            source_type: SourceType::Url,
            url: url.to_string(),
            checksum: checksum.map(|c| c.to_hex()),
            checksum_type: checksum
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
        };

        self.index.insert(key, entry).await?;

        Ok(final_path.unwrap_or(cache_path))
    }

    /// Verify a downloaded file using sigstore
    async fn verify_sigstore(
        &self,
        file_path: &Path,
        sigstore_config: &SigstoreVerification,
    ) -> Result<(), CacheError> {
        tracing::info!("Verifying sigstore signature for: {}", file_path.display());

        // Get the sigstore bundle - prefer bundle_url over inline bundle
        let bundle_json = if let Some(url) = &sigstore_config.bundle_url {
            tracing::info!("Downloading sigstore bundle from: {}", url);
            self.download_sigstore_bundle(url).await?
        } else if let Some(json) = &sigstore_config.bundle {
            json.clone()
        } else {
            return Err(CacheError::InvalidSigstoreBundle(
                "No bundle_url or bundle provided in sigstore config".to_string(),
            ));
        };

        // Parse the bundle
        let bundle: Bundle = serde_json::from_str(&bundle_json).map_err(|e| {
            CacheError::InvalidSigstoreBundle(format!("Failed to parse sigstore bundle: {}", e))
        })?;

        // Initialize the trust root (production sigstore infrastructure)
        let trusted_root = TrustedRoot::production().map_err(|e| {
            CacheError::SigstoreTrustRoot(format!("Failed to get sigstore trust root: {}", e))
        })?;

        // Create verifier with the trusted root
        let verifier = Verifier::new_with_trusted_root(&trusted_root);

        // Create verification policy with identity and issuer if provided
        let mut policy = VerificationPolicy::default();
        if let Some(identity) = &sigstore_config.identity {
            tracing::info!("Requiring identity: {}", identity);
            policy = policy.require_identity(identity);
        }
        if let Some(issuer) = &sigstore_config.issuer {
            tracing::info!("Requiring issuer: {}", issuer);
            policy = policy.require_issuer(issuer);
        }

        // Read file and verify with raw bytes
        let artifact_bytes = std::fs::read(file_path).map_err(|e| {
            CacheError::SigstoreVerification(format!("Failed to read file for verification: {}", e))
        })?;

        verifier
            .verify(artifact_bytes.as_slice(), &bundle, &policy)
            .map_err(|e| {
                CacheError::SigstoreVerification(format!("Sigstore verification failed: {}", e))
            })?;

        tracing::info!(
            "Sigstore signature verified successfully for: {}",
            file_path.display()
        );
        Ok(())
    }

    /// Download a sigstore bundle from a URL
    ///
    /// Supports both standard sigstore bundle format and PyPI PEP 740 provenance format.
    /// PyPI provenance URLs typically end in `/provenance` and return attestation bundles
    /// that need to be converted to standard sigstore bundle format.
    async fn download_sigstore_bundle(&self, url: &url::Url) -> Result<String, CacheError> {
        let response = self
            .client
            .for_host(url)
            .get(url.clone())
            .send()
            .await
            .map_err(|e| CacheError::SigstoreBundleDownload {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(CacheError::SigstoreBundleDownload {
                url: url.to_string(),
                reason: format!("HTTP error: {}", response.status()),
            });
        }

        let response_text =
            response
                .text()
                .await
                .map_err(|e| CacheError::SigstoreBundleDownload {
                    url: url.to_string(),
                    reason: format!("Failed to read response body: {}", e),
                })?;

        // Check if this is a PyPI provenance response (PEP 740 format)
        // PyPI provenance URLs end in /provenance and return attestation_bundles
        let is_pypi_provenance =
            url.path().ends_with("/provenance") || response_text.contains("attestation_bundles");

        if is_pypi_provenance {
            tracing::info!(
                "Detected PyPI PEP 740 provenance format, converting to sigstore bundle"
            );
            self.convert_pypi_provenance_to_bundle(&response_text, url)
        } else {
            // Standard sigstore bundle format - return as-is
            Ok(response_text)
        }
    }

    /// Convert PyPI PEP 740 provenance response to standard sigstore bundle JSON
    fn convert_pypi_provenance_to_bundle(
        &self,
        provenance_json: &str,
        url: &url::Url,
    ) -> Result<String, CacheError> {
        let provenance: pypi_attestation::ProvenanceResponse =
            serde_json::from_str(provenance_json).map_err(|e| {
                CacheError::SigstoreBundleDownload {
                    url: url.to_string(),
                    reason: format!("Failed to parse PyPI provenance response: {}", e),
                }
            })?;

        // Get the first attestation from the first bundle
        let attestation = provenance
            .attestation_bundles
            .first()
            .and_then(|bundle| bundle.attestations.first())
            .ok_or_else(|| CacheError::SigstoreBundleDownload {
                url: url.to_string(),
                reason: "No attestations found in PyPI provenance response".to_string(),
            })?;

        // Convert to standard sigstore bundle format
        let bundle = attestation.to_sigstore_bundle();
        serde_json::to_string(&bundle).map_err(|e| CacheError::SigstoreBundleDownload {
            url: url.to_string(),
            reason: format!("Failed to serialize sigstore bundle: {}", e),
        })
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

            tokio::fs::copy(&source_path, &cache_path).await?;
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
        let mut file = tokio::fs::File::create(&cache_path).await?;
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

        Ok((
            cache_path,
            actual_filename.or_else(|| Some(filename.to_string())),
        ))
    }

    /// Check if a file should be extracted
    fn should_extract(&self, path: &Path) -> bool {
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
            if let Ok(metadata) = tokio::fs::metadata(&path).await {
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

fn extract_filename_from_header(header_value: &str) -> Option<String> {
    for part in header_value.split(';') {
        let part = part.trim();
        if part.starts_with("filename=") {
            let filename = part.strip_prefix("filename=")?;
            let filename = filename.trim_matches('"').trim_matches('\'');
            if !filename.is_empty() {
                return Some(filename.to_string());
            }
        }
    }
    None
}

fn is_archive(name: &str) -> bool {
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
    let mut entries = tokio::fs::read_dir(dir).await?;

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
