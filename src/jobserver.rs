//! A GNU make jobserver shared by the builds that run in parallel.
//!
//! Build tools that are jobserver clients (GNU make >= 4.4 and ninja >= 1.13
//! with `fifo:` auth) take a token before starting each job beyond their
//! first, so the total number of jobs across all running builds stays within
//! the configured limit. Each running build holds one implicit token, so the
//! pool holds `jobs - running builds` tokens and is resized as builds start
//! and finish.
//!
//! Only implemented on unix (named pipe); on other platforms no jobserver is
//! created and every build uses its own job count, as before.

/// The jobserver of a parallel build. Advertised to build scripts through
/// `MAKEFLAGS` until dropped.
pub(crate) struct Jobserver {
    #[cfg(unix)]
    jobs: usize,
    #[cfg(unix)]
    pool: unix::Pool,
}

impl Jobserver {
    /// Creates a jobserver for `jobs` concurrent jobs and advertises it to
    /// build scripts. Returns `None` (with a warning) if that is not possible.
    pub(crate) fn new(jobs: usize) -> Option<Self> {
        #[cfg(unix)]
        {
            match unix::Pool::new() {
                Ok(pool) => {
                    let makeflags =
                        format!("-j{jobs} --jobserver-auth=fifo:{}", pool.path().display());
                    tracing::info!("Using a jobserver for {jobs} jobs: MAKEFLAGS='{makeflags}'");
                    rattler_build_core::env_vars::set_jobserver_makeflags(Some(makeflags));
                    Some(Self { jobs, pool })
                }
                Err(e) => {
                    tracing::warn!(
                        "Could not create a jobserver, builds will not share a job limit: {e}"
                    );
                    None
                }
            }
        }
        #[cfg(not(unix))]
        {
            tracing::warn!(
                "A shared jobserver is only available on unix; builds will not share a job limit"
            );
            let _ = jobs;
            None
        }
    }

    /// Sizes the token pool for `running` builds in flight.
    pub(crate) fn set_running_builds(&mut self, running: usize) {
        #[cfg(unix)]
        self.pool.resize(self.jobs.saturating_sub(running.max(1)));
        #[cfg(not(unix))]
        let _ = running;
    }
}

impl Drop for Jobserver {
    fn drop(&mut self) {
        rattler_build_core::env_vars::set_jobserver_makeflags(None);
    }
}

#[cfg(unix)]
mod unix {
    use std::{
        ffi::CString,
        fs::File,
        io::{Read, Write},
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        path::{Path, PathBuf},
    };

    /// A named pipe holding jobserver tokens. The pipe lives in a temporary
    /// directory that is removed when the pool is dropped.
    pub(super) struct Pool {
        _dir: tempfile::TempDir,
        path: PathBuf,
        fifo: File,
        issued: usize,
    }

    impl Pool {
        pub(super) fn new() -> std::io::Result<Self> {
            let dir = tempfile::Builder::new()
                .prefix("rattler-build-jobserver-")
                .tempdir()?;
            let path = dir.path().join("fifo");
            let c_path =
                CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
            // SAFETY: `c_path` is a valid NUL-terminated path.
            if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Read-write so the pipe never reports EOF; non-blocking so that
            // taking tokens back never waits for running jobs.
            let fifo = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)?;
            Ok(Self {
                _dir: dir,
                path,
                fifo,
                issued: 0,
            })
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        /// Moves the number of tokens in the pool towards `target`. Tokens
        /// held by running jobs cannot be taken back; that is retried on the
        /// next call.
        pub(super) fn resize(&mut self, target: usize) {
            while self.issued < target {
                if self.fifo.write_all(b"+").is_err() {
                    break;
                }
                self.issued += 1;
            }
            let mut token = [0u8; 1];
            while self.issued > target {
                match self.fifo.read(&mut token) {
                    Ok(1) => self.issued -= 1,
                    _ => break,
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{
            io::{Read, Write},
            os::unix::fs::OpenOptionsExt,
        };

        use super::Pool;

        /// Counts the tokens in the pool without consuming them.
        fn tokens_in(pool: &Pool) -> usize {
            let mut reader = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(pool.path())
                .unwrap();
            let mut buf = [0u8; 64];
            match reader.read(&mut buf) {
                Ok(n) => {
                    reader.write_all(&buf[..n]).unwrap();
                    n
                }
                Err(_) => 0,
            }
        }

        #[test]
        fn resize_adds_and_takes_back_tokens() {
            let mut pool = Pool::new().unwrap();
            assert!(pool.path().exists());

            pool.resize(3);
            assert_eq!(tokens_in(&pool), 3);

            pool.resize(1);
            assert_eq!(tokens_in(&pool), 1);

            pool.resize(0);
            assert_eq!(tokens_in(&pool), 0);
        }

        #[test]
        fn dropping_removes_the_fifo() {
            let pool = Pool::new().unwrap();
            let path = pool.path().to_path_buf();
            drop(pool);
            assert!(!path.exists());
        }
    }
}
