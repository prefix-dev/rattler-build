use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use rattler::install::Placement;
use rattler_repodata_gateway::{DownloadReporter, JLAPReporter, Reporter};
use url::Url;

/// Reporter used for tracking download progress via `MultiProgress`.
pub struct GatewayReporter {
    progress_bars: Arc<Mutex<Vec<ProgressBar>>>,
    multi_progress: MultiProgress,
    progress_template: Option<ProgressStyle>,
    finish_template: Option<ProgressStyle>,
    prefix: String,
    finish_message: Option<String>,
    steady_tick: Option<Duration>,
    placement: Placement,
}

/// Builder for [`GatewayReporter`].
#[derive(Default)]
pub struct GatewayReporterBuilder {
    multi_progress: Option<MultiProgress>,
    progress_template: Option<ProgressStyle>,
    finish_template: Option<ProgressStyle>,
    prefix: Option<String>,
    finish_message: Option<String>,
    steady_tick: Option<Duration>,
    placement: Option<Placement>,
}

impl GatewayReporter {
    /// Construct a new builder.
    pub fn builder() -> GatewayReporterBuilder {
        GatewayReporterBuilder::default()
    }

    fn place_progress_bar(&self, progress_bar: ProgressBar) -> ProgressBar {
        match &self.placement {
            Placement::Before(other) => self.multi_progress.insert_before(other, progress_bar),
            Placement::After(other) => self.multi_progress.insert_after(other, progress_bar),
            Placement::Index(index) => self.multi_progress.insert(*index, progress_bar),
            Placement::End => self.multi_progress.add(progress_bar),
        }
    }
}

impl DownloadReporter for GatewayReporter {
    fn on_download_start(&self, _url: &Url) -> usize {
        let progress_bar = ProgressBar::new(1)
            .with_finish(ProgressFinish::AndLeave)
            .with_prefix(self.prefix.clone());

        if let Some(template) = &self.progress_template {
            progress_bar.set_style(template.clone());
        }

        if let Some(duration) = self.steady_tick {
            progress_bar.enable_steady_tick(duration);
        }

        let progress_bar = self.place_progress_bar(progress_bar);

        let mut progress_bars = self.progress_bars.lock().unwrap();
        progress_bars.push(progress_bar);
        progress_bars.len() - 1
    }

    fn on_download_complete(&self, _url: &Url, index: usize) {
        if let Some(progress_bar) = self.progress_bars.lock().unwrap().get(index) {
            if let Some(template) = &self.finish_template {
                progress_bar.set_style(template.clone());
            }

            if let Some(message) = &self.finish_message {
                progress_bar.finish_with_message(message.clone());
            } else {
                progress_bar.finish();
            }
        }
    }

    fn on_download_progress(&self, _url: &Url, index: usize, bytes: usize, total: Option<usize>) {
        if let Some(progress_bar) = self.progress_bars.lock().unwrap().get(index) {
            progress_bar.set_length(total.unwrap_or(bytes) as u64);
            progress_bar.set_position(bytes as u64);
        }
    }
}

impl Reporter for GatewayReporter {
    fn jlap_reporter(&self) -> Option<&dyn JLAPReporter> {
        None
    }

    fn download_reporter(&self) -> Option<&dyn DownloadReporter> {
        Some(self)
    }
}

impl GatewayReporterBuilder {
    /// Configure the multi progress instance.
    #[must_use]
    pub fn with_multi_progress(mut self, multi_progress: MultiProgress) -> Self {
        self.multi_progress = Some(multi_progress);
        self
    }

    /// Configure the progress template style.
    #[must_use]
    pub fn with_progress_template(mut self, template: ProgressStyle) -> Self {
        self.progress_template = Some(template);
        self
    }

    /// Configure the finish template style.
    #[must_use]
    pub fn with_finish_template(mut self, template: ProgressStyle) -> Self {
        self.finish_template = Some(template);
        self
    }

    /// Configure the prefix shown for the progress bar.
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Configure the finish message shown when the download completes.
    #[must_use]
    pub fn with_finish_message(mut self, message: impl Into<String>) -> Self {
        self.finish_message = Some(message.into());
        self
    }

    /// Enable steady ticking for the progress bar.
    #[must_use]
    pub fn with_steady_tick(mut self, duration: Duration) -> Self {
        self.steady_tick = Some(duration);
        self
    }

    /// Configure where to place the progress bar alongside other bars.
    #[must_use]
    pub fn with_placement(mut self, placement: Placement) -> Self {
        self.placement = Some(placement);
        self
    }

    /// Finalize the builder.
    pub fn finish(self) -> GatewayReporter {
        GatewayReporter {
            progress_bars: Arc::new(Mutex::new(Vec::new())),
            multi_progress: self.multi_progress.expect("multi progress is required"),
            progress_template: self.progress_template,
            finish_template: self.finish_template,
            prefix: self
                .prefix
                .unwrap_or_else(|| "Downloading repodata".to_string()),
            finish_message: Some(self.finish_message.unwrap_or_else(|| "Done".to_string())),
            steady_tick: self.steady_tick,
            placement: self.placement.unwrap_or_default(),
        }
    }
}
