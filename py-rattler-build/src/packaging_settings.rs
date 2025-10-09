//! Python bindings for PackagingConfig
//!
//! This module provides Python wrappers for the Rust PackagingSettings struct,
//! which controls package format and compression settings.

use crate::error::RattlerBuildError;
use ::rattler_build::metadata::PackagingSettings as RustPackagingSettings;
use pyo3::prelude::*;
use rattler_conda_types::package::ArchiveType;

/// Python wrapper for ArchiveType enum.
///
/// Represents the package archive format.
///
/// Variants:
///     TarBz2: Traditional .tar.bz2 format
///     Conda: Modern .conda format (recommended)
#[pyclass(name = "ArchiveType")]
#[derive(Clone, Debug)]
pub enum PyArchiveType {
    /// Traditional tar.bz2 format (.tar.bz2)
    TarBz2,
    /// Modern conda format (.conda) - recommended
    Conda,
}

impl PyArchiveType {
    /// Convert Python wrapper to Rust type
    pub(crate) fn to_rust(&self) -> ArchiveType {
        match self {
            PyArchiveType::TarBz2 => ArchiveType::TarBz2,
            PyArchiveType::Conda => ArchiveType::Conda,
        }
    }

    /// Convert Rust type to Python wrapper
    pub(crate) fn from_rust(archive_type: ArchiveType) -> Self {
        match archive_type {
            ArchiveType::TarBz2 => PyArchiveType::TarBz2,
            ArchiveType::Conda => PyArchiveType::Conda,
        }
    }
}

#[pymethods]
impl PyArchiveType {
    /// Get the file extension for this archive type.
    ///
    /// Returns:
    ///     ".tar.bz2" for TarBz2, ".conda" for Conda
    fn extension(&self) -> &'static str {
        match self {
            PyArchiveType::TarBz2 => ".tar.bz2",
            PyArchiveType::Conda => ".conda",
        }
    }

    /// String representation of the ArchiveType.
    fn __repr__(&self) -> String {
        match self {
            PyArchiveType::TarBz2 => "ArchiveType.TarBz2".to_string(),
            PyArchiveType::Conda => "ArchiveType.Conda".to_string(),
        }
    }

    /// String representation.
    fn __str__(&self) -> &'static str {
        match self {
            PyArchiveType::TarBz2 => "tar.bz2",
            PyArchiveType::Conda => "conda",
        }
    }
}

/// Python wrapper for PackagingConfig struct.
///
/// PackagingConfig controls the package format and compression level
/// when creating conda packages.
///
/// Examples:
///     Create with default compression:
///     >>> settings = PackagingConfig.tar_bz2()
///     >>> settings = PackagingConfig.conda()
///
///     Create with custom compression:
///     >>> settings = PackagingConfig(ArchiveType.Conda, compression_level=10)
///     >>> settings = PackagingConfig.tar_bz2(compression_level=9)
#[pyclass(name = "PackagingConfig")]
#[derive(Clone, Debug)]
pub struct PyPackagingConfig {
    pub(crate) inner: RustPackagingSettings,
}

#[pymethods]
impl PyPackagingConfig {
    /// Create a new PackagingConfig.
    ///
    /// Args:
    ///     archive_type: The archive format (TarBz2 or Conda)
    ///     compression_level: Compression level (1-9 for tar.bz2, -7 to 22 for conda)
    ///
    /// Returns:
    ///     A new PackagingSettings instance
    ///
    /// Note:
    ///     - For tar.bz2: compression_level should be 1-9 (default 9)
    ///     - For conda: compression_level should be -7 to 22 (default 22)
    ///     - Higher values = better compression but slower
    #[new]
    #[pyo3(signature = (archive_type, compression_level=None))]
    fn new(archive_type: PyArchiveType, compression_level: Option<i32>) -> PyResult<Self> {
        let rust_archive_type = archive_type.to_rust();

        // Set appropriate defaults based on archive type
        let compression_level = if let Some(level) = compression_level {
            level
        } else {
            match rust_archive_type {
                ArchiveType::TarBz2 => 9, // Max compression for bzip2
                ArchiveType::Conda => 22, // Max compression for zstd
            }
        };

        // Validate compression levels
        match rust_archive_type {
            ArchiveType::TarBz2 => {
                if !(1..=9).contains(&compression_level) {
                    return Err(RattlerBuildError::Other(format!(
                        "Invalid compression level {} for tar.bz2. Must be 1-9.",
                        compression_level
                    ))
                    .into());
                }
            }
            ArchiveType::Conda => {
                if !(-7..=22).contains(&compression_level) {
                    return Err(RattlerBuildError::Other(format!(
                        "Invalid compression level {} for conda. Must be -7 to 22.",
                        compression_level
                    ))
                    .into());
                }
            }
        }

        Ok(PyPackagingConfig {
            inner: RustPackagingSettings {
                archive_type: rust_archive_type,
                compression_level,
            },
        })
    }

    /// Create PackagingSettings for tar.bz2 format.
    ///
    /// Args:
    ///     compression_level: Compression level (1-9, default 9)
    ///
    /// Returns:
    ///     PackagingSettings configured for tar.bz2
    #[staticmethod]
    #[pyo3(signature = (compression_level=9))]
    fn tar_bz2(compression_level: i32) -> PyResult<Self> {
        Self::new(PyArchiveType::TarBz2, Some(compression_level))
    }

    /// Create PackagingSettings for conda format (recommended).
    ///
    /// Args:
    ///     compression_level: Compression level (-7 to 22, default 22)
    ///
    /// Returns:
    ///     PackagingSettings configured for .conda format
    #[staticmethod]
    #[pyo3(signature = (compression_level=22))]
    fn conda(compression_level: i32) -> PyResult<Self> {
        Self::new(PyArchiveType::Conda, Some(compression_level))
    }

    /// Get the archive type.
    ///
    /// Returns:
    ///     The archive type (TarBz2 or Conda)
    #[getter]
    fn archive_type(&self) -> PyArchiveType {
        PyArchiveType::from_rust(self.inner.archive_type)
    }

    /// Set the archive type.
    ///
    /// Args:
    ///     value: The archive type to set
    #[setter]
    fn set_archive_type(&mut self, value: PyArchiveType) {
        self.inner.archive_type = value.to_rust();
    }

    /// Get the compression level.
    ///
    /// Returns:
    ///     The compression level (1-9 for tar.bz2, -7 to 22 for conda)
    #[getter]
    fn compression_level(&self) -> i32 {
        self.inner.compression_level
    }

    /// Set the compression level.
    ///
    /// Args:
    ///     value: The compression level
    ///
    /// Note:
    ///     - For tar.bz2: must be 1-9
    ///     - For conda: must be -7 to 22
    #[setter]
    fn set_compression_level(&mut self, value: i32) -> PyResult<()> {
        // Validate based on current archive type
        match self.inner.archive_type {
            ArchiveType::TarBz2 => {
                if !(1..=9).contains(&value) {
                    return Err(RattlerBuildError::Other(format!(
                        "Invalid compression level {} for tar.bz2. Must be 1-9.",
                        value
                    ))
                    .into());
                }
            }
            ArchiveType::Conda => {
                if !(-7..=22).contains(&value) {
                    return Err(RattlerBuildError::Other(format!(
                        "Invalid compression level {} for conda. Must be -7 to 22.",
                        value
                    ))
                    .into());
                }
            }
        }

        self.inner.compression_level = value;
        Ok(())
    }

    /// Get the file extension for the current archive type.
    ///
    /// Returns:
    ///     ".tar.bz2" or ".conda"
    fn extension(&self) -> &'static str {
        match self.inner.archive_type {
            ArchiveType::TarBz2 => ".tar.bz2",
            ArchiveType::Conda => ".conda",
        }
    }

    /// Check if this is using the tar.bz2 format.
    ///
    /// Returns:
    ///     True if using tar.bz2 format
    fn is_tar_bz2(&self) -> bool {
        matches!(self.inner.archive_type, ArchiveType::TarBz2)
    }

    /// Check if this is using the conda format.
    ///
    /// Returns:
    ///     True if using conda format
    fn is_conda(&self) -> bool {
        matches!(self.inner.archive_type, ArchiveType::Conda)
    }

    /// String representation of the PackagingSettings.
    fn __repr__(&self) -> String {
        format!(
            "PackagingSettings(archive_type={}, compression_level={})",
            match self.inner.archive_type {
                ArchiveType::TarBz2 => "TarBz2",
                ArchiveType::Conda => "Conda",
            },
            self.inner.compression_level
        )
    }

    /// Detailed string representation.
    fn __str__(&self) -> String {
        format!(
            "{} format with compression level {}",
            match self.inner.archive_type {
                ArchiveType::TarBz2 => "tar.bz2",
                ArchiveType::Conda => "conda",
            },
            self.inner.compression_level
        )
    }
}
