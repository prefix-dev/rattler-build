//! Python bindings for Directories
//!
//! This module provides Python wrappers for the Rust Directories struct,
//! which represents the directory structure used during package builds.

use crate::error::RattlerBuildError;
use ::rattler_build::metadata::Directories as RustDirectories;
use pyo3::prelude::*;
use std::path::PathBuf;

/// Python wrapper for Directories struct.
///
/// Directories represents the various paths and directories used during
/// the conda package build process, including recipe, cache, work, host
/// and build directories.
///
/// Examples:
///     Access directory information:
///     >>> dirs = get_build_directories()  # From a build context
///     >>> print(dirs.recipe_dir)
///     >>> print(dirs.work_dir)
///     >>> print(dirs.host_prefix)
#[pyclass(name = "Directories")]
#[derive(Clone, Debug)]
pub struct PyDirectories {
    pub(crate) inner: RustDirectories,
}

#[pymethods]
impl PyDirectories {
    /// Get the recipe directory path.
    ///
    /// The directory where the recipe is located.
    ///
    /// Returns:
    ///     Path to the recipe directory
    #[getter]
    fn recipe_dir(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("recipe_dir")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the recipe file path.
    ///
    /// The path to the recipe file itself.
    ///
    /// Returns:
    ///     Path to the recipe file
    #[getter]
    fn recipe_path(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("recipe_path")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the cache directory path.
    ///
    /// The folder where the build cache is located.
    ///
    /// Returns:
    ///     Path to the cache directory
    #[getter]
    fn cache_dir(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("cache_dir")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the host prefix path.
    ///
    /// The directory where host dependencies are installed.
    /// Exposed as $PREFIX (or %PREFIX% on Windows) in the build script.
    ///
    /// Returns:
    ///     Path to the host prefix directory
    #[getter]
    fn host_prefix(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("host_prefix")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the build prefix path.
    ///
    /// The directory where build dependencies are installed.
    /// Exposed as $BUILD_PREFIX (or %BUILD_PREFIX% on Windows) in the build script.
    ///
    /// Returns:
    ///     Path to the build prefix directory
    #[getter]
    fn build_prefix(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("build_prefix")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the work directory path.
    ///
    /// The directory where the source code is copied to and built from.
    ///
    /// Returns:
    ///     Path to the work directory
    #[getter]
    fn work_dir(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("work_dir")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the build directory path.
    ///
    /// The parent directory of host, build and work directories.
    ///
    /// Returns:
    ///     Path to the build directory
    #[getter]
    fn build_dir(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("build_dir")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// Get the output directory path.
    ///
    /// The output directory or local channel directory where packages are written.
    ///
    /// Returns:
    ///     Path to the output directory
    #[getter]
    fn output_dir(&self) -> PyResult<PathBuf> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let path: PathBuf = serde_json::from_value(
            json.get("output_dir")
                .cloned()
                .unwrap_or(serde_json::Value::String(String::new())),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(path)
    }

    /// String representation of the Directories.
    fn __repr__(&self) -> PyResult<String> {
        Ok(format!(
            "Directories(recipe_dir={:?}, work_dir={:?}, host_prefix={:?}, build_prefix={:?}, output_dir={:?})",
            self.recipe_dir()?,
            self.work_dir()?,
            self.host_prefix()?,
            self.build_prefix()?,
            self.output_dir()?
        ))
    }

    /// Detailed string representation showing all paths.
    fn __str__(&self) -> PyResult<String> {
        Ok(format!(
            "Directories:\n  Recipe dir: {:?}\n  Recipe path: {:?}\n  Cache dir: {:?}\n  Work dir: {:?}\n  Host prefix: {:?}\n  Build prefix: {:?}\n  Build dir: {:?}\n  Output dir: {:?}",
            self.recipe_dir()?,
            self.recipe_path()?,
            self.cache_dir()?,
            self.work_dir()?,
            self.host_prefix()?,
            self.build_prefix()?,
            self.build_dir()?,
            self.output_dir()?
        ))
    }
}
