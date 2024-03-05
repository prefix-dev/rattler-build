use ratatui::style::Color;
use throbber_widgets_tui::ThrobberState;

use crate::{console_utils::LoggingOutputHandler, opt::BuildOpts};

/// Representation of a package.
#[derive(Clone, Debug, Default)]
pub(crate) struct Package {
    pub name: String,
    pub build_progress: BuildProgress,
    pub build_log: Vec<String>,
    pub spinner_state: ThrobberState,
}

/// Build progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum BuildProgress {
    #[default]
    None,
    Building,
    Done,
}

impl BuildProgress {
    /// Returns true if the package is building.
    pub fn is_building(&self) -> bool {
        *self == Self::Building
    }

    /// Returns the corresponding color for the progress.
    pub fn as_color(&self) -> Color {
        match self {
            BuildProgress::None => Color::Red,
            BuildProgress::Building => Color::Yellow,
            BuildProgress::Done => Color::Green,
        }
    }
}

/// Application state.
pub(crate) struct TuiState {
    /// Build options.
    pub build_opts: BuildOpts,
    /// Log handler.
    pub log_handler: LoggingOutputHandler,
    /// Is the application running?
    pub running: bool,
    /// Packages to build.
    pub packages: Vec<Package>,
    /// Index of the selected package.
    pub selected_package: usize,
    /// Vertical scroll value.
    pub vertical_scroll: u16,
}

impl TuiState {
    /// Constructs a new instance.
    pub fn new(build_opts: BuildOpts, log_handler: LoggingOutputHandler) -> Self {
        Self {
            build_opts: build_opts.clone(),
            log_handler,
            running: true,
            packages: vec![Package {
                name: build_opts.recipe.to_string_lossy().to_string(),
                build_progress: BuildProgress::None,
                build_log: Vec::new(),
                spinner_state: ThrobberState::default(),
            }],
            selected_package: 0,
            vertical_scroll: 0,
        }
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&mut self) {
        self.packages.iter_mut().for_each(|package| {
            if package.build_progress.is_building() {
                package.spinner_state.calc_next();
            }
        })
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Returns true if a package is building currently.
    pub fn is_building_package(&self) -> bool {
        self.packages.iter().any(|p| p.build_progress.is_building())
    }
}
