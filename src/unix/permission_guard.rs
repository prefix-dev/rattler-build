//! Implementation of the `PermissionGuard` struct.

/// User read/write permissions (0o600).
pub const READ_WRITE: u32 = 0o600;

#[cfg(unix)]
mod unix {
    use fs_err as fs;
    use std::fs::Permissions;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// A guard that modifies the permissions of a file and restores them when dropped.
    pub struct PermissionGuard {
        /// The path to the file.
        path: PathBuf,
        /// The original permissions of the file.
        original_permissions: Permissions,
    }

    impl PermissionGuard {
        /// Create a new `PermissionGuard` for the given path with the given permissions.
        pub fn new<P: AsRef<Path>>(path: P, permissions: u32) -> io::Result<Self> {
            let path = path.as_ref().to_path_buf();
            let metadata = fs::metadata(&path)?;
            let original_permissions = metadata.permissions();

            let new_permissions = Permissions::from_mode(original_permissions.mode() | permissions);

            // Set new permissions
            fs::set_permissions(&path, new_permissions)?;

            Ok(Self {
                path,
                original_permissions,
            })
        }
    }

    impl Drop for PermissionGuard {
        fn drop(&mut self) {
            if self.path.exists() {
                if let Err(e) = fs::set_permissions(&self.path, self.original_permissions.clone()) {
                    eprintln!("Failed to restore file permissions: {}", e);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use fs_err as fs;
        use fs_err::File;
        use tempfile::tempdir;

        #[test]
        fn test_permission_guard_modifies_and_restores() -> io::Result<()> {
            let dir = tempdir()?;
            let test_file = dir.path().join("test-restore.txt");
            File::create(&test_file)?;

            // Set initial permissions to 0o002 so we can check if the guard modifies them
            fs::set_permissions(&test_file, Permissions::from_mode(0o002))?;
            let initial_mode = fs::metadata(&test_file)?.permissions().mode();

            // Create scope for PermissionGuard
            {
                let _guard = PermissionGuard::new(&test_file, 0o200)?; // Write permission

                // Check permissions were modified
                let modified_mode = fs::metadata(&test_file)?.permissions().mode();
                assert_ne!(initial_mode, modified_mode);
                assert_eq!(modified_mode & 0o200, 0o200);
            }

            // Check permissions were restored after guard dropped
            let final_mode = fs::metadata(&test_file)?.permissions().mode();
            assert_eq!(initial_mode, final_mode);

            Ok(())
        }

        #[test]
        fn test_permission_guard_nonexistent_file() {
            let result = PermissionGuard::new("nonexistent_file", 0o777);
            assert!(result.is_err());
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::path::Path;

    pub struct PermissionGuard;

    impl PermissionGuard {
        /// Create a new `PermissionGuard` for the given path with the given permissions. Does nothing on Windows.
        pub fn new<P: AsRef<Path>>(_path: P, _permissions: u32) -> io::Result<Self> {
            Ok(Self)
        }
    }
}

#[cfg(unix)]
pub use self::unix::PermissionGuard;

#[cfg(windows)]
pub use self::windows::PermissionGuard;
