//! This module contains utilities for logging and progress bar handling.
use clap_verbosity_flag::{InfoLevel, Verbosity};
use console::style;
use indicatif::{HumanBytes, HumanDuration, MultiProgress, ProgressState, ProgressStyle};
use std::{
    collections::HashMap,
    io,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Instant,
};
use tracing::{field, Level};
use tracing_core::{span::Id, Event, Field, Subscriber};
use tracing_subscriber::{
    filter::{Directive, ParseError},
    fmt::{
        self,
        format::{self, Format},
        FmtContext, FormatEvent, FormatFields, MakeWriter,
    },
    layer::{Context, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

/// A custom formatter for tracing events.
pub struct TracingFormatter;

impl<S, N> FormatEvent<S, N> for TracingFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: format::Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();
        if *metadata.level() == tracing_core::metadata::Level::INFO
            && metadata.target().starts_with("rattler_build")
        {
            ctx.format_fields(writer.by_ref(), event)?;
            writeln!(writer)
        } else {
            let default_format = Format::default();
            default_format.format_event(ctx, writer, event)
        }
    }
}

#[derive(Debug, Default)]
struct SharedState {
    indentation_level: usize,
    timestamps: HashMap<Id, Instant>,
    formatted_spans: HashMap<Id, String>,
    warnings: Vec<String>,
}

struct CustomVisitor<'a> {
    writer: &'a mut dyn io::Write,
    result: io::Result<()>,
}

impl<'a> CustomVisitor<'a> {
    fn new(writer: &'a mut dyn io::Write) -> Self {
        Self {
            writer,
            result: Ok(()),
        }
    }
}

impl<'a> field::Visit for CustomVisitor<'a> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if self.result.is_err() {
            return;
        }

        self.record_debug(field, &format_args!("{}", value))
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if self.result.is_err() {
            return;
        }

        self.result = match field.name() {
            "message" => write!(self.writer, "{:?}", value),
            "recipe" => write!(self.writer, " recipe: {:?}", value),
            _ => Ok(()),
        };
    }
}

fn chunk_string_without_ansi(input: &str, max_chunk_length: usize) -> Vec<String> {
    let mut chunks: Vec<String> = vec![];
    let mut current_chunk = String::new();
    let mut current_length = 0;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1B' {
            // Beginning of an ANSI escape sequence
            current_chunk.push(c);
            while let Some(&next_char) = chars.peek() {
                // Add to current chunk
                current_chunk.push(chars.next().unwrap());
                if next_char.is_ascii_alphabetic() {
                    // End of ANSI escape sequence
                    break;
                }
            }
        } else {
            // Regular character
            current_length += 1;
            current_chunk.push(c);
            if current_length == max_chunk_length {
                // Current chunk is full
                chunks.push(current_chunk);
                current_chunk = String::new();
                current_length = 0;
            }
        }
    }

    // Add the last chunk if it's not empty
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

fn indent_levels(indent: usize) -> String {
    let mut s = String::new();
    for _ in 0..indent {
        s.push_str(" │");
    }
    format!("{}", style(s).cyan())
}

impl<S> Layer<S> for LoggingOutputHandler
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing_core::span::Attributes<'_>,
        id: &tracing_core::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut state = self.state.lock().unwrap();
        state.timestamps.insert(id.clone(), Instant::now());
        let span = ctx.span(id);

        if let Some(span) = span {
            let mut s = Vec::new();
            let mut w = io::Cursor::new(&mut s);
            attrs.record(&mut CustomVisitor::new(&mut w));
            let s = String::from_utf8_lossy(w.get_ref());

            if !s.is_empty() {
                state
                    .formatted_spans
                    .insert(id.clone(), format!("{}{}", span.name(), s));
            } else {
                state
                    .formatted_spans
                    .insert(id.clone(), span.name().to_string());
            }
        }
    }

    fn on_enter(&self, id: &Id, _ctx: Context<'_, S>) {
        let mut state = self.state.lock().unwrap();
        let ind = indent_levels(state.indentation_level);
        if let Some(txt) = state.formatted_spans.get(id) {
            eprintln!("{ind}\n{ind} {} {}", style("╭─").cyan(), txt);
        }

        state.indentation_level += 1;
    }

    fn on_exit(&self, id: &Id, _ctx: Context<'_, S>) {
        let mut state = self.state.lock().unwrap();

        let prev_ind = indent_levels(state.indentation_level);

        if state.indentation_level > 0 {
            state.indentation_level -= 1;
        }

        let ind = indent_levels(state.indentation_level);

        let elapsed_time = state
            .timestamps
            .remove(id)
            .map(|t| t.elapsed())
            .unwrap_or_default();

        let human_duration = HumanDuration(elapsed_time);

        eprintln!(
            "{prev_ind}\n{ind} {} (took {})",
            style("╰───────────────────").cyan(),
            human_duration
        );
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut state = self.state.lock().unwrap();
        let indent_str = indent_levels(state.indentation_level);

        let mut s = Vec::new();
        event.record(&mut CustomVisitor::new(&mut s));
        let s = String::from_utf8_lossy(&s);

        let (prefix, prefix_len) =
            if event.metadata().level() <= &tracing_core::metadata::Level::WARN {
                state.warnings.push(s.to_string());
                if event.metadata().level() == &tracing_core::metadata::Level::ERROR {
                    (style("× error ").red().bold(), 7)
                } else {
                    (style("⚠ warning ").yellow().bold(), 9)
                }
            } else {
                (style(""), 0)
            };

        let width: usize = terminal_size::terminal_size()
            .map(|(w, _)| w.0)
            .unwrap_or(160) as usize;

        let max_width = width - (state.indentation_level * 2) - 1 - prefix_len;

        self.progress_bars.suspend(|| {
            for line in s.lines() {
                // split line into max_width chunks
                if line.len() <= max_width {
                    eprintln!("{} {}{}", indent_str, prefix, line);
                } else {
                    chunk_string_without_ansi(line, max_width)
                        .iter()
                        .for_each(|chunk| {
                            eprintln!("{} {}{}", indent_str, prefix, chunk);
                        });
                }
            }
        });
    }
}

/// A custom output handler for fancy logging.
#[derive(Debug)]
pub struct LoggingOutputHandler {
    state: Arc<Mutex<SharedState>>,
    progress_bars: MultiProgress,
    writer: io::Stderr,
}

impl Clone for LoggingOutputHandler {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            progress_bars: self.progress_bars.clone(),
            writer: io::stderr(),
        }
    }
}

impl Default for LoggingOutputHandler {
    /// Creates a new output handler.
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedState::default())),
            progress_bars: MultiProgress::new(),
            writer: io::stderr(),
        }
    }
}

impl LoggingOutputHandler {
    /// Create a new logging handler with the given multi-progress.
    pub fn from_multi_progress(multi_progress: MultiProgress) -> LoggingOutputHandler {
        Self {
            state: Arc::new(Mutex::new(SharedState::default())),
            progress_bars: multi_progress,
            writer: io::stderr(),
        }
    }

    fn with_indent_levels(&self, template: &str) -> String {
        let state = self.state.lock().unwrap();
        let indent_str = indent_levels(state.indentation_level);
        format!("{} {}", indent_str, template)
    }

    /// Returns the style to use for a progressbar that is currently in progress.
    pub fn default_bytes_style(&self) -> indicatif::ProgressStyle {
        let template_str = self.with_indent_levels(
            "{spinner:.green} {prefix:20!} [{elapsed_precise}] [{bar:40!.bright.yellow/dim.white}] {bytes:>8} @ {smoothed_bytes_per_sec:8}"
        );

        indicatif::ProgressStyle::default_bar()
            .template(&template_str)
            .unwrap()
            .progress_chars("━━╾─")
            .with_key(
                "smoothed_bytes_per_sec",
                |s: &ProgressState, w: &mut dyn std::fmt::Write| match (
                    s.pos(),
                    s.elapsed().as_millis(),
                ) {
                    (pos, elapsed_ms) if elapsed_ms > 0 => {
                        // TODO: log with tracing?
                        _ = write!(
                            w,
                            "{}/s",
                            HumanBytes((pos as f64 * 1000_f64 / elapsed_ms as f64) as u64)
                        );
                    }
                    _ => {
                        _ = write!(w, "-");
                    }
                },
            )
    }

    /// Returns the style to use for a progressbar that is currently in progress.
    pub fn default_progress_style(&self) -> indicatif::ProgressStyle {
        let template_str = self.with_indent_levels(
            "{spinner:.green} {prefix:20!} [{elapsed_precise}] [{bar:40!.bright.yellow/dim.white}] {pos:>7}/{len:7}"
        );
        indicatif::ProgressStyle::default_bar()
            .template(&template_str)
            .unwrap()
            .progress_chars("━━╾─")
    }

    /// Returns the style to use for a progressbar that is in Deserializing state.
    pub fn deserializing_progress_style(&self) -> indicatif::ProgressStyle {
        let template_str =
            self.with_indent_levels("{spinner:.green} {prefix:20!} [{elapsed_precise}] {wide_msg}");
        indicatif::ProgressStyle::default_bar()
            .template(&template_str)
            .unwrap()
            .progress_chars("━━╾─")
    }

    /// Returns the style to use for a progressbar that is finished.
    pub fn finished_progress_style(&self) -> indicatif::ProgressStyle {
        let template_str = self.with_indent_levels(&format!(
            "{} {{spinner:.green}} {{prefix:20!}} [{{elapsed_precise}}] {{msg:.bold.green}}",
            console::style(console::Emoji("✔", " ")).green()
        ));

        indicatif::ProgressStyle::default_bar()
            .template(&template_str)
            .unwrap()
            .progress_chars("━━╾─")
    }

    /// Returns the style to use for a progressbar that is in error state.
    pub fn errored_progress_style(&self) -> indicatif::ProgressStyle {
        let template_str = self.with_indent_levels(&format!(
            "{} {{prefix:20!}} [{{elapsed_precise}}] {{msg:.bold.red}}",
            console::style(console::Emoji("×", " ")).red()
        ));

        indicatif::ProgressStyle::default_bar()
            .template(&template_str)
            .unwrap()
            .progress_chars("━━╾─")
    }

    /// Returns the style to use for a progressbar that is indeterminate and simply shows a spinner.
    pub fn long_running_progress_style(&self) -> indicatif::ProgressStyle {
        let template_str = self.with_indent_levels("{spinner:.green} {msg}");
        ProgressStyle::with_template(&template_str).unwrap()
    }

    /// Adds a progress bar to the handler.
    pub fn add_progress_bar(&self, progress_bar: indicatif::ProgressBar) -> indicatif::ProgressBar {
        self.progress_bars.add(progress_bar)
    }

    /// Set progress bars to hidden
    pub fn set_progress_bars_hidden(&self, hidden: bool) {
        self.progress_bars.set_draw_target(if hidden {
            indicatif::ProgressDrawTarget::hidden()
        } else {
            indicatif::ProgressDrawTarget::stderr()
        });
    }
}

impl io::Write for LoggingOutputHandler {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress_bars.suspend(|| self.writer.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.progress_bars.suspend(|| self.writer.flush())
    }
}

impl<'a> MakeWriter<'a> for LoggingOutputHandler {
    type Writer = LoggingOutputHandler;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
///////////////////////
// LOGGING CLI utils //
///////////////////////

/// The style to use for logging output.
#[derive(clap::ValueEnum, Clone, Eq, PartialEq, Debug, Copy)]
pub enum LogStyle {
    /// Use fancy logging output.
    Fancy,
    /// Use JSON logging output.
    Json,
    /// Use plain logging output.
    Plain,
}

/// Constructs a default [`EnvFilter`] that is used when the user did not specify a custom RUST_LOG.
pub fn get_default_env_filter(
    verbose: clap_verbosity_flag::LevelFilter,
) -> Result<EnvFilter, ParseError> {
    let mut result = EnvFilter::new(format!("rattler_build={verbose}"));

    if verbose >= clap_verbosity_flag::LevelFilter::Trace {
        result = result.add_directive(Directive::from_str("resolvo=info")?);
        result = result.add_directive(Directive::from_str("rattler=info")?);
        result = result.add_directive(Directive::from_str(
            "rattler_networking::authentication_storage=info",
        )?);
    } else {
        result = result.add_directive(Directive::from_str("resolvo=warn")?);
        result = result.add_directive(Directive::from_str("rattler=warn")?);
        result = result.add_directive(Directive::from_str("rattler_repodata_gateway::fetch=off")?);
        result = result.add_directive(Directive::from_str(
            "rattler_networking::authentication_storage=off",
        )?);
    }

    Ok(result)
}

struct GitHubActionsLayer(bool);

impl<S: Subscriber> Layer<S> for GitHubActionsLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if !self.0 {
            return;
        }
        let metadata = event.metadata();
        match *metadata.level() {
            Level::WARN => println!("::warning ::{}", format_args!("{:?}", event)),
            Level::ERROR => println!("::error ::{}", format_args!("{:?}", event)),
            _ => {} // Ignore other levels
        }
    }
}

/// Initializes logging with the given style and verbosity.
pub fn init_logging(
    log_style: &LogStyle,
    verbosity: &Verbosity<InfoLevel>,
) -> Result<LoggingOutputHandler, ParseError> {
    let log_handler = LoggingOutputHandler::default();

    // Setup tracing subscriber
    let registry =
        tracing_subscriber::registry().with(get_default_env_filter(verbosity.log_level_filter())?);

    let log_style = if verbosity.log_level_filter() >= clap_verbosity_flag::LevelFilter::Debug {
        LogStyle::Plain
    } else {
        *log_style
    };

    let registry = registry.with(GitHubActionsLayer(github_integration_enabled()));

    match log_style {
        LogStyle::Fancy => {
            registry.with(log_handler.clone()).init();
        }
        LogStyle::Plain => {
            registry
                .with(
                    fmt::layer()
                        .with_writer(log_handler.clone())
                        .event_format(TracingFormatter),
                )
                .init();
        }
        LogStyle::Json => {
            log_handler.set_progress_bars_hidden(true);
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(io::stderr),
                )
                .init();
        }
    }

    Ok(log_handler)
}

/// check if we are on Github CI nad if the user has enabled the integration
pub fn github_integration_enabled() -> bool {
    std::env::var("GITHUB_ACTIONS").is_ok()
        && std::env::var("RATTLER_BUILD_ENABLE_GITHUB_INTEGRATION") == Ok("true".to_string())
}
