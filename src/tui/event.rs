//! TUI event handling.

use crossterm::event::{Event as CrosstermEvent, KeyEvent, MouseEvent};
use futures::{FutureExt, StreamExt};
use miette::IntoDiagnostic;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::mpsc;

use super::state::Package;
use crate::BuildOutput;

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
    /// Resolves packages to build.
    ResolvePackages(PathBuf),
    /// Handles the result of resolving packages.
    ProcessResolvedPackages(BuildOutput, Vec<Package>),
    /// Start building.
    StartBuild(usize),
    /// Build log.
    BuildLog(Vec<u8>),
    /// Finish building.
    FinishBuild,
    /// Handle build error.
    HandleBuildError(miette::Error),
    /// Handle console input.
    HandleInput,
    /// Edit recipe.
    EditRecipe,
}

/// Terminal event handler.
#[derive(Debug)]
#[allow(dead_code)]
pub struct EventHandler {
    /// Event sender channel.
    pub sender: mpsc::UnboundedSender<Event>,
    /// Event receiver channel.
    receiver: mpsc::UnboundedReceiver<Event>,
    /// Event handler thread.
    handler: tokio::task::JoinHandle<()>,
    /// Is the key input disabled?
    pub key_input_disabled: Arc<AtomicBool>,
}

impl EventHandler {
    /// Constructs a new instance.
    pub fn new(tick_rate: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate);
        let (sender, receiver) = mpsc::unbounded_channel();
        let _sender = sender.clone();
        let key_input_disabled = Arc::new(AtomicBool::new(false));
        let key_input_disabled_cloned = Arc::clone(&key_input_disabled);
        let handler = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick = tokio::time::interval(tick_rate);
            loop {
                if key_input_disabled_cloned.load(Ordering::Relaxed) {
                    continue;
                }
                let tick_delay = tick.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
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
        Self {
            sender,
            receiver,
            handler,
            key_input_disabled,
        }
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
