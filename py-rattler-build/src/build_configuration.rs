//! Python bindings for BuildConfig
//!
//! This module provides Python wrappers for the Rust BuildConfiguration struct,
//! which contains all configuration for building a package.

use crate::{PyDebug, PyDirectories, PyPackagingConfig, PySandboxConfig};
use ::rattler_build::metadata::BuildConfiguration as RustBuildConfiguration;
use pyo3::prelude::*;
use std::collections::HashMap;

/// Python wrapper for BuildConfig struct.
///
/// BuildConfig contains the complete configuration for building a package,
/// including platforms, variants, channels, directories, and all build settings.
///
/// This is a read-only wrapper that exposes all configuration properties for
/// inspection. Typically created internally during the build process.
///
/// Examples:
///     Access build configuration (from build context):
///     >>> config = get_build_config()
///     >>> print(f"Target: {config.target_platform}")
///     >>> print(f"Hash: {config.hash}")
///     >>> if config.cross_compilation():
///     ...     print("Cross-compiling!")
#[pyclass(name = "BuildConfig")]
#[derive(Clone)]
pub struct PyBuildConfig {
    pub(crate) inner: RustBuildConfiguration,
}

#[pymethods]
impl PyBuildConfig {
    /// Get the target platform.
    ///
    /// The platform for which the package is being built.
    ///
    /// Returns:
    ///     Target platform string (e.g., "linux-64", "osx-arm64")
    #[getter]
    fn target_platform(&self) -> String {
        self.inner.target_platform.to_string()
    }

    /// Get the host platform.
    ///
    /// The platform where the package will run (usually same as target,
    /// but different for noarch packages).
    ///
    /// Returns:
    ///     Dictionary with 'platform' (str) and 'virtual_packages' (list) keys
    #[getter]
    fn host_platform(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let platform_dict = pyo3::types::PyDict::new(py);
        platform_dict.set_item("platform", self.inner.host_platform.platform.to_string())?;

        let virt_packages: Vec<String> = self
            .inner
            .host_platform
            .virtual_packages
            .iter()
            .map(|vp| format!("{}", vp))
            .collect();
        platform_dict.set_item("virtual_packages", virt_packages)?;

        Ok(platform_dict.into())
    }

    /// Get the build platform.
    ///
    /// The platform on which the build is running.
    ///
    /// Returns:
    ///     Dictionary with 'platform' (str) and 'virtual_packages' (list) keys
    #[getter]
    fn build_platform(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let platform_dict = pyo3::types::PyDict::new(py);
        platform_dict.set_item("platform", self.inner.build_platform.platform.to_string())?;

        let virt_packages: Vec<String> = self
            .inner
            .build_platform
            .virtual_packages
            .iter()
            .map(|vp| format!("{}", vp))
            .collect();
        platform_dict.set_item("virtual_packages", virt_packages)?;

        Ok(platform_dict.into())
    }

    /// Get the variant configuration.
    ///
    /// The selected variant for this build (e.g., python version, numpy version).
    ///
    /// Returns:
    ///     Dictionary mapping variant keys to their values
    #[getter]
    fn variant(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut variant_dict = HashMap::new();
        for (key, value) in &self.inner.variant {
            let json_value = serde_json::to_value(value)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            variant_dict.insert(key.normalize(), json_value);
        }

        pythonize::pythonize(py, &variant_dict)
            .map(|obj| obj.into())
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    }

    /// Get the variant hash.
    ///
    /// The computed hash of the variant configuration.
    ///
    /// Returns:
    ///     Hash string (e.g., "h1234567_0")
    #[getter]
    fn hash(&self) -> String {
        self.inner.hash.hash.clone()
    }

    /// Get the build directories.
    ///
    /// Returns:
    ///     Directories instance with all build paths
    #[getter]
    fn directories(&self) -> PyDirectories {
        PyDirectories {
            inner: self.inner.directories.clone(),
        }
    }

    /// Get the channels.
    ///
    /// The channels used for resolving dependencies.
    ///
    /// Returns:
    ///     List of channel URLs as strings
    #[getter]
    fn channels(&self) -> Vec<String> {
        self.inner
            .channels
            .iter()
            .map(|c| c.to_string())
            .collect()
    }

    /// Get the channel priority.
    ///
    /// Returns:
    ///     Channel priority as a string (e.g., "Strict", "Flexible")
    #[getter]
    fn channel_priority(&self) -> String {
        format!("{:?}", self.inner.channel_priority)
    }

    /// Get the solve strategy.
    ///
    /// Returns:
    ///     Solve strategy as a string
    #[getter]
    fn solve_strategy(&self) -> String {
        format!("{:?}", self.inner.solve_strategy)
    }

    /// Get the build timestamp.
    ///
    /// Returns:
    ///     ISO 8601 timestamp string
    #[getter]
    fn timestamp(&self) -> String {
        self.inner.timestamp.to_rfc3339()
    }

    /// Get the subpackages.
    ///
    /// All subpackages from this output or other outputs from the same recipe.
    ///
    /// Returns:
    ///     Dictionary mapping package names to their identifiers
    #[getter]
    fn subpackages(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let subpackages_dict = pyo3::types::PyDict::new(py);
        for (name, identifier) in &self.inner.subpackages {
            let pkg_dict = pyo3::types::PyDict::new(py);
            pkg_dict.set_item("name", name.as_normalized())?;
            pkg_dict.set_item("version", identifier.version.to_string())?;
            pkg_dict.set_item("build_string", identifier.build_string.clone())?;
            subpackages_dict.set_item(name.as_normalized(), pkg_dict)?;
        }

        Ok(subpackages_dict.into())
    }

    /// Get the packaging settings.
    ///
    /// Returns:
    ///     PackagingConfig instance
    #[getter]
    fn packaging_settings(&self) -> PyPackagingConfig {
        PyPackagingConfig {
            inner: self.inner.packaging_settings.clone(),
        }
    }

    /// Check if recipe should be stored in the package.
    ///
    /// Returns:
    ///     True if recipe is stored, False otherwise
    #[getter]
    fn store_recipe(&self) -> bool {
        self.inner.store_recipe
    }

    /// Check if forced colors are enabled.
    ///
    /// Returns:
    ///     True if colors are forced in build script
    #[getter]
    fn force_colors(&self) -> bool {
        self.inner.force_colors
    }

    /// Get the sandbox configuration.
    ///
    /// Returns:
    ///     SandboxConfig instance, or None if not configured
    #[getter]
    fn sandbox_config(&self) -> Option<PySandboxConfig> {
        self.inner
            .sandbox_config
            .as_ref()
            .map(|sc| PySandboxConfig { inner: sc.clone() })
    }

    /// Get the debug configuration.
    ///
    /// Returns:
    ///     Debug instance
    #[getter]
    fn debug(&self) -> PyDebug {
        PyDebug {
            inner: self.inner.debug,
        }
    }

    /// Get the exclude_newer timestamp.
    ///
    /// Packages newer than this date are excluded from the solver.
    ///
    /// Returns:
    ///     ISO 8601 timestamp string, or None if not set
    #[getter]
    fn exclude_newer(&self) -> Option<String> {
        self.inner.exclude_newer.map(|dt| dt.to_rfc3339())
    }

    /// Check if this is a cross-compilation build.
    ///
    /// Returns:
    ///     True if target platform differs from build platform
    fn cross_compilation(&self) -> bool {
        self.inner.cross_compilation()
    }

    /// Get the target platform name only (without virtual packages).
    ///
    /// Returns:
    ///     Platform string
    fn target_platform_name(&self) -> String {
        self.inner.target_platform.to_string()
    }

    /// Get the host platform name only (without virtual packages).
    ///
    /// Returns:
    ///     Platform string
    fn host_platform_name(&self) -> String {
        self.inner.host_platform.platform.to_string()
    }

    /// Get the build platform name only (without virtual packages).
    ///
    /// Returns:
    ///     Platform string
    fn build_platform_name(&self) -> String {
        self.inner.build_platform.platform.to_string()
    }

    /// String representation of the BuildConfig.
    fn __repr__(&self) -> String {
        format!(
            "BuildConfig(target_platform='{}', hash='{}', cross_compilation={})",
            self.inner.target_platform,
            self.inner.hash.hash,
            self.cross_compilation()
        )
    }

    /// Detailed string representation.
    fn __str__(&self) -> String {
        format!(
            "BuildConfig:\n  Target: {}\n  Host: {}\n  Build: {}\n  Hash: {}\n  Cross-compilation: {}\n  Channels: {}\n  Debug: {}",
            self.inner.target_platform,
            self.inner.host_platform.platform,
            self.inner.build_platform.platform,
            self.inner.hash.hash,
            self.cross_compilation(),
            self.inner.channels.len(),
            self.inner.debug.is_enabled()
        )
    }
}
