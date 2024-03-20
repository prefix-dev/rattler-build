//! TUI log handler.

use super::event::Event;
use std::io;
use tokio::sync::mpsc;
use tracing_subscriber::fmt::MakeWriter;

/// Writer for TUI logs.
#[derive(Debug)]
pub struct TuiOutputHandler {
    /// Sender channel for logs.
    pub log_sender: mpsc::UnboundedSender<Event>,
}

impl Clone for TuiOutputHandler {
    fn clone(&self) -> Self {
        Self {
            log_sender: self.log_sender.clone(),
        }
    }
}

impl io::Write for TuiOutputHandler {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.log_sender
            .send(Event::BuildLog(buf.to_vec()))
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("could not send TUI event: {e}"),
                )
            })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TuiOutputHandler {
    type Writer = TuiOutputHandler;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
