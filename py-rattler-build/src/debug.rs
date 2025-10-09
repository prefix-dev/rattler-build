//! Python bindings for Debug
//!
//! This module provides Python wrappers for the Rust Debug struct,
//! which controls debug output during builds.

use ::rattler_build::metadata::Debug as RustDebug;
use pyo3::prelude::*;

/// Python wrapper for Debug struct.
///
/// Debug is a simple wrapper around a boolean that controls whether
/// debug output is enabled during package builds.
///
/// Examples:
///     Enable debug mode:
///     >>> debug = Debug(True)
///     >>> assert debug.is_enabled()
///
///     Disable debug mode:
///     >>> debug = Debug(False)
///     >>> assert not debug.is_enabled()
///
///     Toggle debug mode:
///     >>> debug = Debug.enabled()
///     >>> debug.set_enabled(False)
#[pyclass(name = "Debug")]
#[derive(Clone, Debug)]
pub struct PyDebug {
    pub(crate) inner: RustDebug,
}

#[pymethods]
impl PyDebug {
    /// Create a new Debug instance.
    ///
    /// Args:
    ///     enabled: Whether debug output is enabled (default: False)
    ///
    /// Returns:
    ///     A new Debug instance
    #[new]
    #[pyo3(signature = (enabled=false))]
    fn new(enabled: bool) -> Self {
        PyDebug {
            inner: RustDebug::new(enabled),
        }
    }

    /// Create a Debug instance with debug enabled.
    ///
    /// Returns:
    ///     Debug instance with debug enabled
    #[staticmethod]
    fn enabled() -> Self {
        PyDebug {
            inner: RustDebug::new(true),
        }
    }

    /// Create a Debug instance with debug disabled.
    ///
    /// Returns:
    ///     Debug instance with debug disabled
    #[staticmethod]
    fn disabled() -> Self {
        PyDebug {
            inner: RustDebug::new(false),
        }
    }

    /// Check if debug output is enabled.
    ///
    /// Returns:
    ///     True if debug output is enabled, False otherwise
    fn is_enabled(&self) -> bool {
        self.inner.is_enabled()
    }

    /// Set whether debug output is enabled.
    ///
    /// Args:
    ///     enabled: Whether to enable debug output
    fn set_enabled(&mut self, enabled: bool) {
        self.inner = RustDebug::new(enabled);
    }

    /// Enable debug output.
    fn enable(&mut self) {
        self.inner = RustDebug::new(true);
    }

    /// Disable debug output.
    fn disable(&mut self) {
        self.inner = RustDebug::new(false);
    }

    /// Toggle debug output.
    fn toggle(&mut self) {
        self.inner = RustDebug::new(!self.inner.is_enabled());
    }

    /// String representation of the Debug instance.
    fn __repr__(&self) -> String {
        format!("Debug(enabled={})", self.inner.is_enabled())
    }

    /// String representation.
    fn __str__(&self) -> &'static str {
        if self.inner.is_enabled() {
            "Debug enabled"
        } else {
            "Debug disabled"
        }
    }

    /// Boolean conversion.
    fn __bool__(&self) -> bool {
        self.inner.is_enabled()
    }
}
