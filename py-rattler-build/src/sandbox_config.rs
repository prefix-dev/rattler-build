//! Python bindings for SandboxConfig
//!
//! This module provides Python wrappers for the Rust SandboxConfiguration struct,
//! which controls build sandboxing and isolation settings.

use crate::error::RattlerBuildError;
use ::rattler_build::script::SandboxConfiguration as RustSandboxConfiguration;
use pyo3::prelude::*;
use std::path::PathBuf;

/// Python wrapper for SandboxConfig struct.
///
/// SandboxConfig controls the sandboxing/isolation settings for builds,
/// including network access and filesystem permissions.
///
/// Examples:
///     Create a basic sandbox configuration:
///     >>> config = SandboxConfig(
///     ...     allow_network=False,
///     ...     read=["/usr", "/etc"],
///     ...     read_execute=["/bin", "/usr/bin"],
///     ...     read_write=["/tmp"]
///     ... )
///
///     Use platform defaults:
///     >>> macos_config = SandboxConfig.for_macos()
///     >>> linux_config = SandboxConfig.for_linux()
#[pyclass(name = "SandboxConfig")]
#[derive(Clone, Debug)]
pub struct PySandboxConfig {
    pub(crate) inner: RustSandboxConfiguration,
}

#[pymethods]
impl PySandboxConfig {
    /// Create a new SandboxConfig.
    ///
    /// Args:
    ///     allow_network: Whether to allow network access during the build
    ///     read: List of paths that can be read
    ///     read_execute: List of paths that can be read and executed
    ///     read_write: List of paths that can be read and written
    ///
    /// Returns:
    ///     A new SandboxConfiguration instance
    #[new]
    #[pyo3(signature = (allow_network=false, read=None, read_execute=None, read_write=None))]
    fn new(
        allow_network: bool,
        read: Option<Vec<PathBuf>>,
        read_execute: Option<Vec<PathBuf>>,
        read_write: Option<Vec<PathBuf>>,
    ) -> Self {
        // Since RustSandboxConfiguration fields are private, we use serde
        let config = serde_json::json!({
            "allow_network": allow_network,
            "read": read.unwrap_or_default(),
            "read_execute": read_execute.unwrap_or_default(),
            "read_write": read_write.unwrap_or_default(),
        });

        let inner: RustSandboxConfiguration =
            serde_json::from_value(config).expect("Failed to create SandboxConfiguration");

        PySandboxConfig { inner }
    }

    /// Get the allow_network setting.
    ///
    /// Returns:
    ///     True if network access is allowed, False otherwise
    #[getter]
    fn allow_network(&self) -> PyResult<bool> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        Ok(json
            .get("allow_network")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    /// Set the allow_network setting.
    ///
    /// Args:
    ///     value: Whether to allow network access
    #[setter]
    fn set_allow_network(&mut self, value: bool) -> PyResult<()> {
        let mut json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        json["allow_network"] = serde_json::Value::Bool(value);
        self.inner = serde_json::from_value(json)
            .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(())
    }

    /// Get the list of read-only paths.
    ///
    /// Returns:
    ///     List of paths that can be read
    #[getter]
    fn read(&self) -> PyResult<Vec<PathBuf>> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let paths: Vec<PathBuf> = serde_json::from_value(
            json.get("read")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(paths)
    }

    /// Set the list of read-only paths.
    ///
    /// Args:
    ///     value: List of paths that can be read
    #[setter]
    fn set_read(&mut self, value: Vec<PathBuf>) -> PyResult<()> {
        let mut json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        json["read"] = serde_json::to_value(&value)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        self.inner = serde_json::from_value(json)
            .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(())
    }

    /// Get the list of read-execute paths.
    ///
    /// Returns:
    ///     List of paths that can be read and executed
    #[getter]
    fn read_execute(&self) -> PyResult<Vec<PathBuf>> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let paths: Vec<PathBuf> = serde_json::from_value(
            json.get("read_execute")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(paths)
    }

    /// Set the list of read-execute paths.
    ///
    /// Args:
    ///     value: List of paths that can be read and executed
    #[setter]
    fn set_read_execute(&mut self, value: Vec<PathBuf>) -> PyResult<()> {
        let mut json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        json["read_execute"] = serde_json::to_value(&value)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        self.inner = serde_json::from_value(json)
            .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(())
    }

    /// Get the list of read-write paths.
    ///
    /// Returns:
    ///     List of paths that can be read and written
    #[getter]
    fn read_write(&self) -> PyResult<Vec<PathBuf>> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        let paths: Vec<PathBuf> = serde_json::from_value(
            json.get("read_write")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(paths)
    }

    /// Set the list of read-write paths.
    ///
    /// Args:
    ///     value: List of paths that can be read and written
    #[setter]
    fn set_read_write(&mut self, value: Vec<PathBuf>) -> PyResult<()> {
        let mut json = serde_json::to_value(&self.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        json["read_write"] = serde_json::to_value(&value)
            .map_err(|e| RattlerBuildError::Other(format!("Serialization failed: {}", e)))?;
        self.inner = serde_json::from_value(json)
            .map_err(|e| RattlerBuildError::Other(format!("Deserialization failed: {}", e)))?;
        Ok(())
    }

    /// Create a default sandbox configuration for macOS.
    ///
    /// This configuration includes:
    /// - Network access: disabled
    /// - Read access: entire filesystem
    /// - Read-execute: /bin, /usr/bin
    /// - Read-write: /tmp, /var/tmp, $TMPDIR
    ///
    /// Returns:
    ///     A SandboxConfiguration configured for macOS
    #[staticmethod]
    fn for_macos() -> Self {
        PySandboxConfig {
            inner: RustSandboxConfiguration::for_macos(),
        }
    }

    /// Create a default sandbox configuration for Linux.
    ///
    /// This configuration includes:
    /// - Network access: disabled
    /// - Read access: entire filesystem
    /// - Read-execute: /bin, /usr/bin, /lib*, /usr/lib*
    /// - Read-write: /tmp, /dev/shm, $TMPDIR
    ///
    /// Returns:
    ///     A SandboxConfiguration configured for Linux
    #[staticmethod]
    fn for_linux() -> Self {
        PySandboxConfig {
            inner: RustSandboxConfiguration::for_linux(),
        }
    }

    /// Add a path to the read-only list.
    ///
    /// Args:
    ///     path: Path to add to the read-only list
    fn add_read(&mut self, path: PathBuf) -> PyResult<()> {
        let mut paths = self.read()?;
        paths.push(path);
        self.set_read(paths)
    }

    /// Add a path to the read-execute list.
    ///
    /// Args:
    ///     path: Path to add to the read-execute list
    fn add_read_execute(&mut self, path: PathBuf) -> PyResult<()> {
        let mut paths = self.read_execute()?;
        paths.push(path);
        self.set_read_execute(paths)
    }

    /// Add a path to the read-write list.
    ///
    /// Args:
    ///     path: Path to add to the read-write list
    fn add_read_write(&mut self, path: PathBuf) -> PyResult<()> {
        let mut paths = self.read_write()?;
        paths.push(path);
        self.set_read_write(paths)
    }

    /// String representation of the SandboxConfiguration.
    fn __repr__(&self) -> PyResult<String> {
        Ok(format!(
            "SandboxConfiguration(allow_network={}, read={} paths, read_execute={} paths, read_write={} paths)",
            self.allow_network()?,
            self.read()?.len(),
            self.read_execute()?.len(),
            self.read_write()?.len()
        ))
    }

    /// Detailed string representation.
    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }
}
