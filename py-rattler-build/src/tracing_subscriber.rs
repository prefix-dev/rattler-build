use pyo3::prelude::*;
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};

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

/// Install a Python tracing subscriber for the duration of the build
pub fn with_python_tracing<F, R>(callback: Option<Py<PyAny>>, f: F) -> R
where
    F: FnOnce() -> R,
{
    if let Some(py_callback) = callback {
        let callback = PyProgressCallback::new(py_callback);
        let layer = PythonTracingLayer::new(callback);

        // Create a subscriber with the Python layer and filter
        // Only capture info/warn/error, not debug/trace which are too noisy
        use tracing_subscriber::filter::LevelFilter;
        let subscriber = tracing_subscriber::registry()
            .with(layer)
            .with(LevelFilter::INFO);

        // Set this subscriber for the duration of the closure
        tracing::subscriber::with_default(subscriber, f)
    } else {
        // No callback provided, just run the function
        f()
    }
}
