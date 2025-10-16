//! File locking utilities for concurrent cache access

use crate::error::CacheError;
use rattler_prefix_guard::{lock_exclusive, try_lock_exclusive, unlock};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// A guard that holds a file lock and releases it when dropped
pub struct CacheLockGuard {
    file: Option<File>,
    lock_path: PathBuf,
}

impl CacheLockGuard {
    /// Get the path to the lock file
    pub fn path(&self) -> &Path {
        &self.lock_path
    }
}

impl Drop for CacheLockGuard {
    fn drop(&mut self) {
        if let Some(f) = self.file.take() {
            let _ = unlock(&f);
        }
    }
}

/// Manages file locks for cache entries
pub struct LockManager {
    locks_dir: PathBuf,
}

impl LockManager {
    /// Create a new lock manager
    pub async fn new(cache_dir: &Path) -> Result<Self, CacheError> {
        let locks_dir = cache_dir.join(".locks");

        // Create locks directory if it doesn't exist
        if !locks_dir.exists() {
            tokio::fs::create_dir_all(&locks_dir).await?;
        }

        Ok(Self { locks_dir })
    }

    /// Acquire a lock for a cache entry key
    pub async fn acquire(&self, key: &str) -> Result<CacheLockGuard, CacheError> {
        let lock_path = self.locks_dir.join(format!("{}.lock", key));

        // Open or create the lock file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| {
                CacheError::LockError(format!(
                    "Failed to open lock file {}: {}",
                    lock_path.display(),
                    e
                ))
            })?;

        // Try to acquire the lock with retries
        let max_retries = 100;
        let retry_delay = std::time::Duration::from_millis(100);

        for attempt in 0..max_retries {
            match lock_exclusive(&file) {
                Ok(()) => {
                    return Ok(CacheLockGuard {
                        file: Some(file),
                        lock_path,
                    });
                }
                Err(e) if attempt < max_retries - 1 => {
                    tracing::debug!(
                        "Failed to acquire lock for {} (attempt {}/{}): {}",
                        key,
                        attempt + 1,
                        max_retries,
                        e
                    );
                    tokio::time::sleep(retry_delay).await;
                }
                Err(e) => {
                    return Err(CacheError::LockError(format!(
                        "Failed to acquire lock for {} after {} attempts: {}",
                        key, max_retries, e
                    )));
                }
            }
        }

        Err(CacheError::LockError(format!(
            "Failed to acquire lock for {} after {} attempts",
            key, max_retries
        )))
    }

    /// Try to acquire a lock without blocking
    pub fn try_acquire(&self, key: &str) -> Result<CacheLockGuard, CacheError> {
        let lock_path = self.locks_dir.join(format!("{}.lock", key));

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| {
                CacheError::LockError(format!(
                    "Failed to open lock file {}: {}",
                    lock_path.display(),
                    e
                ))
            })?;

        match try_lock_exclusive(&file) {
            Ok(()) => Ok(CacheLockGuard {
                file: Some(file),
                lock_path,
            }),
            Err(_e) => {
                // For now, just return an error - we could check error_unsupported
                // but for cache locks we want them to fail if they can't be acquired
                Err(CacheError::LockError(format!(
                    "Failed to acquire lock for {}",
                    key
                )))
            }
        }
    }

    /// Clean up stale lock files
    pub async fn cleanup_stale_locks(&self) -> Result<(), CacheError> {
        let mut dir = tokio::fs::read_dir(&self.locks_dir).await?;

        while let Some(entry) = dir.next_entry().await? {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.ends_with(".lock") {
                    // Try to acquire the lock non-blocking
                    // If we can acquire it, the lock was stale
                    if let Ok(guard) = self.try_acquire(filename.trim_end_matches(".lock")) {
                        drop(guard);
                        tracing::debug!("Cleaned up stale lock file: {}", filename);
                    }
                }
            }
        }

        Ok(())
    }
}
