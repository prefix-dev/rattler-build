//! Error types for variant configuration

use std::path::PathBuf;
use thiserror::Error;

#[cfg(feature = "miette")]
use miette::Diagnostic;

/// Errors that can occur while parsing variant configuration files
#[derive(Debug, Error)]
#[cfg_attr(feature = "miette", derive(Diagnostic))]
pub enum VariantConfigError {
    /// Failed to parse YAML file with detailed span information
    #[error("Could not parse variant config file {}: {source}", path.display())]
    ParseError {
        path: PathBuf,
        #[source]
        source: rattler_build_yaml_parser::ParseError,
    },

    /// Failed to read file from disk
    #[error("Could not open file ({0}): {1}")]
    IoError(PathBuf, #[source] std::io::Error),

    /// Invalid variant configuration structure
    #[error("Invalid variant configuration: {0}")]
    InvalidConfig(String),
}

/// Errors that can occur while expanding variants
#[derive(Debug, Error)]
#[cfg_attr(feature = "miette", derive(Diagnostic))]
pub enum VariantExpandError {
    /// Zip key elements have mismatched lengths
    #[error("Zip key elements do not all have same length: {0}")]
    InvalidZipKeyLength(String),

    /// zip_keys must be a list of lists, not a flat list
    #[error("zip_keys must be a list of lists, not a flat list")]
    InvalidZipKeyStructure,

    /// Variant key not found in configuration
    #[error("Variant key '{0}' not found in configuration")]
    MissingVariantKey(String),

    /// Missing output in recipe
    #[error("Missing output: {0}")]
    MissingOutput(String),

    /// Cycle detected in recipe outputs
    #[error("Cycle detected in recipe outputs: {0}")]
    CycleInRecipeOutputs(String),
}

/// Combined error type for variant operations
#[derive(Debug, Error)]
#[cfg_attr(feature = "miette", derive(Diagnostic))]
pub enum VariantError {
    /// Configuration error
    #[error(transparent)]
    #[cfg_attr(feature = "miette", diagnostic(transparent))]
    Config(#[from] VariantConfigError),

    /// Expansion error
    #[error(transparent)]
    Expand(#[from] VariantExpandError),

    /// Recipe parsing errors during variant expansion
    #[error("Failed to parse recipe during variant expansion")]
    RecipeParseErrors(#[source] Box<dyn std::error::Error + Send + Sync>),
}
