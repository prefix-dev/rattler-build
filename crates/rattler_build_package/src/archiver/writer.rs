//! Package archive writer

use std::fs::File;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rattler_conda_types::compression_level::CompressionLevel;
use rattler_package_streaming::write::{write_conda_package, write_tar_bz2_package};

use crate::{ArchiveType, PackageError, Result};

/// Writer for creating conda package archives
pub struct PackageWriter {
    /// Archive type to create
    archive_type: ArchiveType,

    /// Compression level
    compression_level: u8,

    /// Timestamp for reproducible builds
    timestamp: Option<DateTime<Utc>>,

    /// Number of compression threads
    compression_threads: usize,
}

impl PackageWriter {
    /// Create a new PackageWriter
    pub fn new(archive_type: ArchiveType, compression_level: u8) -> Self {
        Self {
            archive_type,
            compression_level,
            timestamp: None,
            compression_threads: 1,
        }
    }

    /// Set the timestamp for reproducible builds
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Set the number of compression threads
    pub fn with_compression_threads(mut self, threads: usize) -> Self {
        self.compression_threads = threads;
        self
    }

    /// Write a package to the given output file
    ///
    /// # Arguments
    /// * `output_path` - Where to write the package
    /// * `temp_dir` - Temporary directory containing all files to package
    /// * `files` - List of files to include (absolute paths within temp_dir)
    /// * `identifier` - Package identifier (name-version-build)
    pub fn write(
        &self,
        output_path: &Path,
        temp_dir: &Path,
        files: &[PathBuf],
        identifier: &str,
    ) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            fs_err::create_dir_all(parent)?;
        }

        // Create the output file
        let output_file = File::create(output_path)?;

        let compression = CompressionLevel::Numeric(self.compression_level as i32);

        match self.archive_type {
            ArchiveType::TarBz2 => {
                write_tar_bz2_package(
                    &output_file,
                    temp_dir,
                    files,
                    compression,
                    self.timestamp.as_ref(),
                    None, // No progress bar for now
                )
                .map_err(|e| PackageError::ArchiveCreation(e.to_string()))?;
            }
            ArchiveType::Conda => {
                write_conda_package(
                    &output_file,
                    temp_dir,
                    files,
                    compression,
                    Some(self.compression_threads as u32),
                    identifier,
                    self.timestamp.as_ref(),
                    None, // No progress bar for now
                )
                .map_err(|e| PackageError::ArchiveCreation(e.to_string()))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_err as fs;

    #[test]
    fn test_package_writer_tar_bz2() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let output_dir = tempfile::tempdir()?;

        // Create test files with proper structure
        let info_dir = temp_dir.path().join("info");
        fs::create_dir_all(&info_dir)?;

        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "hello world")?;

        let index_json = info_dir.join("index.json");
        fs::write(&index_json, r#"{"name":"test","version":"1.0.0"}"#)?;

        // Use TarBz2 format which is simpler
        let writer = PackageWriter::new(ArchiveType::TarBz2, 1);
        let output_path = output_dir.path().join("test.tar.bz2");

        // Files must be absolute paths!
        writer.write(
            &output_path,
            temp_dir.path(),
            &[test_file, index_json],
            "test-1.0.0-h12345_0",
        )?;

        assert!(output_path.exists());
        assert!(output_path.metadata()?.len() > 0);

        Ok(())
    }
}
