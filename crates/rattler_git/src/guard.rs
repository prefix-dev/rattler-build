//! Simple async file locking guard to replace pixi_utils::AsyncPrefixGuard
//!
//! File locking implementation adapted from cargo (MIT licensed).

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use tokio::fs;

use sys::{lock_exclusive, unlock};

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
    file: Option<File>,
    lock_path: PathBuf,
}

impl WriteGuard {
    async fn new(path: &Path) -> Result<Self, std::io::Error> {
        let lock_path = path.with_extension("lock");

        // Use tokio::task::spawn_blocking for the blocking file lock operation
        let (file, actual_lock_path) = tokio::task::spawn_blocking({
            let lock_path = lock_path.clone();
            move || -> std::io::Result<(File, PathBuf)> {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&lock_path)?;

                lock_exclusive(&file)?;

                Ok((file, lock_path))
            }
        })
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))??;

        Ok(Self {
            file: Some(file),
            lock_path: actual_lock_path,
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
        // Release the file lock
        if let Some(f) = self.file.take() {
            let _ = unlock(&f);
        }

        // Clean up any marker files
        let begin_path = self.lock_path.with_extension("begin");
        // We're disallowig fs::remove_file because we usually prefer fs_err. But here it's fine.
        #[allow(clippy::disallowed_methods)]
        let _ = std::fs::remove_file(&begin_path);
    }
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io::{Error, Result};
    use std::os::unix::io::AsRawFd;

    pub(super) fn lock_exclusive(file: &File) -> Result<()> {
        flock(file, libc::LOCK_EX)
    }

    pub(super) fn unlock(file: &File) -> Result<()> {
        flock(file, libc::LOCK_UN)
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
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LockFileEx, UnlockFile,
    };

    pub(super) fn lock_exclusive(file: &File) -> Result<()> {
        lock_file(file, LOCKFILE_EXCLUSIVE_LOCK)
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
