//! TUI event handling.

use crate::metadata::Output;

use super::state::BuildProgress;
use crossterm::event::{Event as CrosstermEvent, KeyEvent, MouseEvent};
use futures::{FutureExt, StreamExt};
use miette::IntoDiagnostic;
use std::{path::PathBuf, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Terminal events.
pub enum Event {
    /// Terminal tick.
    Tick,
    /// Key press.
    Key(KeyEvent),
    /// Mouse click/scroll.
    Mouse(MouseEvent),
    /// Terminal resize.
    Resize(u16, u16),
    /// Get build outputs.
    GetBuildOutputs(Vec<PathBuf>),
    /// Processes the build outputs.
    ProcessBuildOutputs(Vec<Output>),
    /// Start building.
    StartBuild(usize),
    /// Build all packages.
    StartBuildQueue,
    /// Set build state.
    SetBuildState(usize, BuildProgress),
    /// Build log.
    BuildLog(Vec<u8>),
    /// Handle console input.
    HandleInput,
    /// Edit recipe.
    EditRecipe,
}

/// Terminal event handler.
#[derive(Debug)]
#[allow(dead_code)]
pub struct EventHandler {
    /// Tick rate.
    tick_rate: Duration,
    /// Event sender channel.
    pub sender: mpsc::UnboundedSender<Event>,
    /// Event receiver channel.
    receiver: mpsc::UnboundedReceiver<Event>,
    /// Event handler thread.
    handler: tokio::task::JoinHandle<()>,
    /// Token for cancelling the event loop.
    cancellation_token: CancellationToken,
}

impl EventHandler {
    /// Constructs a new instance.
    pub fn new(tick_rate: u64) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            tick_rate: Duration::from_millis(tick_rate),
            sender,
            receiver,
            handler: tokio::spawn(async {}),
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Starts the event loop.
    pub fn start(&mut self) {
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let _cancellation_token = self.cancellation_token.clone();
        let _sender = self.sender.clone();
        let _tick_rate = self.tick_rate;
        self.handler = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick = tokio::time::interval(_tick_rate);
            loop {
                let tick_delay = tick.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                  _ = _cancellation_token.cancelled() => {
                    break;
                  }
                  _ = tick_delay => {
                    _sender.send(Event::Tick).unwrap();
                  }
                  Some(Ok(evt)) = crossterm_event => {
                    match evt {
                      CrosstermEvent::Key(key) => {
                        if key.kind == crossterm::event::KeyEventKind::Press {
                          _sender.send(Event::Key(key)).unwrap();
                        }
                      },
                      CrosstermEvent::Mouse(mouse) => {
                        _sender.send(Event::Mouse(mouse)).unwrap();
                      },
                      CrosstermEvent::Resize(x, y) => {
                        _sender.send(Event::Resize(x, y)).unwrap();
                      },
                      CrosstermEvent::FocusLost => {
                      },
                      CrosstermEvent::FocusGained => {
                      },
                      CrosstermEvent::Paste(_) => {
                      },
                    }
                  }
                };
            }
        });
    }

    /// Cancels the event loop.
    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    /// Receive the next event from the handler thread.
    ///
    /// This function will always block the current thread if
    /// there is no data available and it's possible for more data to be sent.
    pub async fn next(&mut self) -> miette::Result<Event> {
        self.receiver
            .recv()
            .await
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::Other,
                "IO error occurred",
            ))
            .into_diagnostic()
    }
}
