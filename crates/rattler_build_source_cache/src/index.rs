//! Cache index management for content-addressable storage

use crate::error::CacheError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Type of source stored in the cache
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    /// URL source (downloaded file)
    Url,
    /// Git repository
    Git,
}

/// Information about a cached source entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Type of source
    pub source_type: SourceType,

    /// The original URL/URI this was downloaded/cloned from
    pub url: String,

    /// The checksum of the downloaded file (if available, only for URL sources)
    pub checksum: Option<String>,

    /// The checksum type (sha256, md5, etc.)
    pub checksum_type: Option<String>,

    /// The actual filename (from Content-Disposition or URL, only for URL sources)
    pub actual_filename: Option<String>,

    /// For git sources, the resolved commit hash
    pub git_commit: Option<String>,

    /// For git sources, the original revision (branch/tag/commit)
    pub git_rev: Option<String>,

    /// The path to the downloaded archive file or git repository (relative to cache dir)
    pub cache_path: PathBuf,

    /// The path to the extracted directory (relative to cache dir), if applicable (URL sources only)
    pub extracted_path: Option<PathBuf>,

    /// When this entry was last accessed
    pub last_accessed: chrono::DateTime<chrono::Utc>,

    /// When this entry was created
    pub created: chrono::DateTime<chrono::Utc>,

    /// Lock file path for this entry
    pub lock_file: Option<PathBuf>,
}

/// The cache index that manages content-addressable cache metadata
pub struct CacheIndex {
    /// The path to the cache directory
    cache_dir: PathBuf,

    /// The path to the metadata directory within cache
    metadata_dir: PathBuf,

    /// In-memory cache of entries
    entries: tokio::sync::RwLock<HashMap<String, CacheEntry>>,
}

impl CacheIndex {
    /// Create a new cache index
    pub async fn new(cache_dir: PathBuf) -> Result<Self, CacheError> {
        let metadata_dir = cache_dir.join(".metadata");

        // Create metadata directory if it doesn't exist
        if !metadata_dir.exists() {
            tokio::fs::create_dir_all(&metadata_dir).await?;
        }

        let mut index = Self {
            cache_dir,
            metadata_dir,
            entries: tokio::sync::RwLock::new(HashMap::new()),
        };

        // Load existing entries
        index.load_all().await?;

        Ok(index)
    }

    /// Load all entries from disk
    async fn load_all(&mut self) -> Result<(), CacheError> {
        let mut entries = HashMap::new();

        let mut dir = tokio::fs::read_dir(&self.metadata_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.ends_with(".json") {
                    let key = filename.trim_end_matches(".json");
                    let metadata_path = self.metadata_dir.join(filename);

                    match tokio::fs::read_to_string(&metadata_path).await {
                        Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
                            Ok(cache_entry) => {
                                entries.insert(key.to_string(), cache_entry);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse cache metadata {}: {}", key, e);
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Failed to read cache metadata {}: {}", key, e);
                        }
                    }
                }
            }
        }

        *self.entries.write().await = entries;
        Ok(())
    }

    /// Generate a cache key from URL and optional checksum
    pub fn generate_cache_key(
        url: &url::Url,
        checksum: Option<&crate::source::Checksum>,
    ) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        url.as_str().hash(&mut hasher);

        if let Some(cs) = checksum {
            cs.to_hex().hash(&mut hasher);
        }

        format!("{:x}", hasher.finish())
    }

    /// Generate a cache key for a git source
    pub fn generate_git_cache_key(url: &str, rev: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        rev.hash(&mut hasher);

        format!("git_{:x}", hasher.finish())
    }

    /// Get a cache entry by key
    pub async fn get(&self, key: &str) -> Option<CacheEntry> {
        self.entries.read().await.get(key).cloned()
    }

    /// Add or update a cache entry
    pub async fn insert(&self, key: String, entry: CacheEntry) -> Result<(), CacheError> {
        // Update in-memory cache
        self.entries
            .write()
            .await
            .insert(key.clone(), entry.clone());

        // Persist to disk
        let metadata_path = self.metadata_dir.join(format!("{}.json", key));
        let content = serde_json::to_string_pretty(&entry)?;
        tokio::fs::write(&metadata_path, content).await?;

        Ok(())
    }

    /// Update the last accessed time for an entry
    pub async fn touch(&self, key: &str) -> Result<(), CacheError> {
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(key) {
            entry.last_accessed = chrono::Utc::now();
            let updated_entry = entry.clone();
            drop(entries); // Release lock before writing to disk

            // Persist the update
            let metadata_path = self.metadata_dir.join(format!("{}.json", key));
            let content = serde_json::to_string_pretty(&updated_entry)?;
            tokio::fs::write(&metadata_path, content).await?;
        }
        Ok(())
    }

    /// List all cache entries
    pub async fn list_entries(&self) -> Vec<(String, CacheEntry)> {
        self.entries
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Get the full path for a cache entry (archive file or git repo)
    pub fn get_cache_path(&self, entry: &CacheEntry) -> PathBuf {
        self.cache_dir.join(&entry.cache_path)
    }

    /// Get the full path for an extracted directory (URL sources only)
    pub fn get_extracted_path(&self, entry: &CacheEntry) -> Option<PathBuf> {
        entry
            .extracted_path
            .as_ref()
            .map(|p| self.cache_dir.join(p))
    }

    /// Clean up entries that haven't been accessed in the specified duration
    pub async fn cleanup_old_entries(&self, max_age: chrono::Duration) -> Result<(), CacheError> {
        let cutoff = chrono::Utc::now() - max_age;
        let entries = self.list_entries().await;

        for (key, entry) in entries {
            if entry.last_accessed < cutoff {
                // Remove from in-memory cache
                self.entries.write().await.remove(&key);

                // Remove the cache files
                let cache_path = self.get_cache_path(&entry);

                match entry.source_type {
                    SourceType::Url => {
                        // Remove archive file
                        if cache_path.exists() && cache_path.is_file() {
                            let _ = tokio::fs::remove_file(&cache_path).await;
                        }

                        // Remove extracted directory
                        if let Some(extracted_path) = self.get_extracted_path(&entry) {
                            if extracted_path.exists() {
                                let _ = tokio::fs::remove_dir_all(&extracted_path).await;
                            }
                        }
                    }
                    SourceType::Git => {
                        // Remove git repository directory
                        if cache_path.exists() && cache_path.is_dir() {
                            let _ = tokio::fs::remove_dir_all(&cache_path).await;
                        }
                    }
                }

                // Remove the metadata file
                let metadata_path = self.metadata_dir.join(format!("{}.json", key));
                let _ = tokio::fs::remove_file(&metadata_path).await;
            }
        }

        Ok(())
    }

    /// Get cache directory path
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }
}
