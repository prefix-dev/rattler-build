//! Terminal user interface for rattler-build.

mod event;
mod render;
mod state;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use miette::IntoDiagnostic;
use rand::Rng;
use ratatui::backend::Backend;
use ratatui::prelude::*;
use ratatui::Terminal;
use std::io;
use std::panic;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;

use event::*;
use render::*;
use state::*;

/// Representation of a terminal user interface.
///
/// It is responsible for setting up the terminal,
/// initializing the interface and handling the draw events.
#[derive(Debug)]
struct Tui<B: Backend> {
    /// Interface to the Terminal.
    terminal: Terminal<B>,
    /// Terminal event handler.
    pub events: EventHandler,
}

impl<B: Backend> Tui<B> {
    /// Constructs a new instance of [`Tui`].
    pub fn new(terminal: Terminal<B>, events: EventHandler) -> Self {
        Self { terminal, events }
    }

    /// Initializes the terminal interface.
    ///
    /// It enables the raw mode and sets terminal properties.
    pub fn init(&mut self) -> miette::Result<()> {
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
    pub fn draw(&mut self, state: &mut TuiState) -> miette::Result<()> {
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
    pub fn exit(&mut self) -> miette::Result<()> {
        Self::reset()?;
        self.terminal.show_cursor().into_diagnostic()?;
        Ok(())
    }
}

// TODO: send this to jupiter
async fn generate_dummy_log(sender: mpsc::UnboundedSender<Event>) {
    tokio::spawn(async move {
        let file = tokio::fs::File::open("etc/rattler-build-output.log")
            .await
            .unwrap();
        let reader = tokio::io::BufReader::new(file);
        let mut lines = reader.lines();

        let mut logs = Vec::new();
        let mut line_count = 2;

        while let Some(line) = lines.next_line().await.unwrap() {
            logs.push(line);
            if logs.len() >= line_count {
                sender.send(Event::BuildLog(logs.clone())).unwrap();
                logs.clear();
                line_count = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(2..15)
                };
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }

        sender.send(Event::FinishBuild).unwrap();
    });
}

/// Launches the terminal user interface.
pub async fn run_tui() -> miette::Result<()> {
    // Create an application.
    let mut state = TuiState::new();

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(io::stderr());
    let terminal = Terminal::new(backend).into_diagnostic()?;
    let events = EventHandler::new(250);
    let mut tui = Tui::new(terminal, events);
    tui.init()?;

    // Start the main loop.
    while state.running {
        // Render the user interface.
        tui.draw(&mut state)?;
        // Handle events.
        match tui.events.next().await? {
            Event::Tick => state.tick(),
            Event::Key(key_event) => {
                handle_key_events(key_event, tui.events.sender.clone(), &mut state)?
            }
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            Event::StartBuild(index) => {
                state.packages[index].build_progress = BuildProgress::Building;
                generate_dummy_log(tui.events.sender.clone()).await;
            }
            Event::BuildLog(log) => {
                let building_package = &mut state.packages[state.selected_package];
                building_package.build_progress = BuildProgress::Building;
                building_package.build_log.extend(log);
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
