//! Test helpers shared by the runner modules.

use super::{OutputSink, OutputStream};

/// An [`OutputSink`] that records every line together with its stream.
#[derive(Default)]
pub(super) struct VecSink(pub(super) Vec<(OutputStream, String)>);

impl OutputSink for VecSink {
    fn line(&mut self, stream: OutputStream, line: &str) {
        self.0.push((stream, line.to_string()));
    }
}
