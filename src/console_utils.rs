use indicatif::MultiProgress;
use std::io;
use tracing_core::{Event, Subscriber};
use tracing_subscriber::{
    fmt::{
        format::{self, Format, Writer},
        FmtContext, FormatEvent, FormatFields, MakeWriter,
    },
    registry::LookupSpan,
};

#[derive(Clone)]
pub struct IndicatifWriter {
    progress_bars: MultiProgress,
}

impl IndicatifWriter {
    pub(crate) fn new(pb: MultiProgress) -> Self {
        Self { progress_bars: pb }
    }
}

impl io::Write for IndicatifWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress_bars.suspend(|| io::stderr().write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stderr().flush())
    }
}

impl<'a> MakeWriter<'a> for IndicatifWriter {
    type Writer = IndicatifWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

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

        let mut buffer = String::new();
        let mut custom_writer = Writer::new(&mut buffer);
        // Needs support in tracing
        // custom_writer.with_ansi(true);

        if *metadata.level() == tracing_core::metadata::Level::INFO
            && metadata.target().starts_with("rattler_build")
        {
            ctx.format_fields(custom_writer.by_ref(), event)?;
        } else {
            let default_format = Format::default();
            default_format.format_event(ctx, custom_writer, event)?;
        }

        filter_secrets(&mut buffer);
        writer.write_str(&buffer)
    }
}

fn filter_secrets(buffer: &mut String) {
    // regex for conda tokens in URL /t/...
    let rex = regex::Regex::new(r"(/t/)([a-zA-Z0-9\-]{20,})").unwrap();
    *buffer = rex.replace_all(&buffer, "$1<token>").to_string();
}
