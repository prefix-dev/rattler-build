//! [`OutputSink`] implementations that post-process script output on the host.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use fs_err as fs;

use super::{OutputSink, OutputStream};

/// Rewrites each output line with a set of string replacements before
/// forwarding it to an inner [`OutputSink`].
///
/// Builds use this to turn absolute prefixes back into `$PREFIX`-style
/// placeholders and to mask secrets. Every occurrence of every key is
/// replaced. The replacements are applied one after another, longest key
/// first and ties broken by key, so a prefix path is rewritten before a
/// shorter secret that happens to be a substring of it, and the inner sink
/// only ever sees the fully rewritten line. Empty keys are ignored, because
/// replacing the empty string would insert the value between every character.
pub struct ReplacingSink<S> {
    replacements: Vec<(String, String)>,
    inner: S,
}

impl<S: OutputSink> ReplacingSink<S> {
    /// Creates a sink that replaces each key of `replacements` with its value
    /// and forwards the result to `inner`.
    pub fn new(replacements: HashMap<String, String>, inner: S) -> Self {
        let mut replacements: Vec<(String, String)> = replacements
            .into_iter()
            .filter(|(from, _)| !from.is_empty())
            .collect();
        replacements.sort_by(|(left, _), (right, _)| {
            right.len().cmp(&left.len()).then_with(|| left.cmp(right))
        });
        Self {
            replacements,
            inner,
        }
    }

    /// Returns the wrapped sink.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: OutputSink> OutputSink for ReplacingSink<S> {
    fn line(&mut self, stream: OutputStream, line: &str) {
        let rewritten = self
            .replacements
            .iter()
            .fold(line.to_string(), |acc, (from, to)| acc.replace(from, to));
        self.inner.line(stream, &rewritten);
    }
}

/// Records script output on the host: every line is appended to the build log
/// file, emitted as a `tracing` info event, and kept in memory per stream.
///
/// A failed log write is reported with a warning and never interrupts the
/// script, so the captured text stays complete even when the log file does
/// not. Lines are written to the log unbuffered, so the file reflects the
/// script progress while it runs.
pub struct BuildLogSink {
    log_file: fs::File,
    stdout: String,
    stderr: String,
}

impl BuildLogSink {
    /// Creates a sink that appends to `log_file`.
    fn new(log_file: fs::File) -> Self {
        Self {
            log_file,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Opens the build log at `path` for appending, creating it if needed.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let log_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self::new(log_file))
    }

    /// Returns the captured standard output and standard error, in that
    /// order, with each line followed by a newline.
    pub fn into_captured(self) -> (String, String) {
        (self.stdout, self.stderr)
    }
}

impl OutputSink for BuildLogSink {
    fn line(&mut self, stream: OutputStream, line: &str) {
        let line_with_newline = format!("{line}\n");
        let captured = match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr => &mut self.stderr,
        };
        captured.push_str(&line_with_newline);

        if let Err(error) = self.log_file.write_all(line_with_newline.as_bytes()) {
            tracing::warn!("Failed to write to build log: {:?}", error);
        }

        tracing::info!("{}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::test_support::VecSink;

    #[test]
    fn replacing_sink_rewrites_every_occurrence_on_both_streams() {
        let mut replacements = HashMap::new();
        replacements.insert("/opt/host".to_string(), "$PREFIX".to_string());
        replacements.insert("/opt/build".to_string(), "$BUILD_PREFIX".to_string());
        replacements.insert("hunter2".to_string(), "********".to_string());
        let mut sink = ReplacingSink::new(replacements, VecSink::default());

        sink.line(OutputStream::Stdout, "/opt/host/bin -> /opt/host/lib");
        sink.line(OutputStream::Stderr, "token=hunter2 in /opt/build");
        sink.line(OutputStream::Stdout, "untouched");

        assert_eq!(
            sink.into_inner().0,
            vec![
                (
                    OutputStream::Stdout,
                    "$PREFIX/bin -> $PREFIX/lib".to_string()
                ),
                (
                    OutputStream::Stderr,
                    "token=******** in $BUILD_PREFIX".to_string()
                ),
                (OutputStream::Stdout, "untouched".to_string()),
            ]
        );
    }

    #[test]
    fn replacing_sink_applies_longest_key_first() {
        let mut replacements = HashMap::new();
        replacements.insert("/opt/host".to_string(), "$PREFIX".to_string());
        replacements.insert("host".to_string(), "********".to_string());
        let mut sink = ReplacingSink::new(replacements, VecSink::default());

        sink.line(OutputStream::Stdout, "/opt/host/lib for host");

        assert_eq!(
            sink.into_inner().0,
            vec![(OutputStream::Stdout, "$PREFIX/lib for ********".to_string())]
        );
    }

    /// A declared secret whose value is empty must not rewrite anything.
    #[test]
    fn replacing_sink_ignores_empty_keys() {
        let mut replacements = HashMap::new();
        replacements.insert(String::new(), "********".to_string());
        let mut sink = ReplacingSink::new(replacements, VecSink::default());

        sink.line(OutputStream::Stdout, "unchanged line");
        sink.line(OutputStream::Stderr, "");

        assert_eq!(
            sink.into_inner().0,
            vec![
                (OutputStream::Stdout, "unchanged line".to_string()),
                (OutputStream::Stderr, String::new()),
            ]
        );
    }

    #[test]
    fn build_log_sink_appends_to_log_and_captures_per_stream() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("conda_build.log");
        fs::write(&log_path, "existing\n").unwrap();
        let mut sink = BuildLogSink::open(&log_path).unwrap();

        sink.line(OutputStream::Stdout, "first out");
        sink.line(OutputStream::Stderr, "first err");
        sink.line(OutputStream::Stdout, "second out");

        assert_eq!(
            fs::read_to_string(&log_path).unwrap(),
            "existing\nfirst out\nfirst err\nsecond out\n"
        );
        assert_eq!(
            sink.into_captured(),
            (
                "first out\nsecond out\n".to_string(),
                "first err\n".to_string()
            )
        );
    }

    #[test]
    fn build_log_sink_open_fails_without_parent_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("missing").join("conda_build.log");

        assert!(BuildLogSink::open(&log_path).is_err());
    }

    /// A log file that rejects writes must not interrupt the script: the line
    /// is still captured and the sink keeps accepting output.
    #[test]
    fn build_log_sink_keeps_capturing_when_log_write_fails() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("conda_build.log");
        fs::write(&log_path, "").unwrap();
        let read_only = fs::File::open(&log_path).unwrap();
        let mut sink = BuildLogSink::new(read_only);

        sink.line(OutputStream::Stdout, "out");
        sink.line(OutputStream::Stderr, "err");

        assert_eq!(fs::read_to_string(&log_path).unwrap(), "");
        assert_eq!(
            sink.into_captured(),
            ("out\n".to_string(), "err\n".to_string())
        );
    }
}
