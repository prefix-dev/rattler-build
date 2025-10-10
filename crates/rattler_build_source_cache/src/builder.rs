//! Builder for configuring and creating a SourceCache instance

use crate::{cache::SourceCache, error::CacheError};
use rattler_build_networking::BaseClient;
use std::path::{Path, PathBuf};

/// Builder for creating a configured SourceCache
pub struct SourceCacheBuilder {
    cache_dir: Option<PathBuf>,
    client: Option<BaseClient>,
    enable_compression: bool,
    max_concurrent_downloads: usize,
    progress_handler: Option<Box<dyn ProgressHandler>>,
}

/// Trait for handling progress updates
pub trait ProgressHandler: Send + Sync {
    /// Called when a download starts
    fn on_download_start(&self, url: &str, total_size: Option<u64>);

    /// Called periodically during download
    fn on_download_progress(&self, url: &str, downloaded: u64, total: Option<u64>);

    /// Called when a download completes
    fn on_download_complete(&self, url: &str);

    /// Called when extraction starts
    fn on_extraction_start(&self, path: &Path);

    /// Called when extraction completes
    fn on_extraction_complete(&self, path: &Path);
}

impl Default for SourceCacheBuilder {
    fn default() -> Self {
        Self {
            cache_dir: None,
            client: None,
            enable_compression: true,
            max_concurrent_downloads: 4,
            progress_handler: None,
        }
    }
}

impl SourceCacheBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the cache directory
    pub fn cache_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    /// Set the HTTP client to use for downloads
    pub fn client(mut self, client: BaseClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Enable or disable compression for cached files
    pub fn enable_compression(mut self, enable: bool) -> Self {
        self.enable_compression = enable;
        self
    }

    /// Set the maximum number of concurrent downloads
    pub fn max_concurrent_downloads(mut self, max: usize) -> Self {
        self.max_concurrent_downloads = max.max(1);
        self
    }

    /// Set a progress handler for download and extraction operations
    pub fn progress_handler<H: ProgressHandler + 'static>(mut self, handler: H) -> Self {
        self.progress_handler = Some(Box::new(handler));
        self
    }

    /// Build the SourceCache instance
    pub async fn build(self) -> Result<SourceCache, CacheError> {
        // Use default cache directory if not specified
        let cache_dir = self.cache_dir.unwrap_or_else(|| {
            dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from(".cache"))
                .join("rattler-build")
                .join("source-cache")
        });

        // Create cache directory if it doesn't exist
        if !cache_dir.exists() {
            tokio::fs::create_dir_all(&cache_dir).await?;
        }

        // Use default client if not specified
        let client = self
            .client
            .unwrap_or_else(|| BaseClient::builder().timeout(300).build());

        SourceCache::new(cache_dir, client, self.progress_handler).await
    }
}

// Add required dependencies to Cargo.toml
pub(crate) mod dirs {
    use std::path::PathBuf;

    pub fn cache_dir() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join("Library").join("Caches"))
        }

        #[cfg(target_os = "linux")]
        {
            std::env::var("XDG_CACHE_HOME")
                .ok()
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|home| PathBuf::from(home).join(".cache"))
                })
        }

        #[cfg(target_os = "windows")]
        {
            std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }
}
