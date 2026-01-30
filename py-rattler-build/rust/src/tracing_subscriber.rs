use pyo3::prelude::*;
use std::sync::{Arc, Mutex};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::progress_callback::PyProgressCallback;

/// A tracing layer that forwards log events to a Python progress callback
pub struct PythonTracingLayer {
    callback: PyProgressCallback,
}

impl PythonTracingLayer {
    pub fn new(callback: PyProgressCallback) -> Self {
        Self { callback }
    }
}

impl<S> Layer<S> for PythonTracingLayer
where
    S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = match *metadata.level() {
            Level::ERROR => "error",
            Level::WARN => "warn",
            Level::INFO => "info",
            Level::DEBUG => "debug",
            Level::TRACE => "trace",
        };

        // Extract the message from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Extract span name if available
        let span = _ctx.event_span(event).map(|s| s.name().to_string());

        // Forward to Python callback
        self.callback
            .on_log(level, &visitor.message, span.as_deref());
    }
}

/// Visitor to extract the message from a tracing event
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Remove quotes if present
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}

/// A tracing layer that captures log events into a Vec for later retrieval
pub struct LogCaptureLayer {
    log_buffer: Arc<Mutex<Vec<String>>>,
}

impl LogCaptureLayer {
    pub fn new(log_buffer: Arc<Mutex<Vec<String>>>) -> Self {
        Self { log_buffer }
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // Extract the message from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Store in buffer
        if let Ok(mut buffer) = self.log_buffer.lock() {
            buffer.push(visitor.message);
        }
    }
}

/// Install a log capture subscriber with optional Python callback
/// Returns the captured logs after the function completes
pub fn with_log_capture<F, R>(callback: Option<Py<PyAny>>, f: F) -> (R, Arc<Mutex<Vec<String>>>)
where
    F: FnOnce() -> R,
{
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::layer::SubscriberExt as _;

    // Create log buffer (shared)
    let log_buffer = Arc::new(Mutex::new(Vec::new()));
    let log_buffer_clone = Arc::clone(&log_buffer);

    // Run the function with the subscriber
    let result = if let Some(py_callback) = callback {
        let callback = PyProgressCallback::new(py_callback);
        let python_layer = PythonTracingLayer::new(callback);
        let capture_layer = LogCaptureLayer::new(log_buffer_clone);

        let subscriber = tracing_subscriber::registry()
            .with(capture_layer)
            .with(python_layer)
            .with(LevelFilter::INFO);

        tracing::subscriber::with_default(subscriber, f)
    } else {
        let capture_layer = LogCaptureLayer::new(log_buffer_clone);
        let subscriber = tracing_subscriber::registry()
            .with(capture_layer)
            .with(LevelFilter::INFO);

        tracing::subscriber::with_default(subscriber, f)
    };

    (result, log_buffer)
}
