//! A GNU make jobserver shared by the builds that run in parallel.
//!
//! The pool is a named pipe filled once with `jobs` tokens. Build tools that
//! are jobserver clients (GNU make >= 4.4 and ninja >= 1.13 with `fifo:` auth)
//! take a token before starting each job beyond their first. rattler-build
//! takes one token for each build it runs and returns it when the build ends;
//! that token backs the implicit job slot of the build's top-level make or
//! ninja, so the total number of jobs stays at `jobs`.
//!
//! Only created when requested (`--parallel-builds BUILDS:JOBS`), and only
//! implemented on unix; elsewhere no jobserver is created and every build uses
//! its own job count, as before.

/// The jobserver of a parallel build. Removed when dropped.
pub(crate) struct Jobserver {
    #[cfg(unix)]
    jobs: usize,
    #[cfg(unix)]
    pool: unix::Pool,
}

/// A token taken from the jobserver; returned to it when dropped.
pub(crate) struct Token {
    #[cfg(unix)]
    _token: unix::Token,
}

impl Jobserver {
    /// Creates a jobserver for `jobs` concurrent jobs. Returns `None` (with a
    /// warning) if that is not possible.
    pub(crate) fn new(jobs: usize) -> Option<Self> {
        #[cfg(unix)]
        {
            match unix::Pool::new(jobs) {
                Ok(pool) => Some(Self { jobs, pool }),
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

    /// The `MAKEFLAGS` that advertise this jobserver to build tools.
    pub(crate) fn makeflags(&self) -> String {
        #[cfg(unix)]
        {
            format!(
                "-j{} --jobserver-auth=fifo:{}",
                self.jobs,
                self.pool.path().display()
            )
        }
        #[cfg(not(unix))]
        unreachable!("no jobserver is created on this platform")
    }

    /// Takes a token if one is free, without waiting.
    pub(crate) fn try_acquire(&self) -> Option<Token> {
        #[cfg(unix)]
        {
            self.pool.try_acquire().map(|token| Token { _token: token })
        }
        #[cfg(not(unix))]
        None
    }

    /// Waits for a token without blocking the thread.
    pub(crate) async fn acquire(&mut self) -> std::io::Result<Token> {
        #[cfg(unix)]
        {
            self.pool
                .acquire()
                .await
                .map(|token| Token { _token: token })
        }
        #[cfg(not(unix))]
        std::future::pending().await
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
        sync::Arc,
    };

    use tokio::{io::AsyncReadExt, net::unix::pipe};

    /// A named pipe holding jobserver tokens. The pipe lives in a temporary
    /// directory that is removed when the pool is dropped.
    pub(super) struct Pool {
        _dir: tempfile::TempDir,
        path: PathBuf,
        /// Read-write and non-blocking: taking a token never waits, the pipe
        /// never reports end of file, and returned tokens are written here.
        fifo: Arc<File>,
        /// For waiting on a token; created on first use, since it needs a
        /// running tokio runtime.
        receiver: Option<pipe::Receiver>,
    }

    /// A token taken from the pool; written back when dropped.
    pub(super) struct Token {
        byte: u8,
        fifo: Arc<File>,
    }

    impl Drop for Token {
        fn drop(&mut self) {
            if let Err(e) = (&*self.fifo).write_all(&[self.byte]) {
                tracing::warn!("Could not return a jobserver token: {e}");
            }
        }
    }

    /// Opens the pipe for reading and writing (so it never reports end of
    /// file), optionally non-blocking.
    fn open(path: &Path, nonblocking: bool) -> std::io::Result<File> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(if nonblocking { libc::O_NONBLOCK } else { 0 })
            .open(path)
    }

    impl Pool {
        /// Creates the pipe and fills it with `tokens` tokens.
        pub(super) fn new(tokens: usize) -> std::io::Result<Self> {
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
            let fifo = open(&path, true)?;
            (&fifo).write_all(&vec![b'+'; tokens])?;
            Ok(Self {
                _dir: dir,
                path,
                fifo: Arc::new(fifo),
                receiver: None,
            })
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }

        fn token(&self, byte: u8) -> Token {
            Token {
                byte,
                fifo: self.fifo.clone(),
            }
        }

        pub(super) fn try_acquire(&self) -> Option<Token> {
            let mut byte = [0u8; 1];
            match (&*self.fifo).read(&mut byte) {
                Ok(1) => Some(self.token(byte[0])),
                _ => None,
            }
        }

        /// Waits for a token. Cancel safe: a token is only taken when this
        /// returns.
        pub(super) async fn acquire(&mut self) -> std::io::Result<Token> {
            if self.receiver.is_none() {
                self.receiver = Some(pipe::Receiver::from_file(open(&self.path, false)?)?);
            }
            let receiver = self.receiver.as_mut().expect("created above");
            let mut byte = [0u8; 1];
            loop {
                // `read` is cancel safe, unlike `read_exact`.
                match receiver.read(&mut byte).await? {
                    1 => return Ok(self.token(byte[0])),
                    _ => continue,
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{
            io::{Read, Write},
            time::Duration,
        };

        use super::{Pool, open};

        /// Counts the tokens in the pool without consuming them.
        fn tokens_in(pool: &Pool) -> usize {
            let mut reader = open(pool.path(), true).unwrap();
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
        fn tokens_are_returned_when_dropped() {
            let pool = Pool::new(2).unwrap();
            assert_eq!(tokens_in(&pool), 2);

            let first = pool.try_acquire().unwrap();
            let second = pool.try_acquire().unwrap();
            assert!(pool.try_acquire().is_none());
            assert_eq!(tokens_in(&pool), 0);

            drop(first);
            assert_eq!(tokens_in(&pool), 1);
            drop(second);
            assert_eq!(tokens_in(&pool), 2);
        }

        #[tokio::test]
        async fn acquire_waits_for_a_returned_token() {
            let mut pool = Pool::new(1).unwrap();
            let held = pool.try_acquire().unwrap();

            let waiting = tokio::time::timeout(Duration::from_millis(200), pool.acquire()).await;
            assert!(waiting.is_err(), "no token is free yet");

            drop(held);
            let token = tokio::time::timeout(Duration::from_secs(5), pool.acquire())
                .await
                .expect("the returned token is taken")
                .unwrap();
            drop(token);
            assert_eq!(tokens_in(&pool), 1);
        }

        #[test]
        fn dropping_removes_the_fifo() {
            let pool = Pool::new(1).unwrap();
            let path = pool.path().to_path_buf();
            drop(pool);
            assert!(!path.exists());
        }
    }
}
