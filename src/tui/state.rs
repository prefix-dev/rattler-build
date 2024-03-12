use ratatui::{layout::Rect, style::Color};
use throbber_widgets_tui::ThrobberState;

use crate::{console_utils::LoggingOutputHandler, get_build_output, opt::BuildOpts, BuildOutput};

/// Representation of a package.
#[derive(Clone, Debug, Default)]
pub struct Package {
    pub name: String,
    pub build_progress: BuildProgress,
    pub build_log: Vec<String>,
    pub spinner_state: ThrobberState,
    pub area: Rect,
    pub is_hovered: bool,
}

/// Build progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum BuildProgress {
    #[default]
    None,
    Building,
    Failed,
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
            BuildProgress::None => Color::Rgb(100, 100, 100),
            BuildProgress::Building => Color::Yellow,
            BuildProgress::Failed => Color::Red,
            BuildProgress::Done => Color::Green,
        }
    }
}

/// Application state.
#[derive(Clone)]
pub(crate) struct TuiState {
    /// Build output.
    pub build_output: Option<BuildOutput>,
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
    /// Application log.
    pub log: Vec<String>,
}

impl TuiState {
    /// Constructs a new instance.
    pub fn new(build_opts: BuildOpts, log_handler: LoggingOutputHandler) -> Self {
        Self {
            build_output: None,
            build_opts: build_opts.clone(),
            log_handler,
            running: true,
            packages: Vec::new(),
            selected_package: 0,
            vertical_scroll: 0,
            log: Vec::new(),
        }
    }

    /// Resolves and returns the packages to build.
    pub async fn resolve_packages(&self) -> miette::Result<(BuildOutput, Vec<Package>)> {
        let build_output =
            get_build_output(self.build_opts.clone(), self.log_handler.clone()).await?;
        let packages = vec![Package {
            name: build_output
                .outputs
                .iter()
                .map(|output| output.name().as_normalized().to_string())
                .collect::<Vec<String>>()
                .join(", "),
            build_progress: BuildProgress::None,
            build_log: Vec::new(),
            spinner_state: ThrobberState::default(),
            area: Rect::default(),
            is_hovered: false,
        }];
        Ok((build_output, packages))
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
