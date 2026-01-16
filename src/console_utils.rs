//! This module contains utilities for logging and progress bar handling.
use std::{
    borrow::Cow,
    collections::hash_map::DefaultHasher,
    future::Future,
    hash::{Hash, Hasher},
    io,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use clap_verbosity_flag::{InfoLevel, Verbosity};
use console::{Style, style};
use indicatif::{
    HumanBytes, HumanDuration, MultiProgress, ProgressBar, ProgressState, ProgressStyle,
};
use tracing::{Level, field};
use tracing_core::{Event, Field, Subscriber, span::Id};
use tracing_subscriber::{
    EnvFilter, Layer,
    filter::{Directive, ParseError},
    fmt::{
        self, FmtContext, FormatEvent, FormatFields, MakeWriter,
        format::{self, Format},
    },
    layer::{Context, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
};

use crate::consts;

/// A palette of colors used for different package builds.
/// These are chosen to be visually distinct and readable on both light and dark terminals.
const SPAN_COLOR_PALETTE: &[console::Color] = &[
    console::Color::Cyan,
    console::Color::Green,
    console::Color::Yellow,
    console::Color::Blue,
    console::Color::Magenta,
    console::Color::Color256(208), // Orange
    console::Color::Color256(141), // Light purple
    console::Color::Color256(43),  // Teal
];

/// Returns a deterministic color for a given package identifier.
/// The color is chosen by hashing the identifier and selecting from the palette.
fn get_span_color(identifier: &str) -> Style {
    let mut hasher = DefaultHasher::new();
    identifier.hash(&mut hasher);
    let hash = hasher.finish();
    let color_index = (hash as usize) % SPAN_COLOR_PALETTE.len();
    Style::new().fg(SPAN_COLOR_PALETTE[color_index])
}

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

struct SpanInfo {
    id: Id,
    start_time: Instant,
    header: String,
    header_printed: bool,
    /// The color style used for this span's tree characters.
    /// This is inherited from parent spans or computed from the package identifier.
    color: Style,
}

#[derive(Default)]
struct SharedState {
    span_stack: Vec<SpanInfo>,
    warnings: Vec<String>,
}

impl std::fmt::Debug for SharedState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedState")
            .field("span_stack_len", &self.span_stack.len())
            .field("warnings", &self.warnings)
            .finish()
    }
}

struct CustomVisitor<'a> {
    writer: &'a mut dyn io::Write,
    result: io::Result<()>,
    /// Captures the span_color field for deterministic color computation.
    span_color: Option<String>,
}

impl<'a> CustomVisitor<'a> {
    fn new(writer: &'a mut dyn io::Write) -> Self {
        Self {
            writer,
            result: Ok(()),
            span_color: None,
        }
    }
}

impl field::Visit for CustomVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if self.result.is_err() {
            return;
        }

        // Capture span_color field for deterministic color computation
        if field.name() == "span_color" {
            self.span_color = Some(value.to_string());
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
            "package" => write!(self.writer, " package: {:?}", value),
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

/// Creates an indentation string with vertical bars colored according to each span's color.
fn indent_levels_colored(span_stack: &[SpanInfo]) -> String {
    let mut s = String::new();
    for span_info in span_stack {
        s.push_str(&format!(" {}", span_info.color.apply_to("│")));
    }
    s
}

impl<S> Layer<S> for LoggingOutputHandler
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing_core::span::Attributes<'_>,
        id: &Id,
        ctx: Context<'_, S>,
    ) {
        let mut state = self.state.lock().unwrap();

        if let Some(span) = ctx.span(id) {
            let mut s = Vec::new();
            let color_key = {
                let mut w = io::Cursor::new(&mut s);
                let mut visitor = CustomVisitor::new(&mut w);
                attrs.record(&mut visitor);
                visitor.span_color
            };
            let s = String::from_utf8_lossy(&s);

            let name = if s.is_empty() {
                span.name().to_string()
            } else {
                format!("{}{}", span.name(), s)
            };

            // Determine the color for this span:
            // - If there's a span_color field, compute color from it
            // - Otherwise, inherit from parent span
            // - If no parent, use gray (for initial/setup output)
            let span_color = if let Some(ref key) = color_key {
                get_span_color(key)
            } else if let Some(parent) = state.span_stack.last() {
                parent.color.clone()
            } else {
                Style::new().dim()
            };

            let indent = indent_levels_colored(&state.span_stack);
            let header = format!("{indent}\n{indent} {} {}", span_color.apply_to("╭─"), name);

            state.span_stack.push(SpanInfo {
                id: id.clone(),
                start_time: Instant::now(),
                header,
                header_printed: false,
                color: span_color,
            });
        }
    }

    fn on_exit(&self, id: &Id, _ctx: Context<'_, S>) {
        let mut state = self.state.lock().unwrap();

        if let Some(pos) = state.span_stack.iter().position(|info| &info.id == id) {
            let elapsed = state.span_stack[pos].start_time.elapsed();
            let header_printed = state.span_stack[pos].header_printed;
            let span_color = state.span_stack[pos].color.clone();

            // Get the indent before truncating (parent spans only)
            let indent = indent_levels_colored(&state.span_stack[..pos]);
            // For indent_plus_one, we need to include this span's color too
            let indent_plus_one = format!("{} {}", indent, span_color.apply_to("│"));

            state.span_stack.truncate(pos);

            if !header_printed {
                return;
            }

            eprintln!(
                "{indent_plus_one}\n{indent} {} (took {})",
                span_color.apply_to("╰───────────────────"),
                HumanDuration(elapsed)
            );
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut state = self.state.lock().unwrap();
        let indent = indent_levels_colored(&state.span_stack);

        // Print pending headers
        for span_info in &mut state.span_stack {
            if !span_info.header_printed {
                eprintln!("{}", span_info.header);
                span_info.header_printed = true;
            }
        }

        let mut s = Vec::new();
        event.record(&mut CustomVisitor::new(&mut s));
        let message = String::from_utf8_lossy(&s);

        let (prefix, prefix_len) = if event.metadata().level() <= &Level::WARN {
            state.warnings.push(message.to_string());
            if event.metadata().level() == &Level::ERROR {
                (style("× error ").red().bold(), 7)
            } else {
                (style("⚠ warning ").yellow().bold(), 9)
            }
        } else {
            (style(""), 0)
        };

        self.progress_bars.suspend(|| {
            if !self.wrap_lines {
                for line in message.lines() {
                    eprintln!("{} {}{}", indent, prefix, line);
                }
            } else {
                let width = terminal_size::terminal_size()
                    .map(|(w, _)| w.0)
                    .unwrap_or(160) as usize;
                let max_width = width - (state.span_stack.len() * 2) - 1 - prefix_len;

                for line in message.lines() {
                    if line.len() <= max_width {
                        eprintln!("{} {}{}", indent, prefix, line);
                    } else {
                        for chunk in chunk_string_without_ansi(line, max_width) {
                            eprintln!("{} {}{}", indent, prefix, chunk);
                        }
                    }
                }
            }
        });
    }
}

/// A custom output handler for fancy logging.
#[derive(Debug)]
pub struct LoggingOutputHandler {
    state: Arc<Mutex<SharedState>>,
    wrap_lines: bool,
    progress_bars: MultiProgress,
    writer: io::Stderr,
}

impl Clone for LoggingOutputHandler {
    fn clone(&self) -> Self {
        Self {
            wrap_lines: self.wrap_lines,
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
            wrap_lines: true,
            state: Arc::new(Mutex::new(SharedState::default())),
            progress_bars: MultiProgress::new(),
            writer: io::stderr(),
        }
    }
}

impl LoggingOutputHandler {
    /// Return a string with the current indentation level (bars added to the
    /// front of the string), colored according to each span's color.
    pub fn with_indent_levels(&self, template: &str) -> String {
        let state = self.state.lock().unwrap();
        let indent_str = indent_levels_colored(&state.span_stack);
        format!("{} {}", indent_str, template)
    }

    /// Return the multi-progress instance.
    pub fn multi_progress(&self) -> &MultiProgress {
        &self.progress_bars
    }

    /// Set the multi-progress instance.
    pub fn with_multi_progress(mut self, multi_progress: MultiProgress) -> Self {
        self.progress_bars = multi_progress;
        self
    }

    /// Returns the style to use for a progressbar that is currently in
    /// progress.
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

    /// Returns the style to use for a progressbar that is currently in
    /// progress.
    pub fn default_progress_style(&self) -> indicatif::ProgressStyle {
        let template_str = self.with_indent_levels(
            "{spinner:.green} {prefix:20!} [{elapsed_precise}] [{bar:40!.bright.yellow/dim.white}] {pos:>7}/{len:7}"
        );
        indicatif::ProgressStyle::default_bar()
            .template(&template_str)
            .unwrap()
            .progress_chars("━━╾─")
    }

    /// Returns the style to use for a progressbar that is in Deserializing
    /// state.
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
            "{} {{prefix:20!}} [{{elapsed_precise}}] {{msg:.bold.green}}",
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

    /// Returns the style to use for a progressbar that is indeterminate and
    /// simply shows a spinner.
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

    /// Displays a spinner with the given message while running the specified
    /// function to completion.
    pub fn wrap_in_progress<T, F: FnOnce() -> T>(
        &self,
        msg: impl Into<Cow<'static, str>>,
        func: F,
    ) -> T {
        let pb = self.add_progress_bar(
            ProgressBar::hidden()
                .with_style(self.long_running_progress_style())
                .with_message(msg),
        );
        pb.enable_steady_tick(Duration::from_millis(100));
        let result = func();
        pb.finish_and_clear();
        result
    }

    /// Displays a spinner with the given message while running the specified
    /// function to completion.
    pub async fn wrap_in_progress_async<T, Fut: Future<Output = T>>(
        &self,
        msg: impl Into<Cow<'static, str>>,
        future: Fut,
    ) -> T {
        self.wrap_in_progress_async_with_progress(msg, |_pb| future)
            .await
    }

    /// Displays a spinner with the given message while running the specified
    /// function to completion.
    pub async fn wrap_in_progress_async_with_progress<
        T,
        Fut: Future<Output = T>,
        F: FnOnce(ProgressBar) -> Fut,
    >(
        &self,
        msg: impl Into<Cow<'static, str>>,
        f: F,
    ) -> T {
        let pb = self.add_progress_bar(
            ProgressBar::hidden()
                .with_style(self.long_running_progress_style())
                .with_message(msg),
        );
        pb.enable_steady_tick(Duration::from_millis(100));
        let result = f(pb.clone()).await;
        pb.finish_and_clear();
        result
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

/// Constructs a default [`EnvFilter`] that is used when the user did not
/// specify a custom RUST_LOG.
pub fn get_default_env_filter(
    verbose: clap_verbosity_flag::log::LevelFilter,
) -> Result<EnvFilter, ParseError> {
    let mut result = EnvFilter::new(format!("rattler_build={verbose}"));

    if verbose >= clap_verbosity_flag::log::LevelFilter::Trace {
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

        let mut message = Vec::new();
        event.record(&mut CustomVisitor::new(&mut message));
        let message = String::from_utf8_lossy(&message);

        match *metadata.level() {
            Level::ERROR => println!("::error ::{}", message),
            Level::WARN => println!("::warning ::{}", message),
            _ => {}
        }
    }
}

/// Whether to use colors in the output.
#[derive(clap::ValueEnum, Clone, Eq, PartialEq, Debug, Copy, Default)]
pub enum Color {
    /// Always use colors.
    Always,
    /// Never use colors.
    Never,
    /// Use colors when the output is a terminal.
    #[default]
    Auto,
}

/// Initializes logging with the given style and verbosity.
pub fn init_logging(
    log_style: &LogStyle,
    verbosity: &Verbosity<InfoLevel>,
    color: &Color,
    wrap_lines: Option<bool>,
    #[cfg(feature = "tui")] tui_log_sender: Option<
        tokio::sync::mpsc::UnboundedSender<crate::tui::event::Event>,
    >,
) -> Result<LoggingOutputHandler, ParseError> {
    let mut log_handler = LoggingOutputHandler::default();

    // Wrap lines by default, but disable it in CI
    if let Some(wrap_lines) = wrap_lines {
        log_handler.wrap_lines = wrap_lines;
    } else if std::env::var("CI").is_ok() {
        log_handler.wrap_lines = false;
    }

    let use_colors = match color {
        Color::Always => Some(true),
        Color::Never => Some(false),
        Color::Auto => None,
    };

    // Enable disable colors for the colors crate
    if let Some(use_colors) = use_colors {
        console::set_colors_enabled(use_colors);
        console::set_colors_enabled_stderr(use_colors);
    }

    // Setup tracing subscriber
    let registry =
        tracing_subscriber::registry().with(get_default_env_filter(verbosity.log_level_filter())?);

    let log_style = if verbosity.log_level_filter() >= clap_verbosity_flag::log::LevelFilter::Debug
    {
        LogStyle::Plain
    } else {
        *log_style
    };

    let registry = registry.with(GitHubActionsLayer(github_integration_enabled()));

    #[cfg(feature = "tui")]
    {
        if let Some(tui_log_sender) = tui_log_sender {
            log_handler.set_progress_bars_hidden(true);
            let writer = crate::tui::logger::TuiOutputHandler {
                log_sender: tui_log_sender,
            };
            registry
                .with(
                    fmt::layer()
                        .with_writer(writer)
                        .without_time()
                        .with_level(false)
                        .with_target(false),
                )
                .init();
            return Ok(log_handler);
        }
    }

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
                .with(fmt::layer().json().with_writer(io::stderr))
                .init();
        }
    }

    Ok(log_handler)
}

/// Checks whether we are on GitHub Actions and if the user has enabled the GitHub integration
pub fn github_integration_enabled() -> bool {
    github_action_runner()
        && std::env::var(consts::RATTLER_BUILD_ENABLE_GITHUB_INTEGRATION) == Ok("true".to_string())
}

/// Checks whether we are on GitHub Actions
pub fn github_action_runner() -> bool {
    std::env::var(consts::GITHUB_ACTIONS) == Ok("true".to_string())
}
