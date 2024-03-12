//! Terminal user interface for rattler-build.

pub mod event;
pub mod logger;
mod render;
mod state;

use event::*;
use render::*;
use state::*;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use miette::IntoDiagnostic;
use ratatui::backend::Backend;
use ratatui::prelude::*;
use ratatui::Terminal;
use std::io::{self, Stderr};
use std::panic;

use crate::console_utils::LoggingOutputHandler;
use crate::opt::BuildOpts;
use crate::run_build_from_args;

/// Representation of a terminal user interface.
///
/// It is responsible for setting up the terminal,
/// initializing the interface and handling the draw events.
#[derive(Debug)]
pub struct Tui<B: Backend> {
    /// Interface to the Terminal.
    terminal: Terminal<B>,
    /// Terminal event handler.
    pub event_handler: EventHandler,
}

impl<B: Backend> Tui<B> {
    /// Constructs a new instance of [`Tui`].
    pub(crate) fn new(terminal: Terminal<B>, event_handler: EventHandler) -> Self {
        Self {
            terminal,
            event_handler,
        }
    }

    /// Initializes the terminal interface.
    ///
    /// It enables the raw mode and sets terminal properties.
    pub(crate) fn init(&mut self) -> miette::Result<()> {
        terminal::enable_raw_mode().into_diagnostic()?;
        crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)
            .into_diagnostic()?;

        // Define a custom panic hook to reset the terminal properties.
        // This way, you won't have your terminal messed up if an unexpected error happens.
        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic| {
            Self::reset().expect("failed to reset the terminal");
            panic_hook(panic);
        }));

        self.terminal.hide_cursor().into_diagnostic()?;
        self.terminal.clear().into_diagnostic()?;
        Ok(())
    }

    /// Draw the terminal interface by rendering the widgets.
    pub(crate) fn draw(&mut self, state: &mut TuiState) -> miette::Result<()> {
        self.terminal
            .draw(|frame| render_widgets(state, frame))
            .into_diagnostic()?;
        Ok(())
    }

    /// Resets the terminal interface.
    ///
    /// This function is also used for the panic hook to revert
    /// the terminal properties if unexpected errors occur.
    fn reset() -> miette::Result<()> {
        terminal::disable_raw_mode().into_diagnostic()?;
        crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture)
            .into_diagnostic()?;
        Ok(())
    }

    /// Exits the terminal interface.
    ///
    /// It disables the raw mode and reverts back the terminal properties.
    pub(crate) fn exit(&mut self) -> miette::Result<()> {
        Self::reset()?;
        self.terminal.show_cursor().into_diagnostic()?;
        Ok(())
    }
}

/// Initializes the TUI.
pub async fn init() -> miette::Result<Tui<CrosstermBackend<Stderr>>> {
    let backend = CrosstermBackend::new(io::stderr());
    let terminal = Terminal::new(backend).into_diagnostic()?;
    let events = EventHandler::new(250);
    let mut tui = Tui::new(terminal, events);
    tui.init()?;
    Ok(tui)
}

/// Launches the terminal user interface.
pub async fn run<B: Backend>(
    mut tui: Tui<B>,
    opts: BuildOpts,
    log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    // Create an application.
    let mut state = TuiState::new(opts, log_handler);

    // Resolve the packages to build.
    tui.event_handler
        .sender
        .send(Event::ResolvePackages)
        .into_diagnostic()?;

    // Start the main loop.
    while state.running {
        // Render the user interface.
        tui.draw(&mut state)?;
        // Handle events.
        match tui.event_handler.next().await? {
            Event::Tick => state.tick(),
            Event::Key(key_event) => {
                handle_key_events(key_event, tui.event_handler.sender.clone(), &mut state)?
            }
            Event::Mouse(mouse_event) => {
                handle_mouse_events(mouse_event, tui.event_handler.sender.clone(), &mut state)?
            }
            Event::Resize(_, _) => {}
            Event::ResolvePackages => {
                let log_sender = tui.event_handler.sender.clone();
                let state = state.clone();
                tokio::spawn(async move {
                    let resolved = state.resolve_packages().await.unwrap();
                    log_sender
                        .send(Event::ProcessResolvedPackages(resolved.0, resolved.1))
                        .unwrap();
                });
            }
            Event::ProcessResolvedPackages(build_output, packages) => {
                state.build_output = Some(build_output);
                state.packages = packages.clone();
            }
            Event::StartBuild(index) => {
                if !state.is_building_package() {
                    state.selected_package = index;
                    state.packages[index].build_progress = BuildProgress::Building;
                    let build_output = state.build_output.clone().unwrap();
                    let log_sender = tui.event_handler.sender.clone();
                    tokio::spawn(async move {
                        run_build_from_args(build_output).await.unwrap();
                        log_sender.send(Event::FinishBuild).unwrap();
                    });
                } else {
                    // TODO: show a popup message
                }
            }
            Event::BuildLog(log) => {
                if let Some(building_package) = state
                    .packages
                    .iter_mut()
                    .find(|p| p.build_progress.is_building())
                {
                    building_package
                        .build_log
                        .push(String::from_utf8_lossy(&log).to_string());
                } else {
                    state.log.push(String::from_utf8_lossy(&log).to_string());
                }
            }
            Event::FinishBuild => {
                state.packages[state.selected_package].build_progress = BuildProgress::Done;
            }
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
