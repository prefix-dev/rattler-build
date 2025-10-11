//! Simple async file locking guard to replace pixi_utils::AsyncPrefixGuard

use file_lock::{FileLock, FileOptions};
use std::path::{Path, PathBuf};
use tokio::fs;

/// A simple async guard that manages file locking for Git operations
pub struct AsyncPrefixGuard {
    lock_path: PathBuf,
}

impl AsyncPrefixGuard {
    /// Create a new guard for the given path
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let lock_path = path.as_ref().to_path_buf();

        // Create parent directories if needed
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        Ok(Self { lock_path })
    }

    /// Get a write guard
    pub async fn write(&self) -> Result<WriteGuard, std::io::Error> {
        WriteGuard::new(&self.lock_path).await
    }
}

/// A write guard that holds a file lock
pub struct WriteGuard {
    _lock: FileLock,
    lock_path: PathBuf,
}

impl WriteGuard {
    async fn new(path: &Path) -> Result<Self, std::io::Error> {
        let lock_path = path.with_extension("lock");

        // Use tokio::task::spawn_blocking for the blocking file lock operation
        let lock = tokio::task::spawn_blocking({
            let lock_path = lock_path.clone();
            move || {
                FileLock::lock(
                    &lock_path,
                    true, // blocking
                    FileOptions::new().write(true).create(true).truncate(true),
                )
            }
        })
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        Ok(Self {
            _lock: lock,
            lock_path,
        })
    }

    /// Begin the operation (no-op for compatibility)
    pub async fn begin(&mut self) -> Result<(), std::io::Error> {
        // Write a marker to indicate the operation has begun
        fs::write(&self.lock_path.with_extension("begin"), "").await?;
        Ok(())
    }

    /// Finish the operation
    pub async fn finish(self) -> Result<(), std::io::Error> {
        // Remove the begin marker
        let begin_path = self.lock_path.with_extension("begin");
        if begin_path.exists() {
            fs::remove_file(&begin_path).await?;
        }
        Ok(())
    }
}

impl Drop for WriteGuard {
    fn drop(&mut self) {
        // Clean up any marker files
        let begin_path = self.lock_path.with_extension("begin");
        // We're disallowig fs::remove_file because we usually prefer fs_err. But here it's fine.
        #[allow(clippy::disallowed_methods)]
        let _ = std::fs::remove_file(&begin_path);
    }
}
