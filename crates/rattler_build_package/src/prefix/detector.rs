//! Prefix placeholder detection in files

use rattler_conda_types::package::FileMode;
use std::path::Path;

use crate::Result;

/// A detected prefix placeholder
#[derive(Debug, Clone)]
pub struct PrefixPlaceholder {
    /// The file mode (text or binary)
    pub file_mode: FileMode,

    /// The actual placeholder string found
    pub placeholder: String,
}

/// Detect if a binary file contains the given prefix
///
/// # Arguments
/// * `file_path` - Path to the file to check
/// * `prefix` - The prefix bytes to search for
///
/// This uses memchr for efficient binary searching.
#[cfg(target_family = "unix")]
pub fn detect_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool> {
    use fs_err::File;
    use std::os::unix::ffi::OsStrExt;

    let prefix_bytes = prefix.as_os_str().as_bytes();
    let file = File::open(file_path)?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };

    Ok(memchr::memmem::find(mmap.as_ref(), prefix_bytes).is_some())
}

#[cfg(target_family = "windows")]
pub fn detect_prefix_binary(_file_path: &Path, _prefix: &Path) -> Result<bool> {
    // Binary prefix detection not supported on Windows
    tracing::debug!("Binary prefix detection is not supported on Windows");
    Ok(false)
}

/// Detect if a text file contains the given prefix
///
/// Returns the actual prefix string found (which may differ in slashes on Windows)
pub fn detect_prefix_text(file_path: &Path, prefix: &Path) -> Result<Option<String>> {
    use fs_err::File;

    let file = File::open(file_path)?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };

    let prefix_string = prefix.to_string_lossy().to_string();
    let mut detected_prefix = None;

    // Check for the prefix
    if memchr::memmem::find(mmap.as_ref(), prefix_string.as_bytes()).is_some() {
        detected_prefix = Some(prefix_string);
    }

    // On Windows, also check for forward-slash version
    #[cfg(target_family = "windows")]
    {
        use std::borrow::Cow;

        let forward_slash: Cow<'_, str> = prefix.to_string_lossy().replace('\\', "/").into();

        if memchr::memmem::find(mmap.as_ref(), forward_slash.as_bytes()).is_some() {
            if detected_prefix.is_some() {
                return Err(crate::PackageError::MixedPrefixPlaceholders(
                    file_path.to_path_buf(),
                ));
            }
            detected_prefix = Some(forward_slash.to_string());
        }
    }

    Ok(detected_prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_prefix_text() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let prefix = temp_dir.path().join("my_prefix");
        let file_path = temp_dir.path().join("test.txt");

        let content = format!("This file is in {}", prefix.display());
        fs::write(&file_path, content)?;

        let result = detect_prefix_text(&file_path, &prefix)?;
        assert!(result.is_some());

        Ok(())
    }

    #[test]
    fn test_no_prefix() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let prefix = temp_dir.path().join("absent");
        let file_path = temp_dir.path().join("test.txt");

        fs::write(&file_path, "Nothing here")?;

        let result = detect_prefix_text(&file_path, &prefix)?;
        assert!(result.is_none());

        Ok(())
    }
}
