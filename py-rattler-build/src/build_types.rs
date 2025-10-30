use pyo3::prelude::*;
use rattler_build::types::{Directories, PackagingSettings};
use rattler_conda_types::package::ArchiveType;
use std::path::PathBuf;

use crate::error::RattlerBuildError;

/// Python wrapper for Directories
#[pyclass(name = "Directories")]
#[derive(Clone)]
pub struct PyDirectories {
    pub(crate) inner: Directories,
}

#[pymethods]
impl PyDirectories {
    /// Get the recipe directory
    #[getter]
    fn recipe_dir(&self) -> PathBuf {
        self.inner.recipe_dir.clone()
    }

    /// Get the work directory
    #[getter]
    fn work_dir(&self) -> PathBuf {
        self.inner.work_dir.clone()
    }

    /// Get the host prefix directory
    #[getter]
    fn host_prefix(&self) -> PathBuf {
        self.inner.host_prefix.clone()
    }

    /// Get the build prefix directory
    #[getter]
    fn build_prefix(&self) -> PathBuf {
        self.inner.build_prefix.clone()
    }

    /// Get the output directory
    #[getter]
    fn output_dir(&self) -> PathBuf {
        self.inner.output_dir.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "Directories(recipe_dir='{}', work_dir='{}', host_prefix='{}', build_prefix='{}', output_dir='{}')",
            self.inner.recipe_dir.display(),
            self.inner.work_dir.display(),
            self.inner.host_prefix.display(),
            self.inner.build_prefix.display(),
            self.inner.output_dir.display()
        )
    }
}

/// Python wrapper for PackagingSettings
#[pyclass(name = "PackagingSettings")]
#[derive(Clone)]
pub struct PyPackagingSettings {
    pub(crate) inner: PackagingSettings,
}

#[pymethods]
impl PyPackagingSettings {
    /// Create a new packaging settings
    #[new]
    #[pyo3(signature = (archive_type="conda", compression_level=None))]
    fn new(archive_type: &str, compression_level: Option<i32>) -> PyResult<Self> {
        let archive_type = match archive_type.to_lowercase().as_str() {
            "conda" => ArchiveType::Conda,
            "tar-bz2" | "tar.bz2" | "tarbz2" => ArchiveType::TarBz2,
            _ => {
                return Err(RattlerBuildError::Other(format!(
                    "Invalid archive type: {}. Must be 'conda' or 'tar-bz2'",
                    archive_type
                ))
                .into());
            }
        };

        // Default compression levels
        let compression_level = compression_level.unwrap_or_else(|| match archive_type {
            ArchiveType::Conda => 22, // zstd default
            ArchiveType::TarBz2 => 9, // bzip2 default
        });

        Ok(Self {
            inner: PackagingSettings {
                archive_type,
                compression_level,
            },
        })
    }

    /// Get the archive type as a string
    #[getter]
    fn archive_type(&self) -> String {
        match self.inner.archive_type {
            ArchiveType::Conda => "conda".to_string(),
            ArchiveType::TarBz2 => "tar-bz2".to_string(),
        }
    }

    /// Get the compression level
    #[getter]
    fn compression_level(&self) -> i32 {
        self.inner.compression_level
    }

    /// Set the archive type
    #[setter]
    fn set_archive_type(&mut self, archive_type: &str) -> PyResult<()> {
        self.inner.archive_type = match archive_type.to_lowercase().as_str() {
            "conda" => ArchiveType::Conda,
            "tar-bz2" | "tar.bz2" | "tarbz2" => ArchiveType::TarBz2,
            _ => {
                return Err(RattlerBuildError::Other(format!(
                    "Invalid archive type: {}. Must be 'conda' or 'tar-bz2'",
                    archive_type
                ))
                .into());
            }
        };
        Ok(())
    }

    /// Set the compression level
    #[setter]
    fn set_compression_level(&mut self, level: i32) {
        self.inner.compression_level = level;
    }

    fn __repr__(&self) -> String {
        format!(
            "PackagingSettings(archive_type='{}', compression_level={})",
            self.archive_type(),
            self.inner.compression_level
        )
    }
}

/// Register the build_types module with Python
pub fn register_build_types_module(py: Python<'_>, parent: &Bound<'_, PyModule>) -> PyResult<()> {
    let m = PyModule::new(py, "build_types")?;
    m.add_class::<PyDirectories>()?;
    m.add_class::<PyPackagingSettings>()?;
    parent.add_submodule(&m)?;
    Ok(())
}
