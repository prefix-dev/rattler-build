//! # rattler_build_package
//!
//! A library for creating conda packages from files and metadata.
//!
//! This crate provides a flexible API for building conda packages either from
//! recipe structures or from manually-constructed metadata. It handles:
//!
//! - File collection and filtering
//! - Metadata generation (about.json, index.json, paths.json, etc.)
//! - Prefix placeholder detection
//! - File transformations (noarch python, symlinks, etc.)
//! - Archive creation (.tar.bz2 and .conda formats)
//!
//! ## Examples
//!
//! ### Building from a recipe
//!
//! ```rust,ignore
//! // This example requires the "recipe" feature and a loaded recipe
//! use rattler_build_package::{PackageBuilder, PackageConfig};
//! use std::path::Path;
//!
//! // Assuming you have a loaded recipe from rattler_build_recipe
//! let config = PackageConfig::default();
//!
//! let output = PackageBuilder::from_recipe(&recipe, config)
//!     .with_files_from_dir(Path::new("/path/to/files"))?
//!     .build(Path::new("/output/dir"))?;
//!
//! println!("Package created at: {}", output.path.display());
//! ```
//!
//! ### Building from metadata
//!
//! ```rust,no_run
//! use rattler_build_package::{PackageBuilder, PackageConfig};
//! use rattler_conda_types::{PackageName, Platform};
//! use std::path::Path;
//! use std::str::FromStr;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let name = PackageName::new_unchecked("mypackage");
//! let version = "1.0.0".parse()?;
//! let platform = Platform::Linux64;
//! let config = PackageConfig::default();
//!
//! let output = PackageBuilder::new(name, version, platform, config)
//!     .with_files_from_dir(Path::new("/path/to/files"))?
//!     .build(Path::new("/output/dir"))?;
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

pub mod builder;
pub mod error;
pub mod files;
pub mod metadata;

mod archiver;
mod prefix;

// Re-export main types
pub use builder::{PackageBuilder, PackageConfig, PackageOutput};
pub use error::{PackageError, Result};
pub use files::{FileCollector, FileEntry};
pub use metadata::{AboutJsonBuilder, IndexJsonBuilder, PathsJsonBuilder};

/// Archive type for the package
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    /// .tar.bz2 format (legacy)
    TarBz2,
    /// .conda format (modern, preferred)
    #[default]
    Conda,
}

impl ArchiveType {
    /// Get the file extension for this archive type
    pub fn extension(&self) -> &'static str {
        match self {
            ArchiveType::TarBz2 => ".tar.bz2",
            ArchiveType::Conda => ".conda",
        }
    }
}
