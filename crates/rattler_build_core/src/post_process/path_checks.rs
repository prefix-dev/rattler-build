//! Path validation checks for conda packages.
//!
//! This module provides functionality to check paths in packages for common issues:
//! - Non-ASCII characters
//! - Spaces in paths
//! - Path length exceeding limits
//! - Case-insensitive collisions
//! - Mixed file extensions for the same file type

// Allow unused code since these functions provide a reusable API for path validation
// that may be used in future features or by external consumers.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;

use rattler_conda_types::package::PathsEntry;

/// Trait for types that can record warnings during package building.
pub trait WarningRecorder {
    /// Record a warning message.
    fn record_warning(&self, warning: &str);
}

const FILE_EXTENSION_GROUPS: &[&[&str]] = &[
    &[".cc", ".CC", ".cpp", ".CPP"],
    &[".htm", ".HTM", ".html", ".HTML"],
    &[".jpg", ".JPG", ".jpeg", ".JPEG"],
    &[".jsonl", ".JSONL", ".ndjson", ".NDJSON"],
    &[".txt", ".TXT", ".text", ".TEXT"],
    &[".yaml", ".YAML", ".yml", ".YML"],
];

/// Perform path validation checks on the given paths and emit warnings.
///
/// This function checks for:
/// - Non-ASCII characters in paths
/// - Spaces in paths
/// - Paths exceeding 200 characters
/// - Case-insensitive collisions
/// - Mixed file extensions for the same file type
pub fn perform_path_checks(output: &dyn WarningRecorder, paths_entries: &[PathsEntry]) {
    // Convert PathsEntry to paths for easier processing
    let all_paths: Vec<&Path> = paths_entries
        .iter()
        .map(|entry| entry.relative_path.as_path())
        .collect();

    // Check for non-ASCII characters
    check_non_ascii_characters(&all_paths, output);

    // Check for spaces in paths
    check_spaces_in_paths(&all_paths, output);

    // Check path length (default limit: 200 characters)
    check_path_length(&all_paths, 200, output);

    // Check for case-insensitive collisions
    check_case_collisions(&all_paths, output);

    // Check for mixed file extensions
    check_mixed_file_extensions(&all_paths, output);
}

fn check_non_ascii_characters(paths: &[&Path], output: &dyn WarningRecorder) {
    for path in paths {
        if let Some(path_str) = path.to_str()
            && !path_str.is_ascii()
        {
            output.record_warning(&format!(
                "Path contains non-ASCII characters: '{}'",
                path_str
            ));
        }
    }
}

fn check_spaces_in_paths(paths: &[&Path], output: &dyn WarningRecorder) {
    for path in paths {
        if let Some(path_str) = path.to_str()
            && path_str.contains(' ')
        {
            output.record_warning(&format!("Path contains spaces: '{}'", path_str));
        }
    }
}

fn check_path_length(paths: &[&Path], max_length: usize, output: &dyn WarningRecorder) {
    for path in paths {
        if let Some(path_str) = path.to_str() {
            let length = path_str.len();
            if length > max_length {
                output.record_warning(&format!(
                    "Path is very long ({} > {}): '{}'",
                    length, max_length, path_str
                ));
            }
        }
    }
}

fn check_case_collisions(paths: &[&Path], output: &dyn WarningRecorder) {
    let mut path_lower_to_original: HashMap<String, Vec<String>> = HashMap::new();

    for path in paths {
        if let Some(path_str) = path.to_str() {
            let lower = path_str.to_lowercase();
            path_lower_to_original
                .entry(lower)
                .or_default()
                .push(path_str.to_string());
        }
    }

    for (_, originals) in path_lower_to_original {
        if originals.len() > 1 {
            let files_str = originals.join(", ");
            output.record_warning(&format!(
                "Found files which differ only by case: {}",
                files_str
            ));
        }
    }
}

fn check_mixed_file_extensions(paths: &[&Path], output: &dyn WarningRecorder) {
    let mut extension_counts: HashMap<String, usize> = HashMap::new();

    // Count occurrences of each extension
    for path in paths {
        if let Some(ext) = path.extension()
            && let Some(ext_str) = ext.to_str()
        {
            let ext_with_dot = format!(".{}", ext_str);
            *extension_counts.entry(ext_with_dot).or_insert(0) += 1;
        }
    }

    // Check each group of related extensions
    for group in FILE_EXTENSION_GROUPS {
        let mut found_extensions = Vec::new();
        for ext in *group {
            if extension_counts.contains_key(*ext) {
                found_extensions.push(*ext);
            }
        }

        if found_extensions.len() >= 2 {
            let extensions_str = found_extensions
                .iter()
                .map(|ext| format!("{} ({})", ext, extension_counts.get(*ext).unwrap_or(&0)))
                .collect::<Vec<_>>()
                .join(", ");

            output.record_warning(&format!(
                "Found a mix of file extensions for the same file type: {}",
                extensions_str
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::package::{PathType, PathsEntry};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // Mock struct to capture warnings
    #[derive(Default)]
    struct MockOutput {
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl MockOutput {
        fn new() -> Self {
            Self::default()
        }

        fn warnings(&self) -> Vec<String> {
            self.warnings.lock().unwrap().clone()
        }
    }

    impl WarningRecorder for MockOutput {
        fn record_warning(&self, warning: &str) {
            self.warnings.lock().unwrap().push(warning.to_string());
        }
    }

    fn create_test_entry(path: &str, path_type: PathType) -> PathsEntry {
        PathsEntry {
            relative_path: PathBuf::from(path),
            path_type,
            sha256: None,
            prefix_placeholder: None,
            no_link: false,
            size_in_bytes: None,
        }
    }

    #[test]
    fn test_non_ascii_characters() {
        let entries = vec![
            create_test_entry("normal/file.txt", PathType::HardLink),
            create_test_entry("café/file.txt", PathType::HardLink),
            create_test_entry("文件/test.py", PathType::HardLink),
        ];

        let output = MockOutput::new();
        perform_path_checks(&output, &entries);

        let warnings = output.warnings();
        assert!(warnings.iter().any(|w| w.contains("café")));
        assert!(warnings.iter().any(|w| w.contains("文件")));
    }

    #[test]
    fn test_spaces_in_paths() {
        let entries = vec![
            create_test_entry("normal/file.txt", PathType::HardLink),
            create_test_entry("has space/file.txt", PathType::HardLink),
            create_test_entry("another file.txt", PathType::HardLink),
        ];

        let output = MockOutput::new();
        perform_path_checks(&output, &entries);

        let warnings = output.warnings();
        assert_eq!(warnings.iter().filter(|w| w.contains("spaces")).count(), 2);
    }

    #[test]
    fn test_path_too_long() {
        let long_path = "a".repeat(250);
        let entries = vec![
            create_test_entry("normal/file.txt", PathType::HardLink),
            create_test_entry(&long_path, PathType::HardLink),
        ];

        let output = MockOutput::new();
        perform_path_checks(&output, &entries);

        let warnings = output.warnings();
        assert_eq!(
            warnings.iter().filter(|w| w.contains("very long")).count(),
            1
        );
        assert!(warnings.iter().any(|w| w.contains("250 > 200")));
    }

    #[test]
    fn test_case_collisions() {
        let entries = vec![
            create_test_entry("file.txt", PathType::HardLink),
            create_test_entry("File.txt", PathType::HardLink),
            create_test_entry("FILE.TXT", PathType::HardLink),
            create_test_entry("other.py", PathType::HardLink),
        ];

        let output = MockOutput::new();
        perform_path_checks(&output, &entries);

        let warnings = output.warnings();
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.contains("differ only by case"))
                .count(),
            1
        );
    }

    #[test]
    fn test_mixed_extensions() {
        let entries = vec![
            create_test_entry("file.txt", PathType::HardLink),
            create_test_entry("file.TXT", PathType::HardLink),
            create_test_entry("file.text", PathType::HardLink),
            create_test_entry("doc.yaml", PathType::HardLink),
            create_test_entry("doc.yml", PathType::HardLink),
        ];

        let output = MockOutput::new();
        perform_path_checks(&output, &entries);

        let warnings = output.warnings();
        // txt/TXT/text and yaml/yml groups
        assert!(
            warnings
                .iter()
                .filter(|w| w.contains("mix of file extensions"))
                .count()
                >= 2
        );
    }
}
