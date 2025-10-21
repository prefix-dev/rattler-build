use crate::{
    line_end::LineEnd,
    patch::{Diff, Hunk, Line},
    utils::{LineIter, Text},
};
use std::{fmt, iter};

/// An error returned when [`apply`]ing a `Patch` fails
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApplyError(usize, String);

impl fmt::Debug for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ApplyError")
            .field(&self.0)
            .field(&self.1)
            .finish()
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error applying hunk #{}: {}", self.0, self.1)
    }
}

impl std::error::Error for ApplyError {}

/// Configuration for patch application
#[derive(Default, Debug, Clone)]
pub struct ApplyConfig {
    /// Configuration of line end handling
    pub line_end_strategy: LineEndHandling,
    /// Configuration of fuzzy matching
    pub fuzzy_config: FuzzyConfig,
}

// TODO: Add option to keep previous behaviour.
/// Configuration of line end handling
#[derive(Debug, Clone, Default)]
pub enum LineEndHandling {
    /// Replace matched line ending with line ending from patch file if they don't match.
    ///
    /// This is almost like default behavior before, except that we assume uniform line ending.
    ///
    /// Line ending cases in pseudocode:
    ///
    /// ```compile_fail
    /// match (patch_line_ending, file_line) {
    ///     ("\n",   "\n")   => "\n"
    ///     ("\n",   "\r\n") => "\n"
    ///     ("\r\n", "\n")   => "\r\n"
    ///     ("\r\n", "\r\n") => "\r\n"
    /// }
    /// ```
    EnsurePatchLineEnding,
    /// Replace matched line ending with line ending from original file if they don't match.
    ///
    /// Line ending cases in pseudocode:
    ///
    /// ```compile_fail
    /// match (patch_line_ending, file_line) {
    ///     ("\n",   "\n")   => "\n"
    ///     ("\n",   "\r\n") => "\r\n"
    ///     ("\r\n", "\n")   => "\n"
    ///     ("\r\n", "\r\n") => "\r\n"
    /// }
    /// ```
    #[default]
    EnsureFileLineEnding,
    /// Enforce specific line ending.
    ///
    /// Line ending cases in pseudocode:
    ///
    /// ```compile_fail
    /// match (patch_line_ending, file_line) {
    ///     ("\n",   "\n")   => new_line_ending
    ///     ("\n",   "\r\n") => new_line_ending
    ///     ("\r\n", "\n")   => new_line_ending
    ///     ("\r\n", "\r\n") => new_line_ending
    /// }
    /// ```
    EnsureLineEnding(LineEnd),
}

/// Configuration for fuzzy matching behavior
#[derive(Debug, Clone)]
pub struct FuzzyConfig {
    /// Maximum number of context lines that can be ignored (fuzz factor)
    pub max_fuzz: usize,
    /// Whether to allow whitespace-only differences in context lines
    pub ignore_whitespace: bool,
    /// Whether to perform case-insensitive matching
    pub ignore_case: bool,
}

impl Default for FuzzyConfig {
    fn default() -> Self {
        Self {
            max_fuzz: 2,
            ignore_whitespace: false,
            ignore_case: false,
        }
    }
}

// TODO: Ignore line endings in comparison
/// Trait for types that can be compared with fuzzy matching
pub trait FuzzyComparable {
    fn fuzzy_eq(&self, other: &Self, config: &ApplyConfig) -> bool;
    fn similarity(&self, other: &Self, config: &ApplyConfig) -> f32;
}

impl FuzzyComparable for str {
    fn fuzzy_eq(&self, other: &Self, config: &ApplyConfig) -> bool {
        self.similarity(other, config) > 0.8
    }

    fn similarity(&self, other: &Self, config: &ApplyConfig) -> f32 {
        let (s1, s2) = if config.fuzzy_config.ignore_case {
            (self.to_lowercase(), other.to_lowercase())
        } else {
            (self.to_string(), other.to_string())
        };

        let (s1, s2) = if config.fuzzy_config.ignore_whitespace {
            (
                s1.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>(),
                s2.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>(),
            )
        } else {
            (s1, s2)
        };

        if s1 == s2 {
            return 1.0;
        }

        // Use strsim's Levenshtein distance implementation
        let max_len = s1.len().max(s2.len());
        if max_len == 0 {
            return 1.0;
        }

        let distance = strsim::levenshtein(&s1, &s2);
        1.0 - (distance as f32 / max_len as f32)
    }
}

impl FuzzyComparable for [u8] {
    fn fuzzy_eq(&self, other: &Self, config: &ApplyConfig) -> bool {
        // Try to convert to UTF-8 strings for better comparison
        if let (Ok(s1), Ok(s2)) = (std::str::from_utf8(self), std::str::from_utf8(other)) {
            s1.fuzzy_eq(s2, config)
        } else {
            // Fall back to exact byte comparison
            self == other
        }
    }

    fn similarity(&self, other: &Self, config: &ApplyConfig) -> f32 {
        // Try to convert to UTF-8 strings for better comparison
        if let (Ok(s1), Ok(s2)) = (std::str::from_utf8(self), std::str::from_utf8(other)) {
            s1.similarity(s2, config)
        } else {
            // Fall back to exact byte comparison
            if self == other { 1.0 } else { 0.0 }
        }
    }
}

#[derive(Debug)]
enum ImageLine<'a, T: ?Sized> {
    Unpatched((&'a T, Option<LineEnd>)),
    Patched((&'a T, Option<LineEnd>)),
}

impl<'a, T: ?Sized + Text> ImageLine<'a, T> {
    fn inner(&self) -> (&T, Option<LineEnd>) {
        match self {
            ImageLine::Unpatched(inner) | ImageLine::Patched(inner) => *inner,
        }
    }

    fn into_inner(self) -> (&'a T, Option<LineEnd>) {
        match self {
            ImageLine::Unpatched(inner) | ImageLine::Patched(inner) => inner,
        }
    }

    fn is_patched(&self) -> bool {
        match self {
            ImageLine::Unpatched(_) => false,
            ImageLine::Patched(_) => true,
        }
    }
}

impl<T: ?Sized> Copy for ImageLine<'_, T> {}

impl<T: ?Sized> Clone for ImageLine<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

fn map_line_ending<T>(line_end: Option<LineEnd>, ensure_line_end: Option<LineEnd>) -> T
where
    T: From<LineEnd> + Default,
{
    let Some(line_end) = line_end else {
        return Default::default();
    };

    if let Some(ensure_line_end) = ensure_line_end {
        ensure_line_end.into()
    } else {
        line_end.into()
    }
}

/// Apply a `Diff` to a base image with default fuzzy matching
pub fn apply(base_image: &str, diff: &Diff<'_, str>) -> Result<String, ApplyError> {
    apply_with_config(base_image, diff, &ApplyConfig::default())
}

/// Apply a `Diff` to a base image with custom fuzzy matching configuration
pub fn apply_with_config(
    base_image: &str,
    diff: &Diff<'_, str>,
    config: &ApplyConfig,
) -> Result<String, ApplyError> {
    let mut image: Vec<_> = LineIter::new(base_image)
        .map(ImageLine::Unpatched)
        .collect();

    for (i, hunk) in diff.hunks().iter().enumerate() {
        apply_hunk_with_config(&mut image, hunk, config)
            .map_err(|_| ApplyError(i + 1, format!("{:#?}", hunk)))?;
    }

    // TODO: Keep line ending as is like it was before.
    let preferred_line_ending = Some(match config.line_end_strategy {
        LineEndHandling::EnsurePatchLineEnding => {
            let mut lf_score = 0usize;
            let mut crlf_score = 0usize;

            for hunk in diff.hunks().iter() {
                for line in hunk.lines() {
                    match line.line_end() {
                        Some(LineEnd::Lf) => lf_score += 1,
                        Some(LineEnd::CrLf) => crlf_score += 1,
                        _ => (),
                    }
                }
            }

            LineEnd::choose_from_scores(lf_score, crlf_score)
        }
        LineEndHandling::EnsureFileLineEnding => LineEnd::most_common(base_image),
        LineEndHandling::EnsureLineEnding(line_end) => line_end,
    });

    Ok(image
        .into_iter()
        .map(ImageLine::into_inner)
        .map(|(line, ending)| {
            format!(
                "{}{}",
                line,
                map_line_ending::<&str>(ending, preferred_line_ending)
            )
        })
        .collect())
}

/// Apply a non-utf8 `Diff` to a base image with default fuzzy matching
pub fn apply_bytes(base_image: &[u8], patch: &Diff<'_, [u8]>) -> Result<Vec<u8>, ApplyError> {
    apply_bytes_with_config(base_image, patch, &ApplyConfig::default())
}

/// Apply a non-utf8 `Diff` to a base image with custom fuzzy matching configuration
pub fn apply_bytes_with_config(
    base_image: &[u8],
    diff: &Diff<'_, [u8]>,
    config: &ApplyConfig,
) -> Result<Vec<u8>, ApplyError> {
    let mut image: Vec<_> = LineIter::new(base_image)
        .map(ImageLine::Unpatched)
        .collect();

    for (i, hunk) in diff.hunks().iter().enumerate() {
        apply_hunk_with_config(&mut image, hunk, config)
            .map_err(|_| ApplyError(i + 1, format!("{:#?}", hunk)))?;
    }

    // TODO: Keep line ending as is like it was before.
    let preferred_line_ending = Some(match config.line_end_strategy {
        LineEndHandling::EnsurePatchLineEnding => {
            let mut lf_score = 0usize;
            let mut crlf_score = 0usize;

            for hunk in diff.hunks().iter() {
                for line in hunk.lines() {
                    match line.line_end() {
                        Some(LineEnd::Lf) => lf_score += 1,
                        Some(LineEnd::CrLf) => crlf_score += 1,
                        _ => (),
                    }
                }
            }

            LineEnd::choose_from_scores(lf_score, crlf_score)
        }
        LineEndHandling::EnsureFileLineEnding => LineEnd::most_common(base_image),
        LineEndHandling::EnsureLineEnding(line_end) => line_end,
    });

    Ok(image
        .into_iter()
        .map(ImageLine::into_inner)
        .flat_map(|(line, ending)| {
            [
                line,
                map_line_ending::<&[u8]>(ending, preferred_line_ending),
            ]
            .concat()
        })
        .collect())
}

fn apply_hunk_with_config<'a, T>(
    image: &mut Vec<ImageLine<'a, T>>,
    hunk: &Hunk<'a, T>,
    config: &ApplyConfig,
) -> Result<(), ()>
where
    T: PartialEq + FuzzyComparable + ?Sized + Text + ToOwned,
{
    // Find position with fuzzy matching
    let (pos, fuzz_level) = find_position_fuzzy(image, hunk, config).ok_or(())?;

    // update image
    if fuzz_level == 0 {
        // Exact match - replace all lines as before
        image.splice(
            pos..pos + pre_image_line_count(hunk.lines()),
            post_image(hunk.lines()).map(ImageLine::Patched),
        );
    } else {
        // Fuzzy match - preserve original context lines, only apply insertions/deletions
        apply_hunk_preserving_context(image, hunk, pos);
    }

    Ok(())
}

/// Apply hunk while preserving original context lines (for fuzzy matching)
fn apply_hunk_preserving_context<'a, T>(
    image: &mut Vec<ImageLine<'a, T>>,
    hunk: &Hunk<'a, T>,
    pos: usize,
) where
    T: ?Sized + Text + ToOwned,
{
    let mut image_offset = 0;

    for line in hunk.lines() {
        match *line {
            Line::Context(_) => {
                // Keep the original context line, just mark it as patched
                if let Some(img_line) = image.get_mut(pos + image_offset) {
                    *img_line = ImageLine::Patched(img_line.into_inner());
                }
                image_offset += 1;
            }
            Line::Delete(_) => {
                // Remove the line
                image.remove(pos + image_offset);
            }
            Line::Insert(line) => {
                // Insert the new line
                image.insert(pos + image_offset, ImageLine::Patched(line));
                image_offset += 1;
            }
        }
    }
}

/// Search in `image` for a place to apply hunk with fuzzy matching support
fn find_position_fuzzy<T>(
    image: &[ImageLine<T>],
    hunk: &Hunk<'_, T>,
    config: &ApplyConfig,
) -> Option<(usize, usize)>
where
    T: PartialEq + FuzzyComparable + ?Sized + Text + ToOwned,
{
    // Try exact match first (fuzz level 0)
    if let Some(pos) = find_position(image, hunk) {
        return Some((pos, 0));
    }

    // Try fuzzy matching with increasing fuzz levels
    for fuzz_level in 1..=config.fuzzy_config.max_fuzz {
        if let Some(pos) = find_position_with_fuzz(image, hunk, fuzz_level, config) {
            return Some((pos, fuzz_level));
        }
    }

    None
}

/// Find position with specified fuzz level
fn find_position_with_fuzz<T>(
    image: &[ImageLine<T>],
    hunk: &Hunk<'_, T>,
    fuzz_level: usize,
    config: &ApplyConfig,
) -> Option<usize>
where
    T: PartialEq + FuzzyComparable + ?Sized + Text + ToOwned,
{
    let pos = std::cmp::min(hunk.new_range().start().saturating_sub(1), image.len());

    let backward = (0..pos).rev();
    let forward = pos + 1..image.len();

    iter::once(pos)
        .chain(interleave(backward, forward))
        .find(|&pos| match_fragment_fuzzy(image, hunk.lines(), pos, fuzz_level, config))
}

/// Match fragment with fuzzy context matching
fn match_fragment_fuzzy<T>(
    image: &[ImageLine<T>],
    lines: &[Line<'_, T>],
    pos: usize,
    fuzz_level: usize,
    config: &ApplyConfig,
) -> bool
where
    T: PartialEq + FuzzyComparable + ?Sized + Text,
{
    let len = pre_image_line_count(lines);

    let image_slice = if let Some(image) = image.get(pos..pos + len) {
        image
    } else {
        return false;
    };

    // If any of these lines have already been patched then we can't match at this position
    if image_slice.iter().any(ImageLine::is_patched) {
        return false;
    }

    let pre_image_lines: Vec<_> = pre_image(lines).collect();
    let image_lines: Vec<_> = image_slice.iter().map(ImageLine::inner).collect();

    if pre_image_lines.len() != image_lines.len() {
        return false;
    }

    // Get context line indices from the original lines
    let context_indices: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| match line {
            Line::Context(_) => Some(i),
            _ => None,
        })
        .collect();

    // Map context indices to pre_image indices
    let mut pre_image_context_indices = Vec::new();
    let mut pre_image_idx = 0;
    for (original_idx, line) in lines.iter().enumerate() {
        match line {
            Line::Context(_) | Line::Delete(_) => {
                if context_indices.contains(&original_idx) {
                    pre_image_context_indices.push(pre_image_idx);
                }
                pre_image_idx += 1;
            }
            Line::Insert(_) => {}
        }
    }

    // NOTE: Temporary (?) fix mostly for line endings.
    // If we don't have enough context lines to fuzz, fall back to exact matching, but still check for string similarity.
    if pre_image_context_indices.len() < fuzz_level {
        let len = pre_image_line_count(lines);

        let image = if let Some(image) = image.get(pos..pos + len) {
            image
        } else {
            return false;
        };

        // If any of these lines have already been patched then we can't match at this position
        if image.iter().any(ImageLine::is_patched) {
            return false;
        }

        for (pre_line, image_line) in pre_image_lines.iter().zip(image_lines.iter()) {
            if !pre_line.0.fuzzy_eq(image_line.0, config) {
                return false;
            }
        }

        return true;
    }

    // Try different combinations of ignoring context lines
    let combinations = generate_fuzz_combinations(&pre_image_context_indices, fuzz_level);

    for ignored_indices in combinations {
        if match_with_ignored_context(
            pre_image_lines.as_slice(),
            &image_lines,
            &ignored_indices,
            config,
        ) {
            return true;
        }
    }

    false
}

/// Generate combinations of context line indices to ignore
fn generate_fuzz_combinations(context_indices: &[usize], fuzz_level: usize) -> Vec<Vec<usize>> {
    if fuzz_level == 0 || context_indices.is_empty() {
        return vec![vec![]];
    }

    let mut combinations = Vec::new();

    // Generate all combinations of size up to fuzz_level
    for size in 0..=fuzz_level.min(context_indices.len()) {
        combinations.extend(combinations_of_size(context_indices, size));
    }

    combinations
}

/// Generate all combinations of a specific size
fn combinations_of_size(items: &[usize], size: usize) -> Vec<Vec<usize>> {
    if size == 0 {
        return vec![vec![]];
    }
    if size > items.len() {
        return vec![];
    }

    let mut result = Vec::new();
    for i in 0..=items.len() - size {
        let first = items[i];
        for mut rest in combinations_of_size(&items[i + 1..], size - 1) {
            rest.insert(0, first);
            result.push(rest);
        }
    }
    result
}

/// Match lines while ignoring specified context line indices
fn match_with_ignored_context<T>(
    pre_image_lines: &[(&T, Option<LineEnd>)],
    image_lines: &[(&T, Option<LineEnd>)],
    ignored_indices: &[usize],
    config: &ApplyConfig,
) -> bool
where
    T: PartialEq + FuzzyComparable + ?Sized,
{
    for (i, (pre_line, image_line)) in pre_image_lines.iter().zip(image_lines.iter()).enumerate() {
        if ignored_indices.contains(&i) {
            continue; // Skip this context line
        }

        // Require high similarity for non-ignored lines
        if !pre_line.0.fuzzy_eq(image_line.0, config) {
            return false;
        }
    }
    true
}

// Search in `image` for a place to apply hunk.
// This follows the general algorithm (minus fuzzy-matching context lines) described in GNU patch's
// man page.
//
// It might be worth looking into other possible positions to apply the hunk to as described here:
// https://neil.fraser.name/writing/patch/
fn find_position<T: PartialEq + ?Sized + Text + ToOwned>(
    image: &[ImageLine<T>],
    hunk: &Hunk<'_, T>,
) -> Option<usize> {
    // In order to avoid searching through positions which are out of bounds of the image,
    // clamp the starting position based on the length of the image
    let pos = std::cmp::min(hunk.new_range().start().saturating_sub(1), image.len());

    // Create an iterator that starts with 'pos' and then interleaves
    // moving pos backward/foward by one.
    let backward = (0..pos).rev();
    let forward = pos + 1..image.len();

    iter::once(pos)
        .chain(interleave(backward, forward))
        .find(|&pos| match_fragment(image, hunk.lines(), pos))
}

fn pre_image_line_count<T: ?Sized>(lines: &[Line<'_, T>]) -> usize {
    pre_image(lines).count()
}

fn post_image<'a, 'b, T: ?Sized>(
    lines: &'b [Line<'a, T>],
) -> impl Iterator<Item = (&'a T, Option<LineEnd>)> + 'b {
    lines.iter().filter_map(move |line| match *line {
        Line::Context(l) | Line::Insert(l) => Some(l),
        Line::Delete(_) => None,
    })
}

fn pre_image<'a, 'b: 'a, T: ?Sized>(
    lines: &'b [Line<'a, T>],
) -> impl Iterator<Item = (&'a T, Option<LineEnd>)> + 'b {
    lines.iter().filter_map(|line| match *line {
        Line::Context(l) | Line::Delete(l) => Some(l),
        Line::Insert(_) => None,
    })
}

fn match_fragment<T: PartialEq + ?Sized + Text>(
    image: &[ImageLine<T>],
    lines: &[Line<'_, T>],
    pos: usize,
) -> bool {
    let len = pre_image_line_count(lines);

    let image = if let Some(image) = image.get(pos..pos + len) {
        image
    } else {
        return false;
    };

    // If any of these lines have already been patched then we can't match at this position
    if image.iter().any(ImageLine::is_patched) {
        return false;
    }

    pre_image(lines).eq(image.iter().map(ImageLine::inner))
}

#[derive(Debug)]
struct Interleave<I, J> {
    a: iter::Fuse<I>,
    b: iter::Fuse<J>,
    flag: bool,
}

fn interleave<I, J>(
    i: I,
    j: J,
) -> Interleave<<I as IntoIterator>::IntoIter, <J as IntoIterator>::IntoIter>
where
    I: IntoIterator,
    J: IntoIterator<Item = I::Item>,
{
    Interleave {
        a: i.into_iter().fuse(),
        b: j.into_iter().fuse(),
        flag: false,
    }
}

impl<I, J> Iterator for Interleave<I, J>
where
    I: Iterator,
    J: Iterator<Item = I::Item>,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        self.flag = !self.flag;
        if self.flag {
            match self.a.next() {
                None => self.b.next(),
                item => item,
            }
        } else {
            match self.b.next() {
                None => self.a.next(),
                item => item,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use crate::{Diff, apply};

    fn load_files(name: &str) -> (String, String) {
        let base_folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-data")
            .join(name);

        let base_image = std::fs::read_to_string(base_folder.join("target.txt")).unwrap();
        let patch = std::fs::read_to_string(base_folder.join("patch.patch")).unwrap();
        (base_image, patch)
    }

    #[test]
    fn apply_patch() {
        let (base_image, patch) = load_files("fuzzy");
        let patch = crate::Diff::from_bytes(patch.as_bytes()).unwrap();

        println!("Applied: {:#?}", patch);
        let result = crate::apply_bytes(base_image.as_bytes(), &patch).unwrap();
        // take the first 50 lines for snapshot testing
        let result = String::from_utf8(result)
            .unwrap()
            .lines()
            .take(50)
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(result);
        println!("Result:\n{}", result);
    }

    fn assert_patch(old: &str, new: &str, patch: &str) {
        let diff = Diff::from_str(patch).unwrap();
        assert_eq!(Ok(new.to_string()), apply(old, &diff));
    }

    #[test]
    fn line_end_strategies() {
        let old = "old line\r\n";
        let new = "new line\r\n";
        let patch = "\
--- original
+++ modified
@@ -1 +1 @@
-old line
+new line
";
        assert_patch(old, new, patch);

        let old = "old line\n";
        let new = "new line\n";
        let expected = "\
--- original
+++ modified
@@ -1 +1 @@
-old line
+new line
"
        .replace("\n", "\r\n");
        assert_patch(old, new, expected.as_str());
    }
}
