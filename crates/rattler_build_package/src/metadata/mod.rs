//! Metadata generation for conda packages

mod about;
mod index;
mod paths;

pub use about::AboutJsonBuilder;
pub use index::IndexJsonBuilder;
pub use paths::PathsJsonBuilder;

/// Prefix detection configuration
#[derive(Debug, Clone)]
pub struct PrefixDetectionConfig {
    /// Whether to detect prefix in binary files
    pub detect_binary: bool,

    /// Whether to detect prefix in text files
    pub detect_text: bool,

    /// Glob patterns to ignore for prefix detection
    pub ignore_patterns: Vec<String>,
}

impl Default for PrefixDetectionConfig {
    fn default() -> Self {
        Self {
            detect_binary: true,
            detect_text: true,
            ignore_patterns: Vec::new(),
        }
    }
}
