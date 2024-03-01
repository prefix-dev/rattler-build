use std::time::Instant;

/// Representation of a package.
#[derive(Clone, Debug, Default)]
pub(crate) struct Package {
    pub name: String,
    pub build_progress: BuildProgress,
    pub build_log: Vec<String>,
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
    pub fn is_building(&self) -> bool {
        *self == Self::Building
    }
}

/// Application state.
#[derive(Debug)]
pub(crate) struct TuiState {
    /// Is the application running?
    pub running: bool,
    /// Packages to build.
    pub packages: Vec<Package>,
    /// Index of the selected package.
    pub selected_package: usize,
    /// Vertical scroll value.
    pub vertical_scroll: usize,
    /// Last tick value for the spinner.
    pub spinner_last_tick: Instant,
    /// Spinner frame.
    pub spinner_frame: usize,
}

impl Default for TuiState {
    fn default() -> Self {
        let packages = vec![
            Package {
                name: String::from("package 1"),
                build_progress: BuildProgress::None,
                build_log: Vec::new(),
            },
            Package {
                name: String::from("package 2"),
                build_progress: BuildProgress::None,
                build_log: Vec::new(),
            },
            Package {
                name: String::from("package 3"),
                build_progress: BuildProgress::None,
                build_log: Vec::new(),
            },
        ];
        Self {
            running: true,
            packages,
            selected_package: 0,
            vertical_scroll: 0,
            spinner_last_tick: Instant::now(),
            spinner_frame: 0,
        }
    }
}

impl TuiState {
    /// Constructs a new instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {}

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Returns true if a package is building currently.
    pub fn is_building_package(&self) -> bool {
        self.packages.iter().any(|p| p.build_progress.is_building())
    }
}
