//! Python bindings for TestConfig
//!
//! This module provides Python wrappers for the Rust TestConfiguration struct,
//! which controls package testing settings.

use crate::PyDebug;
use ::rattler_build::package_test::TestConfiguration as RustTestConfiguration;
use pyo3::prelude::*;
use std::path::PathBuf;

/// Python wrapper for TestConfig struct.
///
/// TestConfig controls the settings for testing conda packages.
/// This is a read-only wrapper that exposes configuration properties for
/// inspection. Typically created internally during test runs.
///
/// Examples:
///     Access test configuration (from test context):
///     >>> config = get_test_config()  # From test context
///     >>> print(f"Test prefix: {config.test_prefix}")
///     >>> print(f"Keep prefix: {config.keep_test_prefix}")
///     >>> print(f"Target platform: {config.target_platform}")
#[pyclass(name = "TestConfig")]
#[derive(Clone)]
pub struct PyTestConfig {
    pub(crate) inner: RustTestConfiguration,
}

#[pymethods]
impl PyTestConfig {
    /// Get the test prefix directory path.
    ///
    /// The directory where the test environment is created.
    ///
    /// Returns:
    ///     Path to the test prefix directory
    #[getter]
    fn test_prefix(&self) -> PathBuf {
        self.inner.test_prefix.clone()
    }

    /// Get the target platform.
    ///
    /// The platform for which the package was built.
    ///
    /// Returns:
    ///     Target platform string, or None if not set
    #[getter]
    fn target_platform(&self) -> Option<String> {
        self.inner.target_platform.map(|p| p.to_string())
    }

    /// Get the host platform.
    ///
    /// The platform for runtime dependencies.
    ///
    /// Returns:
    ///     Host platform string, or None if not set
    #[getter]
    fn host_platform(&self) -> Option<String> {
        self.inner.host_platform.as_ref().map(|p| p.platform.to_string())
    }

    /// Get the current platform.
    ///
    /// The platform running the tests.
    ///
    /// Returns:
    ///     Current platform string
    #[getter]
    fn current_platform(&self) -> String {
        self.inner.current_platform.platform.to_string()
    }

    /// Check if test prefix should be kept after test.
    ///
    /// Returns:
        ///     True if test prefix is kept, False if it's deleted
    #[getter]
    fn keep_test_prefix(&self) -> bool {
        self.inner.keep_test_prefix
    }

    /// Get the test index to execute.
    ///
    /// If set, only this specific test will be run.
    ///
    /// Returns:
    ///     Test index, or None to run all tests
    #[getter]
    fn test_index(&self) -> Option<usize> {
        self.inner.test_index
    }

    /// Get the channels used for testing.
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
    ///     Channel priority as a string
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

    /// Get the output directory.
    ///
    /// The directory where test artifacts are created.
    ///
    /// Returns:
    ///     Path to the output directory
    #[getter]
    fn output_dir(&self) -> PathBuf {
        self.inner.output_dir.clone()
    }

    /// Get the debug configuration.
    ///
    /// Returns:
    ///     Debug instance indicating if debug mode is enabled
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

    /// String representation of the TestConfig.
    fn __repr__(&self) -> String {
        format!(
            "TestConfig(test_prefix={:?}, target_platform={:?}, keep_test_prefix={})",
            self.inner.test_prefix,
            self.inner.target_platform.map(|p| p.to_string()),
            self.inner.keep_test_prefix
        )
    }

    /// Detailed string representation.
    fn __str__(&self) -> String {
        format!(
            "TestConfig:\n  Test prefix: {:?}\n  Target platform: {:?}\n  Host platform: {:?}\n  Keep prefix: {}\n  Test index: {:?}\n  Output dir: {:?}\n  Debug: {}",
            self.inner.test_prefix,
            self.inner.target_platform.map(|p| p.to_string()),
            self.inner.host_platform.as_ref().map(|p| p.platform.to_string()),
            self.inner.keep_test_prefix,
            self.inner.test_index,
            self.inner.output_dir,
            self.inner.debug.is_enabled()
        )
    }
}
