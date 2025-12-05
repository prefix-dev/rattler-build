//! File collection from directories

use globset::{Glob, GlobSetBuilder};
use std::path::PathBuf;
use walkdir::WalkDir;

use super::FileEntry;
use crate::{PackageError, Result};

/// Collects files from a directory for packaging
///
/// This struct provides a builder-pattern API for specifying which files
/// to include in a package.
pub struct FileCollector {
    /// Source directory to scan
    source_dir: PathBuf,

    /// Glob patterns to include
    include_patterns: GlobSetBuilder,

    /// Glob patterns to exclude
    exclude_patterns: GlobSetBuilder,

    /// Whether to follow symlinks
    follow_symlinks: bool,

    /// Whether to include hidden files
    include_hidden: bool,
}

impl FileCollector {
    /// Create a new FileCollector for the given directory
    pub fn new(source_dir: PathBuf) -> Self {
        Self {
            source_dir,
            include_patterns: GlobSetBuilder::new(),
            exclude_patterns: GlobSetBuilder::new(),
            follow_symlinks: false,
            include_hidden: false,
        }
    }

    /// Add a glob pattern to include files
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use rattler_build_package::FileCollector;
    /// # use std::path::Path;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let collector = FileCollector::new(Path::new("/source").to_path_buf())
    ///     .include_glob("**/*.so")?     // Include all .so files
    ///     .include_glob("bin/*")?;      // Include all files in bin/
    /// # Ok(())
    /// # }
    /// ```
    pub fn include_glob(mut self, pattern: &str) -> Result<Self> {
        let glob = Glob::new(pattern)?;
        self.include_patterns.add(glob);
        Ok(self)
    }

    /// Add a glob pattern to exclude files
    ///
    /// Exclusions take precedence over inclusions.
    pub fn exclude_glob(mut self, pattern: &str) -> Result<Self> {
        let glob = Glob::new(pattern)?;
        self.exclude_patterns.add(glob);
        Ok(self)
    }

    /// Set whether to follow symlinks when traversing directories
    pub fn follow_symlinks(mut self, follow: bool) -> Self {
        self.follow_symlinks = follow;
        self
    }

    /// Set whether to include hidden files (files starting with .)
    pub fn include_hidden(mut self, include: bool) -> Self {
        self.include_hidden = include;
        self
    }

    /// Collect all matching files
    ///
    /// Returns a Vec of FileEntry objects representing all files that match
    /// the configured patterns.
    pub fn collect(self) -> Result<Vec<FileEntry>> {
        let include_set = self.include_patterns.build()?;
        let exclude_set = self.exclude_patterns.build()?;

        let mut files = Vec::new();

        for entry in WalkDir::new(&self.source_dir).follow_links(self.follow_symlinks) {
            let entry = entry?;
            let path = entry.path();

            // Skip the root directory itself
            if path == self.source_dir {
                continue;
            }

            // Get relative path for pattern matching
            let relative_path = path
                .strip_prefix(&self.source_dir)
                .map_err(|e| PackageError::StripPrefix(e))?;

            // Skip hidden files if not included
            if !self.include_hidden {
                if let Some(file_name) = path.file_name() {
                    if file_name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                }
            }

            // Apply filters
            let should_include = if include_set.is_empty() {
                // If no include patterns, include everything
                true
            } else {
                include_set.is_match(relative_path)
            };

            let should_exclude = exclude_set.is_match(relative_path);

            if should_include && !should_exclude {
                // Skip directories - we only want files and symlinks
                if entry.file_type().is_dir() {
                    continue;
                }

                let file_entry = FileEntry::from_paths(path, relative_path)?;
                files.push(file_entry);
            }
        }

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_collector_basic() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let base = temp_dir.path();

        // Create test files
        fs::create_dir_all(base.join("bin"))?;
        fs::create_dir_all(base.join("lib"))?;
        fs::write(base.join("bin/tool"), "#!/bin/bash")?;
        fs::write(base.join("lib/libfoo.so"), "binary")?;
        fs::write(base.join("lib/libfoo.a"), "binary")?;

        let collector = FileCollector::new(base.to_path_buf());
        let files = collector.collect()?;

        assert_eq!(files.len(), 3);

        Ok(())
    }

    #[test]
    fn test_collector_with_glob() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let base = temp_dir.path();

        fs::create_dir_all(base.join("lib"))?;
        fs::write(base.join("lib/libfoo.so"), "binary")?;
        fs::write(base.join("lib/libfoo.a"), "binary")?;

        let collector = FileCollector::new(base.to_path_buf())
            .include_glob("**/*.so")?
            .exclude_glob("**/*.a")?;

        let files = collector.collect()?;

        assert_eq!(files.len(), 1);
        assert!(files[0].destination.to_string_lossy().ends_with(".so"));

        Ok(())
    }
}
