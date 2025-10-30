use pyo3::prelude::*;
use std::sync::Arc;

/// Event types for progress reporting
#[pyclass]
#[derive(Clone)]
pub struct DownloadStartEvent {
    #[pyo3(get)]
    pub url: String,
    #[pyo3(get)]
    pub total_bytes: Option<u64>,
}

#[pymethods]
impl DownloadStartEvent {
    fn __repr__(&self) -> String {
        format!("DownloadStartEvent(url='{}', total_bytes={:?})", self.url, self.total_bytes)
    }
}

#[pyclass]
#[derive(Clone)]
pub struct DownloadProgressEvent {
    #[pyo3(get)]
    pub url: String,
    #[pyo3(get)]
    pub bytes_downloaded: u64,
    #[pyo3(get)]
    pub total_bytes: Option<u64>,
}

#[pymethods]
impl DownloadProgressEvent {
    fn __repr__(&self) -> String {
        format!(
            "DownloadProgressEvent(url='{}', bytes_downloaded={}, total_bytes={:?})",
            self.url, self.bytes_downloaded, self.total_bytes
        )
    }
}

#[pyclass]
#[derive(Clone)]
pub struct DownloadCompleteEvent {
    #[pyo3(get)]
    pub url: String,
}

#[pymethods]
impl DownloadCompleteEvent {
    fn __repr__(&self) -> String {
        format!("DownloadCompleteEvent(url='{}')", self.url)
    }
}

#[pyclass]
#[derive(Clone)]
pub struct BuildStepEvent {
    #[pyo3(get)]
    pub step_name: String,
    #[pyo3(get)]
    pub message: String,
}

#[pymethods]
impl BuildStepEvent {
    fn __repr__(&self) -> String {
        format!("BuildStepEvent(step_name='{}', message='{}')", self.step_name, self.message)
    }
}

#[pyclass]
#[derive(Clone)]
pub struct LogEvent {
    #[pyo3(get)]
    pub level: String,
    #[pyo3(get)]
    pub message: String,
    #[pyo3(get)]
    pub span: Option<String>,
}

#[pymethods]
impl LogEvent {
    fn __repr__(&self) -> String {
        format!(
            "LogEvent(level='{}', message='{}', span={:?})",
            self.level, self.message, self.span
        )
    }
}

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

    /// Get the inner Arc for sharing across threads
    pub fn inner(&self) -> Arc<Py<PyAny>> {
        Arc::clone(&self.callback)
    }

    /// Get a clone of this callback wrapper
    pub fn clone_callback(&self) -> Arc<Self> {
        Arc::new(self.clone())
    }

    /// Call the on_download_start callback
    pub fn on_download_start(&self, url: &str, total_bytes: Option<u64>) {
        if let Err(e) = Python::attach(|py| {
            let event = DownloadStartEvent {
                url: url.to_string(),
                total_bytes,
            };
            let event_obj = Py::new(py, event)?;

            self.callback
                .bind(py)
                .call_method1("on_download_start", (event_obj,))?;
            Ok::<(), PyErr>(())
        }) {
            // Log error but don't fail the build
            eprintln!("Error in Python progress callback on_download_start: {}", e);
        }
    }

    /// Call the on_download_progress callback
    pub fn on_download_progress(&self, url: &str, bytes_downloaded: u64, total_bytes: Option<u64>) {
        if let Err(e) = Python::attach(|py| {
            let event = DownloadProgressEvent {
                url: url.to_string(),
                bytes_downloaded,
                total_bytes,
            };
            let event_obj = Py::new(py, event)?;

            self.callback
                .bind(py)
                .call_method1("on_download_progress", (event_obj,))?;
            Ok::<(), PyErr>(())
        }) {
            eprintln!("Error in Python progress callback on_download_progress: {}", e);
        }
    }

    /// Call the on_download_complete callback
    pub fn on_download_complete(&self, url: &str) {
        if let Err(e) = Python::attach(|py| {
            let event = DownloadCompleteEvent {
                url: url.to_string(),
            };
            let event_obj = Py::new(py, event)?;

            self.callback
                .bind(py)
                .call_method1("on_download_complete", (event_obj,))?;
            Ok::<(), PyErr>(())
        }) {
            eprintln!("Error in Python progress callback on_download_complete: {}", e);
        }
    }

    /// Call the on_build_step callback
    pub fn on_build_step(&self, step_name: &str, message: &str) {
        if let Err(e) = Python::attach(|py| {
            let event = BuildStepEvent {
                step_name: step_name.to_string(),
                message: message.to_string(),
            };
            let event_obj = Py::new(py, event)?;

            self.callback
                .bind(py)
                .call_method1("on_build_step", (event_obj,))?;
            Ok::<(), PyErr>(())
        }) {
            eprintln!("Error in Python progress callback on_build_step: {}", e);
        }
    }

    /// Call the on_log callback
    pub fn on_log(&self, level: &str, message: &str, span: Option<&str>) {
        if let Err(e) = Python::attach(|py| {
            let event = LogEvent {
                level: level.to_string(),
                message: message.to_string(),
                span: span.map(|s| s.to_string()),
            };
            let event_obj = Py::new(py, event)?;

            self.callback
                .bind(py)
                .call_method1("on_log", (event_obj,))?;
            Ok::<(), PyErr>(())
        }) {
            eprintln!("Error in Python progress callback on_log: {}", e);
        }
    }
}

/// Register progress callback types with Python
pub fn register_progress_types(py: Python<'_>, parent: &Bound<'_, PyModule>) -> PyResult<()> {
    let m = PyModule::new(py, "progress")?;
    m.add_class::<DownloadStartEvent>()?;
    m.add_class::<DownloadProgressEvent>()?;
    m.add_class::<DownloadCompleteEvent>()?;
    m.add_class::<BuildStepEvent>()?;
    m.add_class::<LogEvent>()?;
    parent.add_submodule(&m)?;
    Ok(())
}
