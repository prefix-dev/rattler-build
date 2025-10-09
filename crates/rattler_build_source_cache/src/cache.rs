//! Main source cache implementation

use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::{
    builder::ProgressHandler,
    error::CacheError,
    index::{CacheEntry, CacheIndex, SourceType},
    lock::LockManager,
    source::{Checksum, GitSource, Source, UrlSource},
};
use rattler_git::resolver::GitResolver;

/// The main source cache that handles Git, URL, and Path sources
pub struct SourceCache {
    cache_dir: PathBuf,
    index: CacheIndex,
    lock_manager: LockManager,
    client: reqwest_middleware::ClientWithMiddleware,
    git_resolver: GitResolver,
    max_age: Option<chrono::Duration>,
    progress_handler: Option<Box<dyn ProgressHandler>>,
}

impl SourceCache {
    /// Create a new source cache
    pub async fn new(
        cache_dir: PathBuf,
        client: reqwest_middleware::ClientWithMiddleware,
        max_age: Option<chrono::Duration>,
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
            max_age,
            progress_handler,
        };

        Ok(cache)
    }

    /// Get a source from the cache or fetch it if not present
    pub async fn get_source(&self, source: &Source) -> Result<PathBuf, CacheError> {
        match source {
            Source::Git(git_source) => self.get_git_source(git_source).await,
            Source::Url(url_source) => self.get_url_source(url_source).await,
            Source::Path(path) => {
                // Path sources are not cached, just return the path
                Ok(path.clone())
            }
        }
    }

    /// Get a Git source from the cache or clone it if not present
    async fn get_git_source(&self, source: &GitSource) -> Result<PathBuf, CacheError> {
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
                return Ok(cache_path);
            }
        }

        // Use rattler_git to fetch the repository
        tracing::info!("Fetching git repository: {}", git_url);
        let git_cache = self.cache_dir.join("git");
        tokio::fs::create_dir_all(&git_cache).await?;

        let fetch_result = self
            .git_resolver
            .fetch(git_url.clone(), self.client.clone(), git_cache, None)
            .await
            .map_err(|e| CacheError::Git(format!("Git fetch failed: {}", e)))?;

        let repo_path = fetch_result.path().to_path_buf();
        let commit_hash = fetch_result.commit().to_string();

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
            git_commit: Some(commit_hash),
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

        Ok(repo_path)
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
    async fn get_url_source(&self, source: &UrlSource) -> Result<PathBuf, CacheError> {
        // Try each URL until one succeeds
        let mut last_error = None;

        for url in &source.urls {
            match self
                .try_url(url, source.checksum.as_ref(), source.file_name.as_deref())
                .await
            {
                Ok(path) => return Ok(path),
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
    ) -> Result<PathBuf, CacheError> {
        let key = CacheIndex::generate_cache_key(url, checksum);

        // Acquire lock for this cache entry
        let _lock = self.lock_manager.acquire(&key).await?;

        // Check if we have it in cache
        if let Some(entry) = self.index.get(&key).await {
            let cache_path = self.index.get_cache_path(&entry);

            // If extraction was done, return extracted path
            if let Some(extracted_path) = self.index.get_extracted_path(&entry) {
                if extracted_path.exists() {
                    self.index.touch(&key).await?;
                    tracing::info!(
                        "Found extracted source in cache: {}",
                        extracted_path.display()
                    );
                    return Ok(extracted_path);
                }
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
        if let Some(cs) = checksum {
            if !cs.validate(&cache_path) {
                tokio::fs::remove_file(&cache_path).await?;
                return Err(CacheError::ValidationFailed { path: cache_path });
            }
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

        // Download from HTTP/HTTPS
        let response = self.client.get(url.clone()).send().await?;

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

        // Create target directory
        if !target_dir.exists() {
            tokio::fs::create_dir_all(target_dir).await?;
        }

        let name = archive_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Extract based on file type
        if is_tarball(name) {
            extract_tar(archive_path, target_dir)?;
        } else if name.ends_with(".zip") {
            extract_zip(archive_path, target_dir)?;
        } else if name.ends_with(".7z") {
            extract_7z(archive_path, target_dir)?;
        } else {
            return Err(CacheError::ExtractionError(format!(
                "Unsupported archive format: {}",
                name
            )));
        }

        // Notify completion
        if let Some(handler) = &self.progress_handler {
            handler.on_extraction_complete(archive_path);
        }

        Ok(())
    }

    /// Clean up old cache entries
    pub async fn cleanup(&self) -> Result<(), CacheError> {
        if let Some(max_age) = self.max_age {
            self.index.cleanup_old_entries(max_age).await?;
            self.lock_manager.cleanup_stale_locks().await?;
        }
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

fn is_tarball(name: &str) -> bool {
    name.ends_with(".tar")
        || name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar.bz2")
        || name.ends_with(".tbz2")
        || name.ends_with(".tar.xz")
        || name.ends_with(".txz")
        || name.ends_with(".tar.zst")
}

fn extract_tar(archive: &Path, target: &Path) -> Result<(), CacheError> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let file = std::fs::File::open(archive)
        .map_err(|e| CacheError::ExtractionError(format!("Failed to open archive: {}", e)))?;

    let name = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let mut archive = Archive::new(GzDecoder::new(file));
        archive
            .unpack(target)
            .map_err(|e| CacheError::ExtractionError(format!("Failed to extract tar.gz: {}", e)))?;
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        let mut archive = Archive::new(bzip2::read::BzDecoder::new(file));
        archive.unpack(target).map_err(|e| {
            CacheError::ExtractionError(format!("Failed to extract tar.bz2: {}", e))
        })?;
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        let mut archive = Archive::new(xz2::read::XzDecoder::new(file));
        archive
            .unpack(target)
            .map_err(|e| CacheError::ExtractionError(format!("Failed to extract tar.xz: {}", e)))?;
    } else if name.ends_with(".tar.zst") {
        let decoder = zstd::stream::read::Decoder::new(file).map_err(|e| {
            CacheError::ExtractionError(format!("Failed to create zstd decoder: {}", e))
        })?;
        let mut archive = Archive::new(decoder);
        archive.unpack(target).map_err(|e| {
            CacheError::ExtractionError(format!("Failed to extract tar.zst: {}", e))
        })?;
    } else {
        let mut archive = Archive::new(file);
        archive
            .unpack(target)
            .map_err(|e| CacheError::ExtractionError(format!("Failed to extract tar: {}", e)))?;
    }

    Ok(())
}

fn extract_zip(archive: &Path, target: &Path) -> Result<(), CacheError> {
    let file = std::fs::File::open(archive)
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
