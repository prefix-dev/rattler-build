//! Terminal user interface for rattler-build.

pub mod event;
pub mod logger;
mod render;
mod state;
mod utils;

use event::*;
use render::*;
use state::*;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use miette::IntoDiagnostic;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::prelude::*;
use std::io::{self, Stderr};
use std::panic;
use std::path::PathBuf;

use crate::build::run_build;
use crate::console_utils::LoggingOutputHandler;
use crate::{BuildData, get_build_output, sort_build_outputs_topologically};

use self::utils::run_editor;

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
    /// Is the interface paused?
    pub paused: bool,
}

impl<B: Backend> Tui<B> {
    /// Constructs a new instance of [`Tui`].
    pub(crate) fn new(terminal: Terminal<B>, event_handler: EventHandler) -> Self {
        Self {
            terminal,
            event_handler,
            paused: false,
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
        self.event_handler.start();
        Ok(())
    }

    /// Draw the terminal interface by rendering the widgets.
    pub(crate) fn draw(&mut self, state: &mut TuiState) -> miette::Result<()> {
        self.terminal
            .draw(|frame| render_widgets(state, frame))
            .into_diagnostic()?;
        Ok(())
    }

    /// Toggles the paused state of interface.
    ///
    /// It disables the key input and exits the
    /// terminal interface on pause (and vice-versa).
    pub fn toggle_pause(&mut self) -> miette::Result<()> {
        self.paused = !self.paused;
        if self.paused {
            Self::reset()?;
            self.event_handler.cancel();
        } else {
            self.init()?;
            self.event_handler.start();
        }
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
        Terminal::new(CrosstermBackend::new(io::stderr()))
            .into_diagnostic()?
            .show_cursor()
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
    build_data: BuildData,
    recipe_paths: Vec<PathBuf>,
    log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    // Create an application.
    let mut state = TuiState::new(build_data, log_handler);

    // Resolve the packages to build.
    tui.event_handler
        .sender
        .send(Event::GetBuildOutputs(recipe_paths))
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
            Event::GetBuildOutputs(recipe_paths) => {
                let state = state.clone();
                let log_sender = tui.event_handler.sender.clone();
                tokio::spawn(async move {
                    let mut outputs = Vec::new();
                    for recipe_path in &recipe_paths {
                        let output =
                            get_build_output(&state.build_data, recipe_path, &state.tool_config)
                                .await
                                .unwrap();
                        outputs.extend(output);
                    }
                    log_sender
                        .send(Event::ProcessBuildOutputs(outputs))
                        .unwrap();
                });
            }
            Event::ProcessBuildOutputs(mut outputs) => {
                sort_build_outputs_topologically(&mut outputs, state.build_data.up_to.as_deref())?;
                let packages: Vec<Package> = outputs
                    .into_iter()
                    .map(|output| Package::from_output(output, &state.tool_config))
                    .collect();
                state.packages.retain(|package| {
                    packages
                        .iter()
                        .any(|p| p.recipe_path != package.recipe_path)
                });
                for new_package in packages {
                    match state
                        .packages
                        .iter_mut()
                        .find(|p| new_package.name == p.name)
                    {
                        Some(package) => {
                            *package = new_package;
                        }
                        None => state.packages.push(new_package),
                    }
                }
            }
            Event::StartBuildQueue => match state.build_queue {
                Some(mut build_index) => {
                    while build_index != state.packages.len() {
                        if state.packages[build_index].build_progress == BuildProgress::Done {
                            build_index += 1;
                            continue;
                        }
                        state.build_queue = Some(build_index);
                        tui.event_handler
                            .sender
                            .send(Event::StartBuild(build_index))
                            .into_diagnostic()?;
                        break;
                    }
                    if build_index == state.packages.len() {
                        state.build_queue = None;
                    }
                }
                None => {
                    state.build_queue = Some(0);
                    tui.event_handler
                        .sender
                        .send(Event::StartBuild(0))
                        .into_diagnostic()?;
                }
            },
            Event::StartBuild(index) => {
                if !state.is_building_package() {
                    let package = state.packages[index].clone();
                    let log_sender = tui.event_handler.sender.clone();
                    let mut packages = Vec::new();
                    for subpackage in package.subpackages.iter() {
                        if let Some(i) = state.packages.iter().position(|v| v.name == *subpackage) {
                            if state.packages[i].build_progress != BuildProgress::Done {
                                packages.push((i, state.packages[i].clone()));
                            }
                        } else {
                            tracing::error!("Cannot find subpackage to build: {subpackage}")
                        }
                    }
                    packages.push((index, package.clone()));
                    tokio::spawn(async move {
                        let mut build_error = None;
                        for (i, package) in packages {
                            log_sender
                                .send(Event::SetBuildState(i, BuildProgress::Building))
                                .unwrap();
                            match run_build(package.output, &package.tool_config).await {
                                Ok((output, _archive)) => {
                                    output.record_build_end();
                                    let span = tracing::info_span!("Build summary");
                                    let _enter = span.enter();
                                    let _ = output.log_build_summary().map_err(|e| {
                                        tracing::error!("Error writing build summary: {}", e);
                                        e
                                    });
                                    log_sender
                                        .send(Event::SetBuildState(i, BuildProgress::Done))
                                        .unwrap();
                                }
                                Err(e) => {
                                    build_error = Some(e);
                                    log_sender
                                        .send(Event::SetBuildState(i, BuildProgress::Failed))
                                        .unwrap();
                                    break;
                                }
                            };
                        }
                        if let Some(e) = build_error {
                            tracing::error!("Error building package: {}", e);
                        } else if state.build_queue.is_some() {
                            log_sender.send(Event::StartBuildQueue).unwrap();
                        }
                    });
                }
            }
            Event::SetBuildState(index, progress) => {
                state.selected_package = index;
                state.packages[index].build_progress = progress;
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
            Event::HandleInput => {
                state.input_mode = false;
                if state.input.value() == "edit" {
                    tui.event_handler
                        .sender
                        .send(Event::EditRecipe)
                        .into_diagnostic()?;
                } else {
                    tracing::error!("Unknown command: {}", state.input.value());
                    tracing::info!("Available commands are: [edit]");
                }
                state.input.reset();
            }
            Event::EditRecipe => {
                let package = state.packages[state.selected_package].clone();
                state.input_mode = false;
                state.input.reset();
                tui.toggle_pause()?;
                run_editor(&package.recipe_path)?;
                tui.event_handler
                    .sender
                    .send(Event::GetBuildOutputs(vec![package.recipe_path]))
                    .into_diagnostic()?;
                tui.toggle_pause()?;
            }
        }
    }

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
