use pyo3::prelude::*;
use std::sync::Arc;

/// Python progress callback bridge
///
/// This struct wraps a Python callback object and provides thread-safe
/// methods to invoke it from Rust code during the build process.
#[derive(Clone)]
pub struct PyProgressCallback {
    callback: Arc<Py<PyAny>>,
}

impl PyProgressCallback {
    /// Create a new Python progress callback wrapper
    pub fn new(callback: Py<PyAny>) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Call the on_log callback
    pub fn on_log(&self, level: &str, message: &str, span: Option<&str>) {
        if let Err(e) = Python::attach(|py| {
            // Import the LogEvent class from Python
            let progress_module = py.import("rattler_build.progress")?;
            let log_event_class = progress_module.getattr("LogEvent")?;

            // Create a LogEvent instance
            let event = log_event_class.call1((level, message, span))?;

            // Call the on_log method
            self.callback.bind(py).call_method1("on_log", (event,))?;
            Ok::<(), PyErr>(())
        }) {
            // Log error but don't fail the build
            eprintln!("Error in Python progress callback on_log: {}", e);
        }
    }
}
