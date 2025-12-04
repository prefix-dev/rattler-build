//! Parse a Patch

use super::{ESCAPED_CHARS_BYTES, Hunk, HunkRange, Line, NO_NEWLINE_AT_EOF};
use crate::{
    LineEnd,
    patch::Diff,
    utils::{LineIter, Text},
};
use std::{borrow::Cow, fmt};

type Result<T, E = ParsePatchError> = std::result::Result<T, E>;

/// Kind of line start in `Hunk` header.
#[derive(Debug)]
pub enum HeaderLineKind {
    Adding,
    Removing,
}

impl fmt::Display for HeaderLineKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                HeaderLineKind::Adding => "+++",
                HeaderLineKind::Removing => "---",
            }
        )
    }
}

/// An error returned when parsing a `Patch` using [`Patch::from_str`] fails
///
/// [`Patch::from_str`]: struct.Patch.html#method.from_str
#[derive(thiserror::Error, Debug)]
pub enum ParsePatchError {
    #[error("unexpected end of file")]
    UnexpectedEof,
    #[error("multiple '{0}' lines")]
    HeaderMultipleLines(HeaderLineKind),
    #[error("unable to parse filename")]
    UnableToParseFilename,
    #[error("filename unterminated")]
    UnterminatedFilename,
    #[error("invalid char in unquoted filename")]
    InvalidCharInUnquotedFilename,
    #[error("expected escaped character")]
    ExpectedEscapedCharacter,
    #[error("invalid escaped character")]
    InvalidEscapedCharacter,
    #[error("invalid unescaped character")]
    InvalidUnescapedCharacter,
    #[error("no hunks found")]
    NoHunks,
    #[error("hunks not in order or overlap")]
    HunksOrder,
    #[error("hunk header does not match hunk")]
    HunkHeaderHunkMismatch,
    #[error("unable to parse hunk header")]
    HunkHeader,
    #[error("hunk header unterminated")]
    HunkHeaderUnterminated,
    #[error("can't parse range")]
    Range,
    #[error("expected end of hunk")]
    ExpectedEndOfHunk,
    #[error("expected no more deleted lines")]
    UnexpectedDeletedLine,
    #[error("expected no more inserted lines")]
    UnexpectedInsertLine,
    #[error("unexpected 'No newline at end of file' line")]
    UnexpectedNoNewlineAtEOF,
    #[error("unexpected line in hunk body")]
    UnexpectedLineInHunkBody,
    #[error("missing newline")]
    MissingNewline,
}

#[derive(Debug, Clone, Default)]
pub enum HunkRangeStrategy {
    /// Do not trust the line counts in the hunk headers and check
    /// that they are indeed match hunk lines.
    #[default]
    Check,
    /// Do not trust the line counts in the hunk headers, but infer
    /// them by inspecting the patch (e.g. after editing the patch
    /// without adjusting the hunk headers appropriately).
    ///
    /// Note that we also skip all empty lines at then end of the hunk
    /// lines before calculating hunk ranges.
    ///
    /// Tries to resemble behavior of `git apply` `--recount`
    /// argument.
    Recount,
    /// Trust the line counts in the hunk headers.
    Ignore,
}

#[derive(Debug, Clone, Default)]
pub struct ParserConfig {
    /// Choose what to do with hunk ranges.
    pub hunk_strategy: HunkRangeStrategy,
}

struct Parser<'a, T: Text + ?Sized> {
    lines: std::iter::Peekable<LineIter<'a, T>>,
    config: ParserConfig,
}

impl<'a, T: Text + ?Sized> Parser<'a, T> {
    fn new(input: &'a T) -> Self {
        Self::with_config(input, ParserConfig::default())
    }

    fn with_config(input: &'a T, config: ParserConfig) -> Self {
        Self {
            lines: LineIter::new(input).peekable(),
            config,
        }
    }

    fn peek(&mut self) -> Option<&(&'a T, Option<LineEnd>)> {
        self.lines.peek()
    }

    fn next(&mut self) -> Result<(&'a T, Option<LineEnd>)> {
        let line = self.lines.next().ok_or(ParsePatchError::UnexpectedEof)?;
        Ok(line)
    }
}

pub fn parse_multiple(input: &str) -> Result<Vec<Diff<'_, str>>> {
    parse_multiple_with_config(input, ParserConfig::default())
}

pub fn parse_multiple_with_config(input: &str, config: ParserConfig) -> Result<Vec<Diff<'_, str>>> {
    let mut parser = Parser::with_config(input, config);
    let mut patches = vec![];
    loop {
        match (patch_header(&mut parser), hunks(&mut parser)) {
            (Ok(header), Ok(hunks)) => {
                let original = header.0.map(|(line, _end)| convert_cow_to_str(line));
                let modified = header.1.map(|(line, _end)| convert_cow_to_str(line));
                patches.push(Diff::new(original, modified, hunks))
            }
            (Ok((None, None)), Err(_)) => break,
            // Allow NoHunks error when we have valid headers (pure renames/deletes/adds)
            (Ok(header), Err(ParsePatchError::NoHunks))
                if header.0.is_some() || header.1.is_some() =>
            {
                let original = header.0.map(|(line, _end)| convert_cow_to_str(line));
                let modified = header.1.map(|(line, _end)| convert_cow_to_str(line));
                patches.push(Diff::new(original, modified, vec![]))
            }
            (Ok(_), Err(e)) | (Err(e), _) => {
                return Err(e);
            }
        }
    }
    Ok(patches)
}

pub fn parse(input: &str) -> Result<Diff<'_, str>> {
    let mut parser = Parser::new(input);
    let header = patch_header(&mut parser)?;
    let hunks = hunks(&mut parser)?;

    let original = header.0.map(|(line, _end)| convert_cow_to_str(line));
    let modified = header.1.map(|(line, _end)| convert_cow_to_str(line));

    Ok(Diff::new(original, modified, hunks))
}

pub fn parse_bytes_multiple(input: &[u8]) -> Result<Vec<Diff<'_, [u8]>>> {
    parse_bytes_multiple_with_config(input, ParserConfig::default())
}

pub fn parse_bytes_multiple_with_config(
    input: &[u8],
    config: ParserConfig,
) -> Result<Vec<Diff<'_, [u8]>>> {
    let mut parser = Parser::with_config(input, config);
    let mut patches = vec![];
    loop {
        match (patch_header(&mut parser), hunks(&mut parser)) {
            (Ok(header), Ok(hunks)) => {
                let original = header.0.map(|(line, _end)| line);
                let modified = header.1.map(|(line, _end)| line);

                patches.push(Diff::new(original, modified, hunks))
            }
            (Ok((None, None)), Err(_)) | (Err(_), Err(_)) => break,
            // Allow NoHunks error when we have valid headers (pure renames/deletes/adds)
            (Ok(header), Err(ParsePatchError::NoHunks))
                if header.0.is_some() || header.1.is_some() =>
            {
                let original = header.0.map(|(line, _end)| line);
                let modified = header.1.map(|(line, _end)| line);
                patches.push(Diff::new(original, modified, vec![]))
            }
            (Ok(_), Err(e)) | (Err(e), Ok(_)) => {
                return Err(e);
            }
        }
    }
    Ok(patches)
}

pub fn parse_bytes(input: &[u8]) -> Result<Diff<'_, [u8]>> {
    let mut parser = Parser::new(input);
    let header = patch_header(&mut parser)?;
    let hunks = hunks(&mut parser)?;

    let original = header.0.map(|(line, _end)| line);
    let modified = header.1.map(|(line, _end)| line);

    Ok(Diff::new(original, modified, hunks))
}

// This is only used when the type originated as a utf8 string
fn convert_cow_to_str(cow: Cow<'_, [u8]>) -> Cow<'_, str> {
    match cow {
        Cow::Borrowed(b) => std::str::from_utf8(b).unwrap().into(),
        Cow::Owned(o) => String::from_utf8(o).unwrap().into(),
    }
}

#[allow(clippy::type_complexity)]
fn patch_header<'a, T: Text + ToOwned + ?Sized>(
    parser: &mut Parser<'a, T>,
) -> Result<(
    Option<(Cow<'a, [u8]>, Option<LineEnd>)>,
    Option<(Cow<'a, [u8]>, Option<LineEnd>)>,
)> {
    let (git_original, git_modified, saw_git_header) = header_preamble(parser)?;

    let mut filename1 = None;
    let mut filename2 = None;
    let mut saw_traditional_header1 = false;
    let mut saw_traditional_header2 = false;

    while let Some((line, _end)) = parser.peek() {
        if line.starts_with("--- ") {
            if saw_traditional_header1 {
                return Err(ParsePatchError::HeaderMultipleLines(
                    HeaderLineKind::Removing,
                ));
            }
            saw_traditional_header1 = true;
            filename1 = parse_filename("--- ", parser.next()?, saw_git_header)?;
        } else if line.starts_with("+++ ") {
            if saw_traditional_header2 {
                return Err(ParsePatchError::HeaderMultipleLines(HeaderLineKind::Adding));
            }
            saw_traditional_header2 = true;
            filename2 = parse_filename("+++ ", parser.next()?, saw_git_header)?;
        } else {
            break;
        }
    }

    // Traditional --- +++ headers take precedence over git metadata
    // If we saw a traditional header (even if it parsed to None for /dev/null), use it
    // Otherwise fall back to git metadata
    let original = if saw_traditional_header1 {
        filename1
    } else {
        git_original
    };
    let modified = if saw_traditional_header2 {
        filename2
    } else {
        git_modified
    };

    Ok((original, modified))
}

// Parse the patch header preamble, extracting filenames from git metadata.
// Skips preamble lines like "diff --git", git metadata, etc., until reaching
// the first filename header ("--- " or "+++ ") or hunk line.
// Returns extracted filenames from git metadata (for pure renames/deletes/adds)
// and a flag indicating whether we saw a git header (for prefix stripping).
#[allow(clippy::type_complexity)]
fn header_preamble<'a, T: Text + ToOwned + ?Sized>(
    parser: &mut Parser<'a, T>,
) -> Result<(
    Option<(Cow<'a, [u8]>, Option<LineEnd>)>,
    Option<(Cow<'a, [u8]>, Option<LineEnd>)>,
    bool, // saw_git_header
)> {
    let mut git_original = None;
    let mut git_modified = None;
    let mut rename_from = None;
    let mut rename_to = None;
    let mut seen_diff_git = false;

    while let Some((line, end)) = parser.peek() {
        if line.starts_with("--- ") | line.starts_with("+++ ") | line.starts_with("@@ ") {
            break;
        }

        // Parse git diff header: diff --git a/... b/...
        if line.starts_with("diff --git ") {
            // If we've already seen a diff --git line and have extracted data,
            // this is the start of the next patch, so stop here
            if seen_diff_git && (git_original.is_some() || rename_from.is_some()) {
                break;
            }
            seen_diff_git = true;

            if let Some(rest) = line.strip_prefix("diff --git ") {
                // Parse "a/file1 b/file2" (with prefixes) or "file1 file2" (without prefixes)
                // Try to split on " b/" first to detect standard format
                if let Some((file1, file2)) = rest.split_at_exclusive(" b/") {
                    // Standard format with b/ prefix
                    git_original = parse_git_filename(file1, true).map(|f| (f, *end));
                    git_modified = parse_git_filename(file2, true).map(|f| (f, *end));
                } else if let Some((file1, file2)) = rest.split_at_exclusive(" ") {
                    // --no-prefix format
                    git_original = parse_git_filename(file1, false).map(|f| (f, *end));
                    git_modified = parse_git_filename(file2, false).map(|f| (f, *end));
                }
                // If neither split works, skip this line (malformed diff --git line)
            }
        }
        // Parse rename from/to
        else if line.starts_with("rename from ")
            && let Some(filename) = line.strip_prefix("rename from ")
        {
            rename_from = Some((Cow::Borrowed(filename.as_bytes()), *end));
        } else if line.starts_with("rename to ")
            && let Some(filename) = line.strip_prefix("rename to ")
        {
            rename_to = Some((Cow::Borrowed(filename.as_bytes()), *end));
        }

        parser.next()?;
    }

    // Prefer rename from/to over git diff header
    let original = rename_from.or(git_original);
    let modified = rename_to.or(git_modified);

    Ok((original, modified, seen_diff_git))
}

#[allow(clippy::type_complexity)]
fn parse_filename<'a, T: Text + ToOwned + ?Sized>(
    prefix: &str,
    l: (&'a T, Option<LineEnd>),
    saw_git_header: bool,
) -> Result<Option<(Cow<'a, [u8]>, Option<LineEnd>)>> {
    let line =
        l.0.strip_prefix(prefix)
            .ok_or(ParsePatchError::UnableToParseFilename)?;

    let filename = if let Some((filename, _)) = line.split_at_exclusive("\t") {
        filename
    } else if let Some((filename, _)) = line.split_at_exclusive(" ") {
        filename
    } else if let Some((filename, _)) = line.split_at_exclusive("\n") {
        filename
    } else {
        line
    };

    // Check for /dev/null before parsing
    if filename.as_bytes() == b"/dev/null" {
        return Ok(None);
    }

    let mut parsed_filename = if let Some(quoted) = is_quoted(filename) {
        escaped_filename(quoted)?
    } else {
        unescaped_filename(filename)?
    };

    // Strip a/ or b/ prefix only if we saw a git header (git format uses these prefixes)
    if saw_git_header && let Cow::Borrowed(bytes) = parsed_filename {
        if let Some(rest) = std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.strip_prefix("a/"))
        {
            parsed_filename = Cow::Borrowed(rest.as_bytes());
        } else if let Some(rest) = std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.strip_prefix("b/"))
        {
            parsed_filename = Cow::Borrowed(rest.as_bytes());
        }
    }

    Ok(Some((parsed_filename, l.1)))
}

fn is_quoted<T: Text + ?Sized>(s: &T) -> Option<&T> {
    s.strip_prefix("\"").and_then(|s| s.strip_suffix("\""))
}

fn unescaped_filename<T: Text + ToOwned + ?Sized>(filename: &T) -> Result<Cow<'_, [u8]>> {
    // NOTE: may be a problem for other types of line feed except "\n" and "\r\n".
    let bytes = filename.as_bytes().trim_ascii_end();

    if bytes.iter().any(|b| ESCAPED_CHARS_BYTES.contains(b)) {
        return Err(ParsePatchError::InvalidCharInUnquotedFilename);
    }

    Ok(bytes.into())
}

fn escaped_filename<T: Text + ToOwned + ?Sized>(escaped: &T) -> Result<Cow<'_, [u8]>> {
    let mut filename = Vec::new();

    let mut chars = escaped.as_bytes().iter().copied();
    while let Some(c) = chars.next() {
        if c == b'\\' {
            let ch = match chars
                .next()
                .ok_or(ParsePatchError::ExpectedEscapedCharacter)?
            {
                b'n' => b'\n',
                b't' => b'\t',
                b'0' => b'\0',
                b'r' => b'\r',
                b'\"' => b'\"',
                b'\\' => b'\\',
                _ => return Err(ParsePatchError::InvalidEscapedCharacter),
            };
            filename.push(ch);
        } else if ESCAPED_CHARS_BYTES.contains(&c) {
            return Err(ParsePatchError::InvalidUnescapedCharacter);
        } else {
            filename.push(c);
        }
    }

    Ok(filename.into())
}

/// Parse a filename from a git diff header, handling:
/// - Standard format with a/ and b/ prefixes
/// - --no-prefix format without prefixes
/// - /dev/null for created/deleted files
///
/// Returns None for /dev/null (represents non-existent file)
fn parse_git_filename<T: Text + ?Sized>(filename: &T, has_prefix: bool) -> Option<Cow<'_, [u8]>> {
    // Check for /dev/null (file doesn't exist)
    if filename.as_bytes() == b"/dev/null" {
        return None;
    }

    // If we detected prefixes (found " b/" in the line), strip a/ or b/
    if has_prefix {
        if let Some(stripped) = filename.strip_prefix("a/") {
            return Some(Cow::Borrowed(stripped.as_bytes()));
        } else if let Some(stripped) = filename.strip_prefix("b/") {
            return Some(Cow::Borrowed(stripped.as_bytes()));
        }
    }

    // No prefix or couldn't strip, use as-is
    Some(Cow::Borrowed(filename.as_bytes()))
}

fn verify_hunks_in_order<T: ?Sized + ToOwned>(hunks: &[Hunk<'_, T>]) -> bool {
    for hunk in hunks.windows(2) {
        if hunk[0].old_range.end() > hunk[1].old_range.start()
            || hunk[0].new_range.end() > hunk[1].new_range.start()
        {
            return false;
        }
    }
    true
}

fn hunks<'a, T: Text + ?Sized + ToOwned>(parser: &mut Parser<'a, T>) -> Result<Vec<Hunk<'a, T>>> {
    let mut hunks = Vec::new();
    while parser.peek().is_some() {
        let r = hunk(parser);

        // TODO: Handle properly. For example there is case where hunk
        // is partially parsed. I think we want to make it hard error
        // instead or treating it as PS.
        if let Ok(h) = r {
            hunks.push(h);
        } else {
            break;
        }
    }

    if hunks.is_empty() {
        return Err(ParsePatchError::NoHunks);
    }

    // check and verify that the Hunks are in sorted order and don't overlap
    if !verify_hunks_in_order(&hunks) {
        return Err(ParsePatchError::HunksOrder);
    }

    Ok(hunks)
}

// Hunk ranges tolerance levels based on the end lines.
fn tolerance_level<T: Text + ?Sized + ToOwned>(lines: &[Line<'_, T>]) -> (usize, bool) {
    let mut tolerance = 0;
    let mut revlines = lines.iter().rev();
    while let Some(Line::Context((_, end))) = revlines.next() {
        if end.is_some() {
            tolerance += 1;
        } else {
            break;
        }
    }

    let line_ends_with_newline = matches!(revlines.next(), Some(Line::Context((_, Some(_)))));

    (tolerance, line_ends_with_newline)
}

fn hunk<'a, T: Text + ?Sized + ToOwned>(parser: &mut Parser<'a, T>) -> Result<Hunk<'a, T>> {
    let n = *parser.peek().ok_or(ParsePatchError::UnexpectedEof)?;
    let (mut range1, mut range2, function_context) = hunk_header(n)?;
    let _ = parser.next();
    let mut lines = hunk_lines(parser, &range1, &range2)?;

    // check counts of lines to see if they match the ranges in the hunk header
    let (len1, len2) = super::hunk_lines_count(&lines);

    match parser.config.hunk_strategy {
        HunkRangeStrategy::Check => {
            let t = tolerance_level(&lines);
            let tolerance = t.0 + usize::from(t.1);

            if len1.abs_diff(range1.len) > tolerance || len2.abs_diff(range2.len) > tolerance {
                return Err(ParsePatchError::HunkHeaderHunkMismatch);
            }
        }
        HunkRangeStrategy::Recount => {
            let empty_context_lines = lines
                .iter()
                .rev()
                .take_while(|l| match *l {
                    Line::Context(c) => c.0.len() == 0 && c.1.is_some(),
                    _ => false,
                })
                .count();

            lines = lines
                .into_iter()
                .rev()
                .skip(empty_context_lines)
                .rev()
                .collect();

            // Should never overflow since len{1,2} >= empty_context_lines by the defintion above.
            range1.len = len1 - empty_context_lines;
            range2.len = len2 - empty_context_lines;
        }
        HunkRangeStrategy::Ignore => (),
    }

    Ok(Hunk::new(range1, range2, function_context, lines))
}

type HunkHeader<'a, T> = (HunkRange, HunkRange, Option<(&'a T, Option<LineEnd>)>);

fn hunk_header<T: Text + ?Sized>(oinput: (&T, Option<LineEnd>)) -> Result<HunkHeader<'_, T>> {
    let input = oinput
        .0
        .strip_prefix("@@ ")
        .ok_or(ParsePatchError::HunkHeader)?;

    let (ranges, function_context) = input
        .split_at_exclusive(" @@")
        .ok_or(ParsePatchError::HunkHeaderUnterminated)?;
    let function_context = function_context.strip_prefix(" ");

    let (range1, range2) = ranges
        .split_at_exclusive(" ")
        .ok_or(ParsePatchError::HunkHeader)?;
    let range1 = range(
        range1
            .strip_prefix("-")
            .ok_or(ParsePatchError::HunkHeader)?,
    )?;
    let range2 = range(
        range2
            .strip_prefix("+")
            .ok_or(ParsePatchError::HunkHeader)?,
    )?;
    Ok((range1, range2, function_context.map(|fc| (fc, oinput.1))))
}

fn range<T: Text + ?Sized>(s: &T) -> Result<HunkRange> {
    let (start, len) = if let Some((start, len)) = s.split_at_exclusive(",") {
        (
            start.parse().ok_or(ParsePatchError::Range)?,
            len.parse().ok_or(ParsePatchError::Range)?,
        )
    } else {
        (s.parse().ok_or(ParsePatchError::Range)?, 1)
    };

    Ok(HunkRange::new(start, len))
}

fn hunk_lines<'a, T: Text + ?Sized + ToOwned>(
    parser: &mut Parser<'a, T>,
    old_range: &HunkRange,
    new_range: &HunkRange,
) -> Result<Vec<Line<'a, T>>> {
    let mut lines: Vec<Line<'a, T>> = Vec::new();
    let mut no_newline_context = false;
    let mut no_newline_delete = false;
    let mut no_newline_insert = false;

    // Track how many lines we've seen for each side
    let mut old_lines_seen = 0;
    let mut new_lines_seen = 0;

    // Calculate maximum lines we should read based on ranges
    let expected_old_lines = old_range.len;
    let expected_new_lines = new_range.len;

    while let Some(line) = parser.peek() {
        // Check if we've read enough lines based on the ranges,
        // but continue to check for the "No newline at end of file" marker
        if old_lines_seen >= expected_old_lines && new_lines_seen >= expected_new_lines {
            // Check if the next line is the "No newline at end of file" marker
            if !line.0.starts_with(NO_NEWLINE_AT_EOF) {
                // We've read all the lines we expect for this hunk
                break;
            }
        }

        let line = if no_newline_context {
            return Err(ParsePatchError::ExpectedEndOfHunk);
        } else if let Some(l) = line.0.strip_prefix(" ") {
            old_lines_seen += 1;
            new_lines_seen += 1;
            Line::Context((l, line.1))
        } else if line.0.len() == 0 && line.1.is_some() {
            old_lines_seen += 1;
            new_lines_seen += 1;
            Line::Context(*line)
        } else if let Some(l) = line.0.strip_prefix("-") {
            if no_newline_delete {
                return Err(ParsePatchError::UnexpectedDeletedLine);
            }
            old_lines_seen += 1;
            Line::Delete((l, line.1))
        } else if let Some(l) = line.0.strip_prefix("+") {
            if no_newline_insert {
                return Err(ParsePatchError::UnexpectedInsertLine);
            }
            new_lines_seen += 1;
            Line::Insert((l, line.1))
        } else if line.0.starts_with(NO_NEWLINE_AT_EOF) {
            let last_line = lines
                .pop()
                .ok_or(ParsePatchError::UnexpectedNoNewlineAtEOF)?;
            match last_line {
                Line::Context((line, _end)) => {
                    no_newline_context = true;
                    Line::Context((line, None))
                }
                Line::Delete((line, _end)) => {
                    no_newline_delete = true;
                    Line::Delete((line, None))
                }
                Line::Insert((line, _end)) => {
                    no_newline_insert = true;
                    Line::Insert((line, None))
                }
            }
        } else {
            return Err(ParsePatchError::UnexpectedLineInHunkBody);
        };

        lines.push(line);
        parser.next()?;
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use crate::patch::Line;
    use crate::patch::parse::{HunkRangeStrategy, ParserConfig, parse_multiple_with_config};

    use super::{parse, parse_bytes, parse_multiple};

    #[test]
    fn test_escaped_filenames() {
        // No escaped characters
        let s = "\
--- original
+++ modified
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap();
        parse_bytes(s.as_ref()).unwrap();

        // unescaped characters fail parsing
        let s = "\
--- ori\"ginal
+++ modified
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap_err();
        parse_bytes(s.as_ref()).unwrap_err();

        // quoted with invalid escaped characters
        let s = "\
--- \"ori\\\"g\rinal\"
+++ modified
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap_err();
        parse_bytes(s.as_ref()).unwrap_err();

        // quoted with escaped characters
        let s = r#"\
--- "ori\"g\tinal"
+++ "mo\0\t\r\n\\dified"
@@ -1,0 +1,1 @@
+Oathbringer
"#;
        let p = parse(s).unwrap();
        assert_eq!(p.original(), Some("ori\"g\tinal"));
        assert_eq!(p.modified(), Some("mo\0\t\r\n\\dified"));
        let b = parse_bytes(s.as_ref()).unwrap();
        assert_eq!(b.original(), Some(&b"ori\"g\tinal"[..]));
        assert_eq!(b.modified(), Some(&b"mo\0\t\r\n\\dified"[..]));
    }

    #[test]
    fn test_missing_filename_header() {
        // Missing Both '---' and '+++' lines
        let patch = r#"
@@ -1,11 +1,12 @@
 diesel::table! {
     users1 (id) {
-        id -> Nullable<Integer>,
+        id -> Integer,
     }
 }

 diesel::table! {
-    users2 (id) {
-        id -> Nullable<Integer>,
+    users2 (myid) {
+        #[sql_name = "id"]
+        myid -> Integer,
     }
 }
"#;

        parse(patch).unwrap();

        // Missing '---'
        let s = "\
+++ modified
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap();

        // Missing '+++'
        let s = "\
--- original
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap();

        // Headers out of order
        let s = "\
+++ modified
--- original
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap();

        // multiple headers should fail to parse
        let s = "\
--- original
--- modified
@@ -1,0 +1,1 @@
+Oathbringer
";
        parse(s).unwrap_err();
    }

    #[test]
    fn adjacent_hunks_correctly_parse() {
        let s = "\
--- original
+++ modified
@@ -110,7 +110,7 @@
 --

 I am afraid, however, that all I have known - that my story - will be forgotten.
 I am afraid for the world that is to come.
-Afraid that my plans will fail. Afraid of a doom worse than the Deepness.
+Afraid that Alendi will fail. Afraid of a doom brought by the Deepness.

 Alendi was never the Hero of Ages.
@@ -117,7 +117,7 @@
 At best, I have amplified his virtues, creating a Hero where there was none.

-At worst, I fear that all we believe may have been corrupted.
+At worst, I fear that I have corrupted all we believe.

 --
 Alendi must not reach the Well of Ascension. He must not take the power for himself.

";
        parse(s).unwrap();
    }

    #[test]
    fn test_real_world_patches() {
        insta::glob!("test-data/*.patch", |path| {
            let input = std::fs::read_to_string(path).unwrap();
            let patches = parse_multiple_with_config(
                &input,
                ParserConfig {
                    hunk_strategy: HunkRangeStrategy::Recount,
                },
            );
            insta::assert_debug_snapshot!(patches);
        });
    }

    #[test]
    fn test_multi_patch_file() {
        let input = std::fs::read_to_string("src/patch/test-data/40.patch").unwrap();

        let result = parse_multiple_with_config(
            &input,
            ParserConfig {
                hunk_strategy: HunkRangeStrategy::Recount,
            },
        );

        match &result {
            Ok(patches) => {
                // Should parse all 16 individual file changes from the 4 commits
                assert_eq!(
                    patches.len(),
                    16,
                    "Should parse all 16 file changes from the multi-commit patch"
                );
            }
            Err(e) => {
                panic!("Failed to parse multi-patch file: {:?}", e);
            }
        }
    }

    #[test]
    fn test_from_in_patch_content() {
        // Test that "From " with diff prefixes is correctly parsed as content,
        // while "From " without prefix acts as a boundary
        let patch_with_from_content = r#"--- a/email.txt
+++ b/email.txt
@@ -1,4 +1,4 @@
 To: someone@example.com
-From: old@example.com
+From: new@example.com
 Subject: Test
 Hello world
"#;

        let result = parse(patch_with_from_content).unwrap();
        assert_eq!(result.hunks().len(), 1);

        let hunk = &result.hunks()[0];
        let lines: Vec<_> = hunk.lines().iter().collect();
        assert_eq!(lines.len(), 5);

        // Verify the "From" lines are correctly parsed as delete/insert, not as boundary
        match lines[1] {
            Line::Delete((content, _)) => assert_eq!(*content, "From: old@example.com"),
            _ => panic!("Expected delete line with 'From: old@example.com'"),
        }
        match lines[2] {
            Line::Insert((content, _)) => assert_eq!(*content, "From: new@example.com"),
            _ => panic!("Expected insert line with 'From: new@example.com'"),
        }
    }

    #[test]
    fn test_pure_renames() {
        // Test parsing patches with pure renames (no hunks, only git metadata)
        let patch = r#"diff --git a/old_file.txt b/new_file.txt
similarity index 100%
rename from old_file.txt
rename to new_file.txt
diff --git a/another_old.txt b/another_new.txt
similarity index 100%
rename from another_old.txt
rename to another_new.txt
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 2, "Should parse two rename patches");

        // First rename
        assert_eq!(result[0].original(), Some("old_file.txt"));
        assert_eq!(result[0].modified(), Some("new_file.txt"));
        assert_eq!(
            result[0].hunks().len(),
            0,
            "Pure rename should have no hunks"
        );

        // Second rename
        assert_eq!(result[1].original(), Some("another_old.txt"));
        assert_eq!(result[1].modified(), Some("another_new.txt"));
        assert_eq!(
            result[1].hunks().len(),
            0,
            "Pure rename should have no hunks"
        );
    }

    #[test]
    fn test_deleted_file() {
        // Test parsing patches with deleted files
        let patch = r#"diff --git a/deleted_file.txt b/deleted_file.txt
deleted file mode 100644
index e69de29bb2d1d..0000000000000
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1, "Should parse one delete patch");

        assert_eq!(result[0].original(), Some("deleted_file.txt"));
        assert_eq!(result[0].modified(), Some("deleted_file.txt"));
        assert_eq!(
            result[0].hunks().len(),
            0,
            "Deleted file should have no hunks"
        );
    }

    #[test]
    fn test_git_diff_without_rename_metadata() {
        // Test that we can extract filenames from diff --git line even without rename from/to
        let patch = r#"diff --git a/file1.txt b/file2.txt
similarity index 100%
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        assert_eq!(result[0].original(), Some("file1.txt"));
        assert_eq!(result[0].modified(), Some("file2.txt"));
        assert_eq!(result[0].hunks().len(), 0);
    }

    #[test]
    fn test_git_diff_without_prefix() {
        // Test git diff --no-prefix format (no a/ and b/ prefixes)
        let patch = r#"diff --git old_file.txt new_file.txt
similarity index 100%
rename from old_file.txt
rename to new_file.txt
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1, "Should parse one rename patch");

        // Should prefer rename from/to over diff --git when both present
        assert_eq!(result[0].original(), Some("old_file.txt"));
        assert_eq!(result[0].modified(), Some("new_file.txt"));
        assert_eq!(result[0].hunks().len(), 0);
    }

    #[test]
    fn test_git_diff_no_prefix_without_rename_metadata() {
        // Test git diff --no-prefix format without rename from/to metadata
        let patch = r#"diff --git deleted_file.txt deleted_file.txt
deleted file mode 100644
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        assert_eq!(result[0].original(), Some("deleted_file.txt"));
        assert_eq!(result[0].modified(), Some("deleted_file.txt"));
        assert_eq!(result[0].hunks().len(), 0);
    }

    #[test]
    fn test_git_diff_no_prefix_with_a_in_filename() {
        // Edge case: --no-prefix format with file literally named "a/something.txt"
        // Should NOT strip the a/ since we didn't find b/ prefix
        let patch = r#"diff --git a/file.txt a/file.txt
deleted file mode 100644
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        // In --no-prefix mode, filename is literally "a/file.txt"
        assert_eq!(result[0].original(), Some("a/file.txt"));
        assert_eq!(result[0].modified(), Some("a/file.txt"));
        assert_eq!(result[0].hunks().len(), 0);
    }

    #[test]
    fn test_git_diff_new_file_with_dev_null() {
        // Test creating a new file - git header doesn't actually use /dev/null in the diff --git line
        // but the --- header does
        let patch = r#"diff --git a/new_file.txt b/new_file.txt
new file mode 100644
--- /dev/null
+++ b/new_file.txt
@@ -0,0 +1,3 @@
+line 1
+line 2
+line 3
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        // The --- /dev/null should override the git header
        assert_eq!(result[0].original(), None);
        assert_eq!(result[0].modified(), Some("new_file.txt"));
        assert_eq!(result[0].hunks().len(), 1);
    }

    #[test]
    fn test_git_diff_deleted_file_with_dev_null() {
        // Test deleting a file - git header doesn't actually use /dev/null in the diff --git line
        // but the +++ header does
        let patch = r#"diff --git a/deleted_file.txt b/deleted_file.txt
deleted file mode 100644
--- a/deleted_file.txt
+++ /dev/null
@@ -1,3 +0,0 @@
-line 1
-line 2
-line 3
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        assert_eq!(result[0].original(), Some("deleted_file.txt"));
        // The +++ /dev/null should override the git header
        assert_eq!(result[0].modified(), None);
        assert_eq!(result[0].hunks().len(), 1);
    }

    #[test]
    fn test_git_diff_dev_null_in_git_header() {
        // Test /dev/null in the git diff header itself
        let patch = r#"diff --git /dev/null b/new_file.txt
new file mode 100644
--- /dev/null
+++ b/new_file.txt
@@ -0,0 +1,2 @@
+new content
+here
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        // Both from git header and --- header should recognize /dev/null
        assert_eq!(result[0].original(), None);
        assert_eq!(result[0].modified(), Some("new_file.txt"));
        assert_eq!(result[0].hunks().len(), 1);
    }

    #[test]
    fn test_traditional_unified_diff_no_git_header() {
        // Test traditional unified diff format WITHOUT git header
        // These should NOT have a/ b/ prefixes stripped
        let patch = r#"--- a/old_file.txt
+++ a/new_file.txt
@@ -1,3 +1,3 @@
 line 1
-old line
+new line
 line 3
"#;

        let result = parse_multiple(patch).unwrap();
        assert_eq!(result.len(), 1);

        // Without diff --git, the a/ prefix should be preserved (it's part of the actual filename)
        assert_eq!(result[0].original(), Some("a/old_file.txt"));
        assert_eq!(result[0].modified(), Some("a/new_file.txt"));
        assert_eq!(result[0].hunks().len(), 1);
    }
}
