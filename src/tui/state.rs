use std::path::PathBuf;

use ratatui::{layout::Rect, style::Color};
use throbber_widgets_tui::ThrobberState;
use tui_input::Input;

use crate::{
    console_utils::LoggingOutputHandler, get_build_output, metadata::Output, opt::BuildOpts,
    BuildOutput,
};

/// Representation of a package.
#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub build_string: Option<String>,
    pub subpackages: Vec<String>,
    pub build_progress: BuildProgress,
    pub build_log: Vec<String>,
    pub spinner_state: ThrobberState,
    pub area: Rect,
    pub is_hovered: bool,
    pub output: Output,
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
    /// Horizontal scroll value.
    pub horizontal_scroll: u16,
    /// Application log.
    pub log: Vec<String>,
    /// Is the input mode enabled?
    pub input_mode: bool,
    /// Current value of the prompt input.
    pub input: Input,
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
            horizontal_scroll: 0,
            log: Vec::new(),
            input_mode: false,
            input: Input::default(),
        }
    }

    /// Resolves and returns the packages to build.
    pub async fn resolve_packages(
        &self,
        recipe_path: PathBuf,
    ) -> miette::Result<(BuildOutput, Vec<Package>)> {
        let build_output = get_build_output(
            self.build_opts.clone(),
            recipe_path,
            self.log_handler.clone(),
        )
        .await?;
        let packages = build_output
            .outputs
            .iter()
            .map(|output| {
                let name = output.name().as_normalized().to_string();
                Package {
                    name: name.clone(),
                    version: output.version().to_string(),
                    build_string: output.build_string().map(String::from),
                    subpackages: output
                        .build_configuration
                        .subpackages
                        .keys()
                        .map(|v| v.as_normalized().to_string())
                        .filter(|v| v != &name)
                        .collect(),
                    build_progress: BuildProgress::None,
                    build_log: Vec::new(),
                    spinner_state: ThrobberState::default(),
                    area: Rect::default(),
                    is_hovered: false,
                    output: output.clone(),
                }
            })
            .collect();
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
