//! Error types for the rattler_build_package crate

use std::path::PathBuf;

/// Result type alias using PackageError
pub type Result<T> = std::result::Result<T, PackageError>;

/// Errors that can occur during package creation
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to serialize JSON
    #[error("Failed to serialize JSON: {0}")]
    JsonSerialization(#[from] serde_json::Error),

    /// Failed to build glob pattern
    #[error("Failed to build glob pattern: {0}")]
    Glob(#[from] globset::Error),

    /// Failed to strip prefix from path
    #[error("Could not strip prefix from path: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    /// Build string is not set
    #[error("Build string is not set")]
    BuildStringNotSet,

    /// Dependencies are not finalized
    #[error("Dependencies are not yet finalized/resolved")]
    DependenciesNotFinalized,

    /// File contains mixed prefix placeholders (forward and backslashes)
    #[error("Found mixed prefix placeholders in file: {0}")]
    MixedPrefixPlaceholders(PathBuf),

    /// Content type could not be determined for file
    #[error("Failed to determine content type for file: {0}")]
    ContentTypeNotFound(PathBuf),

    /// License files were not found
    #[error("No license files were copied")]
    LicensesNotFound,

    /// Invalid metadata
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(String),

    /// WalkDir error
    #[error("Failed to walk directory: {0}")]
    WalkDir(#[from] walkdir::Error),

    /// Version parsing error
    #[error("Failed to parse version: {0}")]
    VersionParse(#[from] rattler_conda_types::ParseVersionError),

    /// Package name parsing error
    #[error("Failed to parse package name: {0}")]
    PackageNameParse(#[from] rattler_conda_types::InvalidPackageNameError),

    /// Path normalization error
    #[error("Path normalization error: {0}")]
    PathNormalization(String),

    /// Required field is missing
    #[error("Required field '{0}' is missing")]
    MissingField(String),

    /// Package streaming error
    #[error("Package streaming error: {0}")]
    PackageStreaming(String),

    /// Archive creation error
    #[error("Failed to create archive: {0}")]
    ArchiveCreation(String),
}
