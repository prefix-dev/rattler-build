//! File collection and transformation for package creation

use content_inspector::ContentType;
use fs_err as fs;
use std::path::{Path, PathBuf};

use crate::Result;

mod collector;
mod content_type;
mod transformer;

pub use collector::FileCollector;
pub use content_type::detect_content_type;
pub use transformer::FileTransformer;

/// Represents a file to be included in the package
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Source path on disk
    pub source: PathBuf,

    /// Destination path within the package (relative)
    pub destination: PathBuf,

    /// Content type of the file
    pub content_type: Option<ContentType>,

    /// Whether this is a symlink
    pub is_symlink: bool,

    /// Symlink target (if this is a symlink)
    pub symlink_target: Option<PathBuf>,
}

impl FileEntry {
    /// Create a FileEntry from source and destination paths
    pub fn from_paths(source: &Path, dest: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(source)?;
        let is_symlink = metadata.is_symlink();

        let symlink_target = if is_symlink {
            Some(fs::read_link(source)?)
        } else {
            None
        };

        let content_type = if !is_symlink && metadata.is_file() {
            Some(detect_content_type(source)?)
        } else {
            None
        };

        Ok(Self {
            source: source.to_path_buf(),
            destination: dest.to_path_buf(),
            content_type,
            is_symlink,
            symlink_target,
        })
    }

    /// Create a FileEntry with all fields specified
    pub fn new(
        source: PathBuf,
        destination: PathBuf,
        content_type: Option<ContentType>,
        is_symlink: bool,
        symlink_target: Option<PathBuf>,
    ) -> Self {
        Self {
            source,
            destination,
            content_type,
            is_symlink,
            symlink_target,
        }
    }
}
