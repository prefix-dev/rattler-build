//! File locking utilities for concurrent cache access

use crate::error::CacheError;
use file_lock::{FileLock, FileOptions};
use std::path::{Path, PathBuf};

/// A guard that holds a file lock and releases it when dropped
pub struct CacheLockGuard {
    _lock: FileLock,
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
        // FileLock automatically releases on drop
        // Clean up the lock file
        let _ = std::fs::remove_file(&self.lock_path);
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

        // Try to acquire the lock with retries
        let max_retries = 100;
        let retry_delay = std::time::Duration::from_millis(100);

        for attempt in 0..max_retries {
            match FileLock::lock(
                &lock_path,
                true, // block until lock is acquired
                FileOptions::new().write(true).create(true).truncate(true),
            ) {
                Ok(lock) => {
                    return Ok(CacheLockGuard {
                        _lock: lock,
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

        match FileLock::lock(
            &lock_path,
            false, // non-blocking
            FileOptions::new().write(true).create(true).truncate(true),
        ) {
            Ok(lock) => Ok(CacheLockGuard {
                _lock: lock,
                lock_path,
            }),
            Err(e) => Err(CacheError::LockError(format!(
                "Failed to acquire lock for {}: {}",
                key, e
            ))),
        }
    }

    /// Clean up stale lock files
    pub async fn cleanup_stale_locks(&self) -> Result<(), CacheError> {
        let mut dir = tokio::fs::read_dir(&self.locks_dir).await?;

        while let Some(entry) = dir.next_entry().await? {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.ends_with(".lock") {
                    let lock_path = self.locks_dir.join(filename);

                    // Try to acquire the lock non-blocking
                    // If we can acquire it, the lock was stale
                    if let Ok(guard) = self.try_acquire(filename.trim_end_matches(".lock")) {
                        drop(guard); // This will clean up the lock file
                        tracing::debug!("Cleaned up stale lock file: {}", filename);
                    }
                }
            }
        }

        Ok(())
    }
}
