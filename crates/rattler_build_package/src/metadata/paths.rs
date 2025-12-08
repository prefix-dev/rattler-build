//! PathsJson builder

use fs_err as fs;
use rattler_conda_types::Platform;
use rattler_conda_types::package::{FileMode, PathType, PathsEntry, PathsJson, PrefixPlaceholder};
use rattler_digest::{compute_bytes_digest, compute_file_digest};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use super::PrefixDetectionConfig;
use crate::files::FileEntry;
use crate::{PackageError, Result};

/// Builder for creating PathsJson metadata
///
/// This generates the paths.json file which contains information about
/// all files in the package, including their SHA256 hashes and prefix
/// placeholders.
#[derive(Debug)]
pub struct PathsJsonBuilder {
    /// Prefix directory where files are staged
    prefix: PathBuf,

    /// Files to include in paths.json
    files: Vec<FileEntry>,

    /// Target platform
    target_platform: Platform,

    /// Prefix detection configuration
    prefix_detection: PrefixDetectionConfig,
}

impl PathsJsonBuilder {
    /// Create a new PathsJsonBuilder
    pub fn new(prefix: PathBuf, target_platform: Platform) -> Self {
        Self {
            prefix,
            files: Vec::new(),
            target_platform,
            prefix_detection: PrefixDetectionConfig::default(),
        }
    }

    /// Add a file to the paths.json
    pub fn add_file(mut self, file: FileEntry) -> Self {
        self.files.push(file);
        self
    }

    /// Add multiple files
    pub fn add_files(mut self, files: Vec<FileEntry>) -> Self {
        self.files.extend(files);
        self
    }

    /// Set the prefix detection configuration
    pub fn with_prefix_detection(mut self, config: PrefixDetectionConfig) -> Self {
        self.prefix_detection = config;
        self
    }

    /// Build the PathsJson
    ///
    /// This will:
    /// - Compute SHA256 hashes for all files (in parallel)
    /// - Detect prefix placeholders
    /// - Create the appropriate PathsEntry for each file
    pub fn build(self) -> Result<PathsJson> {
        // Process files in parallel
        let entries: Vec<Result<PathsEntry>> = self
            .files
            .par_iter()
            .map(|file| self.create_paths_entry(file))
            .collect();

        // Collect results, propagating errors
        let mut paths = Vec::new();
        for entry in entries {
            paths.push(entry?);
        }

        Ok(PathsJson {
            paths,
            paths_version: 1,
        })
    }

    /// Create a PathsEntry for a single file
    fn create_paths_entry(&self, file: &FileEntry) -> Result<PathsEntry> {
        let metadata = fs::symlink_metadata(&file.source)?;

        if metadata.is_dir() {
            // Check if directory is empty
            let mut entries = fs::read_dir(&file.source)?;
            if entries.next().is_none() {
                // Empty directory
                return Ok(PathsEntry {
                    sha256: None,
                    relative_path: file.destination.clone(),
                    path_type: PathType::Directory,
                    prefix_placeholder: None,
                    no_link: false,
                    size_in_bytes: None,
                });
            }
            // Non-empty directories are not included in paths.json
            // (their contents are)
            return Err(PackageError::InvalidMetadata(format!(
                "Non-empty directory should not be in file list: {:?}",
                file.destination
            )));
        }

        if metadata.is_symlink() {
            // For symlinks, compute hash of target content if it's a file
            let digest = if self.is_symlink_to_file(&file.source) {
                compute_file_digest::<sha2::Sha256>(&file.source)?
            } else {
                compute_bytes_digest::<sha2::Sha256>(&[])
            };

            return Ok(PathsEntry {
                sha256: Some(digest),
                relative_path: file.destination.clone(),
                path_type: PathType::SoftLink,
                prefix_placeholder: None,
                no_link: false,
                size_in_bytes: Some(metadata.len()),
            });
        }

        // Regular file
        let file_size = metadata.len();
        let digest = if file_size > 0 {
            Some(compute_file_digest::<sha2::Sha256>(&file.source)?)
        } else {
            Some(compute_bytes_digest::<sha2::Sha256>(&[]))
        };

        // Detect prefix placeholder
        let prefix_placeholder =
            if self.prefix_detection.detect_binary || self.prefix_detection.detect_text {
                self.detect_prefix(&file.source, file.content_type.as_ref())?
            } else {
                None
            };

        Ok(PathsEntry {
            sha256: digest,
            relative_path: file.destination.clone(),
            path_type: PathType::HardLink,
            prefix_placeholder,
            no_link: false,
            size_in_bytes: Some(file_size),
        })
    }

    /// Check if a symlink points to a file
    fn is_symlink_to_file(&self, path: &Path) -> bool {
        match path.canonicalize() {
            Ok(canonical) => canonical.is_file(),
            Err(_) => false,
        }
    }

    /// Detect prefix placeholder in a file
    fn detect_prefix(
        &self,
        file_path: &Path,
        content_type: Option<&content_inspector::ContentType>,
    ) -> Result<Option<PrefixPlaceholder>> {
        use content_inspector::ContentType;

        // Skip .pyc and .pyo files
        if let Some(ext) = file_path.extension()
            && (ext == "pyc" || ext == "pyo")
        {
            return Ok(None);
        }

        // Check if file matches ignore patterns
        // TODO: Implement pattern matching against prefix_detection.ignore_patterns

        let content_type = content_type
            .ok_or_else(|| PackageError::ContentTypeNotFound(file_path.to_path_buf()))?;

        // Determine if this is a text file
        let is_text = content_type.is_text()
            && matches!(content_type, ContentType::UTF_8 | ContentType::UTF_8_BOM);

        let file_mode = if is_text && self.prefix_detection.detect_text {
            // Try text detection
            match crate::prefix::detect_prefix_text(file_path, &self.prefix)? {
                Some(placeholder) => {
                    return Ok(Some(PrefixPlaceholder {
                        file_mode: FileMode::Text,
                        placeholder,
                    }));
                }
                None => FileMode::Text,
            }
        } else {
            FileMode::Binary
        };

        // Binary prefix detection (Unix only, if enabled)
        if file_mode == FileMode::Binary && self.prefix_detection.detect_binary {
            // Skip binary detection on Windows
            if self.target_platform.is_windows() {
                return Ok(None);
            }

            if crate::prefix::detect_prefix_binary(file_path, &self.prefix)? {
                return Ok(Some(PrefixPlaceholder {
                    file_mode: FileMode::Binary,
                    placeholder: self.prefix.to_string_lossy().to_string(),
                }));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_paths_builder_empty() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let builder = PathsJsonBuilder::new(temp_dir.path().to_path_buf(), Platform::Linux64);
        let paths = builder.build()?;

        assert_eq!(paths.paths_version, 1);
        assert_eq!(paths.paths.len(), 0);

        Ok(())
    }

    #[test]
    fn test_paths_builder_with_files() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let base = temp_dir.path();

        // Create test files
        fs::write(base.join("test1.txt"), "hello")?;
        fs::write(base.join("test2.txt"), "world")?;

        let file1 = FileEntry::from_paths(&base.join("test1.txt"), &PathBuf::from("test1.txt"))?;
        let file2 = FileEntry::from_paths(&base.join("test2.txt"), &PathBuf::from("test2.txt"))?;

        let builder = PathsJsonBuilder::new(base.to_path_buf(), Platform::Linux64)
            .add_file(file1)
            .add_file(file2);

        let paths = builder.build()?;

        assert_eq!(paths.paths.len(), 2);
        assert_eq!(paths.paths[0].relative_path, PathBuf::from("test1.txt"));
        assert_eq!(paths.paths[1].relative_path, PathBuf::from("test2.txt"));

        // Check that SHA256 was computed
        assert!(paths.paths[0].sha256.is_some());
        assert!(paths.paths[1].sha256.is_some());

        Ok(())
    }

    #[test]
    fn test_paths_builder_with_prefix() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let base = temp_dir.path();

        // Create a file with the prefix in it
        let content = format!("This file is in {}", base.display());
        fs::write(base.join("test.txt"), content)?;

        let file = FileEntry::from_paths(&base.join("test.txt"), &PathBuf::from("test.txt"))?;

        let builder = PathsJsonBuilder::new(base.to_path_buf(), Platform::Linux64).add_file(file);

        let paths = builder.build()?;

        assert_eq!(paths.paths.len(), 1);

        // Should have detected the prefix
        if let Some(placeholder) = &paths.paths[0].prefix_placeholder {
            assert_eq!(placeholder.file_mode, FileMode::Text);
        }

        Ok(())
    }
}
