//! Utility functions for working with paths.

use fs_err as fs;
#[cfg(windows)]
use retry_policies::{RetryDecision, RetryPolicy, policies::ExponentialBackoff};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(windows)]
use std::time::Duration;
use std::{
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

use miette::IntoDiagnostic;

/// Converts `p` to an absolute path, but doesn't resolve symlinks.
/// The function does normalize the path by resolving any `.` and `..` components which are present.
/// Usually, the `base_path` would be set to the current working directory.
pub fn to_lexical_absolute(p: &Path, base_path: &Path) -> PathBuf {
    let mut absolute = if p.is_absolute() {
        PathBuf::new()
    } else {
        base_path.to_path_buf()
    };
    for component in p.components() {
        match component {
            Component::CurDir => { /* do nothing for `.` components */ }
            Component::ParentDir => {
                // pop the last element that we added for `..` components
                absolute.pop();
            }
            // just push the component for any other component
            component => absolute.push(component.as_os_str()),
        }
    }
    absolute
}

/// Convert a path to a string with forward slashes (only on windows). Otherwise,
/// just return the path as a string.
pub fn to_forward_slash_lossy(path: &Path) -> std::borrow::Cow<'_, str> {
    #[cfg(target_os = "windows")]
    {
        let mut buf = String::new();
        for c in path.components() {
            match c {
                Component::RootDir => { /* root on windows can be skipped */ }
                Component::CurDir => buf.push('.'),
                Component::ParentDir => buf.push_str(".."),
                Component::Prefix(prefix) => {
                    buf.push_str(&prefix.as_os_str().to_string_lossy());
                    continue;
                }
                Component::Normal(s) => buf.push_str(&s.to_string_lossy()),
            }
            // use `/` instead of `\`
            buf.push('/');
        }

        fn ends_with_main_sep(p: &Path) -> bool {
            use std::os::windows::ffi::OsStrExt as _;
            p.as_os_str().encode_wide().last() == Some(std::path::MAIN_SEPARATOR as u16)
        }
        if buf != "/" && !ends_with_main_sep(path) && buf.ends_with('/') {
            buf.pop();
        }

        std::borrow::Cow::Owned(buf)
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string_lossy()
    }
}

/// Returns the UNIX epoch time in seconds.
pub fn get_current_timestamp() -> miette::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .into_diagnostic()?
        .as_secs())
}

/// Removes a directory and all its contents, including read-only files.
#[allow(clippy::disallowed_methods)]
pub fn remove_dir_all_force(path: &Path) -> std::io::Result<()> {
    // Using std::fs to get proper error codes on Windows
    let result = match std::fs::remove_dir_all(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // If the normal removal fails, try to forcefully remove it.
            let _ = make_path_writable(path);
            std::fs::remove_dir_all(path)
        }
        Err(e) => Err(e),
    };

    #[cfg(windows)]
    {
        if let Err(e) = result {
            return try_remove_with_retry(path, Some(e));
        }
    }

    if let Err(err) = &result {
        tracing::warn!("Failed to remove directory {:?}: {}", path, err);
    } else {
        tracing::debug!("Removed directory {:?}", path);
    }

    result
}

#[cfg(windows)]
fn try_remove_with_retry(path: &Path, last_err: Option<std::io::Error>) -> std::io::Result<()> {
    // On Windows, rename the directory to a temporary sibling first.
    // Rename succeeds even when files inside are locked (by antivirus,
    // indexers, etc.) because open handles follow the renamed path.
    // This frees the original path immediately; the actual deletion can
    // then proceed on the renamed path without blocking the caller.
    let trash_path = pending_removal_path(path);
    let (target, renamed) = match std::fs::rename(path, &trash_path) {
        Ok(()) => {
            tracing::debug!("Renamed {:?} → {:?} for deferred deletion", path, trash_path);
            (&*trash_path, true)
        }
        Err(rename_err) => {
            tracing::debug!("Rename failed for {:?}: {rename_err}, retrying in-place", path);
            (path, false)
        }
    };

    // Try to actually delete (with retries).
    let retry_policy = ExponentialBackoff::builder()
        .base(2)
        .retry_bounds(Duration::from_millis(100), Duration::from_secs(2))
        .build_with_max_retries(5);

    let mut current_try: u32 = 0;
    let mut last_err = if renamed { None } else { last_err };
    let request_start = SystemTime::now();

    loop {
        if let Some(err) = &last_err {
            match retry_policy.should_retry(request_start, current_try) {
                RetryDecision::DoNotRetry => {
                    if renamed {
                        // Original path is already free — leave the trash dir
                        // for later cleanup and report success.
                        tracing::warn!(
                            "Could not delete {:?} (last error: {err}), \
                             leaving for later cleanup",
                            trash_path
                        );
                        return Ok(());
                    }
                    return Err(
                        last_err
                            .unwrap_or_else(|| std::io::Error::other("Directory could not be deleted")),
                    );
                }
                RetryDecision::Retry { execute_after } => {
                    let sleep_for = execute_after
                        .duration_since(SystemTime::now())
                        .unwrap_or(Duration::ZERO);

                    tracing::info!("Retrying deletion {}/{}: {}", current_try + 1, 5, err);

                    std::thread::sleep(sleep_for);
                }
            }
        }

        // Try to make the directory writable before removal
        if target.exists() {
            let _ = make_path_writable(target);
        }

        // Note: do not use `fs_err` here, it will not give us the correct error code!
        #[allow(clippy::disallowed_methods)]
        match std::fs::remove_dir_all(target) {
            Ok(_) => return Ok(()),
            Err(e) if matches!(e.raw_os_error(), Some(32) | Some(5)) => {
                last_err = Some(e);
                current_try += 1;
            }
            Err(e) => {
                if renamed {
                    tracing::warn!(
                        "Could not delete {:?}: {e}, leaving for later cleanup",
                        trash_path
                    );
                    return Ok(());
                }
                return Err(std::io::Error::other(format!(
                    "Failed to remove directory {:?}: {}",
                    path, e
                )));
            }
        }
    }
}

/// Generate a unique sibling path for rename-before-delete.
#[cfg(windows)]
fn pending_removal_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{name}.pending-rm-{unique}"))
}

/// Make the path and any children writable by adjusting permissions.
fn make_path_writable(path: &Path) -> std::io::Result<()> {
    // If the normal removal fails, try to forcefully remove it.
    tracing::debug!(
        "Adjusting permissions to remove read-only files in {}.",
        path.display()
    );
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let file_path = entry.path();
        let metadata = fs::metadata(file_path)?;
        let mut permissions = metadata.permissions();

        if permissions.readonly() {
            // Set only the user write bit
            #[cfg(unix)]
            permissions.set_mode(permissions.mode() | 0o200);
            #[cfg(windows)]
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(file_path, permissions)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_lexical_absolute() {
        let path = Path::new("/foo/bar/../baz");
        let absolute = to_lexical_absolute(path, &PathBuf::new());
        assert_eq!(absolute, Path::new("/foo/baz"));
    }

    #[test]
    fn test_to_forward_slash_lossy() {
        #[cfg(windows)]
        {
            let path = Path::new(r"C:\\foo\\bar\\baz");
            let forward_slash = to_forward_slash_lossy(path);
            assert_eq!(forward_slash, "C:/foo/bar/baz");
        }

        #[cfg(unix)]
        {
            let path = Path::new("/foo/bar/baz");
            let forward_slash = to_forward_slash_lossy(path);
            assert_eq!(forward_slash, "/foo/bar/baz");
        }
    }

    #[cfg(windows)]
    mod try_remove_with_retry_tests {
        use super::*;
        use std::fs::File;
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;
        use tempfile::TempDir;

        // Windows sharing flags
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_DELETE: u32 = 0x4;

        #[test]
        fn test_successful_removal() -> std::io::Result<()> {
            let temp_dir = TempDir::new()?;
            let dir_path = temp_dir.path().to_path_buf();

            std::mem::forget(temp_dir);
            let file_path = dir_path.join("test.txt");

            File::create(&file_path)?;
            let result = try_remove_with_retry(&dir_path, None);
            assert!(result.is_ok());
            assert!(!dir_path.exists());

            Ok(())
        }

        #[test]
        fn test_nonexistent_path() {
            let nonexistent_path = PathBuf::from("/nonexistent/path/that/does/not/exist");
            let result = try_remove_with_retry(&nonexistent_path, None);
            assert!(result.is_err());
        }

        #[test]
        fn test_locked_file_rename_and_remove() -> std::io::Result<()> {
            let temp_dir = TempDir::new()?;
            let dir_path = temp_dir.keep();
            let file_path = dir_path.join("locked.txt");

            // Simulate a realistic antivirus/indexer lock:
            // FILE_SHARE_READ | FILE_SHARE_DELETE allows rename of the
            // parent directory but removal may fail because the file
            // isn't fully gone until the handle is closed.
            //
            // share_mode(0) (exclusive) blocks rename too, but that's
            // not what real-world tools use.
            let _locked_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
                .open(&file_path)?;

            // try_remove_with_retry should succeed: it renames the
            // directory out of the way, then either deletes or leaves
            // the trash dir for later cleanup.
            let result = try_remove_with_retry(&dir_path, None);
            assert!(
                result.is_ok(),
                "Should succeed via rename with antivirus-style lock: {:?}",
                result.err()
            );
            assert!(
                !dir_path.exists(),
                "Original path should be gone (renamed away)"
            );

            // Drop the lock so the trash dir can be cleaned up.
            drop(_locked_file);

            Ok(())
        }

        #[test]
        // Original code for GO permission issues is tested here
        fn test_readonly_file_removal() -> std::io::Result<()> {
            let temp_dir = TempDir::new()?;
            let dir_path = temp_dir.path().to_path_buf();
            let file_path = dir_path.join("readonly.txt");
            {
                let mut file = File::create(&file_path)?;
                file.write_all(b"Test content")?;
            }

            let metadata = fs::metadata(&file_path)?;
            let mut permissions = metadata.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&file_path, permissions)?;
            std::mem::forget(temp_dir);

            let metadata = fs::metadata(&file_path)?;
            assert!(
                metadata.permissions().readonly(),
                "File should be read-only"
            );

            let result = remove_dir_all_force(&dir_path);

            assert!(
                result.is_ok(),
                "Directory removal failed: {:?}",
                result.err()
            );
            assert!(!dir_path.exists(), "Directory still exists!");
            Ok(())
        }

        #[test]
        // We are using remove_dir_all on retry logic, so it will even clear read-only files
        // This is for testing’s sake only for permission-related issues
        fn test_readonly_file_removal_with_retry() -> std::io::Result<()> {
            let temp_dir = TempDir::new()?;
            let dir_path = temp_dir.path().to_path_buf();
            let file_path = dir_path.join("readonly.txt");
            {
                let mut file = File::create(&file_path)?;
                file.write_all(b"Test content")?;
            }

            let metadata = fs::metadata(&file_path)?;
            let mut permissions = metadata.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&file_path, permissions)?;
            std::mem::forget(temp_dir);

            let metadata = fs::metadata(&file_path)?;
            assert!(
                metadata.permissions().readonly(),
                "File should be read-only"
            );

            let result = try_remove_with_retry(&dir_path, None);

            assert!(
                result.is_ok(),
                "Directory removal failed: {:?}",
                result.err()
            );
            assert!(!dir_path.exists(), "Directory still exists!");
            Ok(())
        }
    }
}
