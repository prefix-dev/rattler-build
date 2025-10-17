//! Error types for source cache operations

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to download from URL: {0}")]
    Download(#[from] reqwest::Error),

    #[error("Failed to download from URL: {0}")]
    DownloadMiddleware(#[from] reqwest_middleware::Error),

    #[error("URL does not point to a file: {0}")]
    UrlNotFile(url::Url),

    #[error("Checksum validation failed for {path:?}")]
    ValidationFailed { path: PathBuf },

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Failed to extract archive: {0}")]
    ExtractionError(String),

    #[error("Failed to acquire lock for cache entry: {0}")]
    LockError(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid cache entry")]
    InvalidCacheEntry,

    #[error("No checksum provided for URL: {0}")]
    NoChecksum(String),

    #[error("WalkDir error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("{0}")]
    Other(String),
}
