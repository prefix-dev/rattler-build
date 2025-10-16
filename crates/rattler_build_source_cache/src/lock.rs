//! File locking utilities for concurrent cache access
//!
//! Implementation of file locks adapted from cargo:
//! <https://github.com/rust-lang/cargo/blob/39c13e67a5962466cc7253d41bc1099bbcb224c3/src/cargo/util/flock.rs>
//!
//! Under MIT license:
//!
//! Permission is hereby granted, free of charge, to any
//! person obtaining a copy of this software and associated
//! documentation files (the "Software"), to deal in the
//! Software without restriction, including without
//! limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software
//! is furnished to do so, subject to the following
//! conditions:
//!
//! The above copyright notice and this permission notice
//! shall be included in all copies or substantial portions
//! of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
//! ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
//! TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
//! PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
//! SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//! CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
//! OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
//! IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.

use crate::error::CacheError;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use sys::{error_contended, error_unsupported, lock_exclusive, try_lock_exclusive, unlock};

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
            match acquire_lock(&lock_path, &file, key) {
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
            Err(e) if error_unsupported(&e) => {
                // Filesystem doesn't support locking, treat as success
                Ok(CacheLockGuard {
                    file: Some(file),
                    lock_path,
                })
            }
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

/// Acquires a lock on a file, handling NFS and unsupported filesystems gracefully.
fn acquire_lock(path: &Path, file: &File, key: &str) -> io::Result<()> {
    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    fn is_on_nfs_mount(path: &Path) -> bool {
        use std::ffi::CString;
        use std::mem;
        use std::os::unix::prelude::*;

        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => return false,
        };

        unsafe {
            let mut buf: libc::statfs = mem::zeroed();
            let r = libc::statfs(path.as_ptr(), &mut buf);

            r == 0 && buf.f_type as u32 == libc::NFS_SUPER_MAGIC as u32
        }
    }

    #[cfg(any(not(target_os = "linux"), target_env = "musl"))]
    fn is_on_nfs_mount(_path: &Path) -> bool {
        false
    }

    // File locking on Unix is currently implemented via `flock`, which is known
    // to be broken on NFS. Skip all file locks entirely on NFS mounts.
    if is_on_nfs_mount(path) {
        return Ok(());
    }

    match try_lock_exclusive(file) {
        Ok(()) => return Ok(()),

        // Ignore locking on filesystems that don't implement file locking
        Err(e) if error_unsupported(&e) => return Ok(()),

        Err(e) => {
            if !error_contended(&e) {
                return Err(e);
            }
        }
    }

    tracing::info!("waiting for file lock on {}", key);

    lock_exclusive(file)
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io::{Error, Result};
    use std::os::unix::io::AsRawFd;

    pub(super) fn lock_exclusive(file: &File) -> Result<()> {
        flock(file, libc::LOCK_EX)
    }

    pub(super) fn try_lock_exclusive(file: &File) -> Result<()> {
        flock(file, libc::LOCK_EX | libc::LOCK_NB)
    }

    pub(super) fn unlock(file: &File) -> Result<()> {
        flock(file, libc::LOCK_UN)
    }

    pub(super) fn error_contended(err: &Error) -> bool {
        err.raw_os_error() == Some(libc::EWOULDBLOCK)
    }

    pub(super) fn error_unsupported(err: &Error) -> bool {
        match err.raw_os_error() {
            #[allow(unreachable_patterns)]
            Some(libc::ENOTSUP | libc::EOPNOTSUPP | libc::ENOSYS) => true,
            _ => false,
        }
    }

    #[cfg(not(target_os = "solaris"))]
    fn flock(file: &File, flag: libc::c_int) -> Result<()> {
        let ret = unsafe { libc::flock(file.as_raw_fd(), flag) };
        if ret < 0 {
            Err(Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(target_os = "solaris")]
    fn flock(file: &File, flag: libc::c_int) -> Result<()> {
        // Solaris lacks flock(), so try to emulate using fcntl()
        let mut flock = libc::flock {
            l_type: 0,
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_sysid: 0,
            l_pid: 0,
            l_pad: [0, 0, 0, 0],
        };
        flock.l_type = if flag & libc::LOCK_UN != 0 {
            libc::F_UNLCK
        } else if flag & libc::LOCK_EX != 0 {
            libc::F_WRLCK
        } else if flag & libc::LOCK_SH != 0 {
            libc::F_RDLCK
        } else {
            panic!("unexpected flock() operation")
        };

        let mut cmd = libc::F_SETLKW;
        if (flag & libc::LOCK_NB) != 0 {
            cmd = libc::F_SETLK;
        }

        let ret = unsafe { libc::fcntl(file.as_raw_fd(), cmd, &flock) };

        if ret < 0 {
            Err(Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::fs::File;
    use std::io::{Error, Result};
    use std::mem;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Foundation::{ERROR_INVALID_FUNCTION, ERROR_LOCK_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFile,
    };

    pub(super) fn lock_exclusive(file: &File) -> Result<()> {
        lock_file(file, LOCKFILE_EXCLUSIVE_LOCK)
    }

    pub(super) fn try_lock_exclusive(file: &File) -> Result<()> {
        lock_file(file, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY)
    }

    pub(super) fn error_contended(err: &Error) -> bool {
        err.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32)
    }

    pub(super) fn error_unsupported(err: &Error) -> bool {
        err.raw_os_error() == Some(ERROR_INVALID_FUNCTION as i32)
    }

    pub(super) fn unlock(file: &File) -> Result<()> {
        unsafe {
            let ret = UnlockFile(file.as_raw_handle() as HANDLE, 0, 0, !0, !0);
            if ret == 0 {
                Err(Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn lock_file(file: &File, flags: u32) -> Result<()> {
        unsafe {
            let mut overlapped = mem::zeroed();
            let ret = LockFileEx(
                file.as_raw_handle() as HANDLE,
                flags,
                0,
                !0,
                !0,
                &mut overlapped,
            );
            if ret == 0 {
                Err(Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}
