use std::{
    fs,
    path::{Path, PathBuf, StripPrefixError},
    process::Command,
};

use fs_extra::dir::{copy, create_all, remove, CopyOptions};
use fs_extra::error::ErrorKind::PermissionDenied;

use crate::metadata::Source;

pub mod git_source;
pub mod patch;
pub mod url_source;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to download source from url: {0}")]
    Url(#[from] reqwest::Error),

    #[error("WalkDir Error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("FileSystem error: '{0}'")]
    FileSystemError(fs_extra::error::Error),

    #[error("StripPrefixError Error: {0}")]
    StripPrefixError(#[from] StripPrefixError),

    #[error("Download could not be validated with checksum!")]
    ValidationFailed,

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

    #[error("Failed to run git command: {0}")]
    GitError(#[from] git2::Error),
}

/// Fetches all sources in a list of sources and applies specified patches
pub async fn fetch_sources(
    sources: &[Source],
    work_dir: &Path,
    recipe_dir: &Path,
    cache_dir: &Path,
) -> Result<(), SourceError> {
    let cache_src = cache_dir.join("src_cache");
    fs::create_dir_all(&cache_src)?;

    for src in sources {
        match &src {
            Source::Git(src) => {
                tracing::info!("Fetching source from GIT: {}", src.git_url);
                let result = match git_source::git_src(src, &cache_src, recipe_dir) {
                    Ok(path) => path,
                    Err(e) => return Err(e),
                };
                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                copy_dir(&result, &dest_dir)?;

                if let Some(patches) = &src.patches {
                    patch::apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url);
                let res = url_source::url_src(src, &cache_src, &src.checksum).await?;
                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                extract(&res, &dest_dir)?;
                tracing::info!("Extracted to {:?}", work_dir);

                if let Some(patches) = &src.patches {
                    patch::apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Path(src) => {
                tracing::info!("Copying source from path: {:?}", src.path);
                let src_path = recipe_dir.join(&src.path);

                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                copy_dir(&src_path, &dest_dir)?;

                if let Some(patches) = &src.patches {
                    patch::apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
        }
    }
    Ok(())
}

/// Extracts a tar archive to the specified target directory
fn extract(
    archive: &Path,
    target_directory: &Path,
) -> Result<std::process::Output, std::io::Error> {
    let output = Command::new("tar")
        .arg("-xf")
        .arg(String::from(archive.to_string_lossy()))
        .arg("--preserve-permissions")
        .arg("--strip-components=1")
        .arg("-C")
        .arg(String::from(target_directory.to_string_lossy()))
        .output();

    output
}

fn copy_dir(from: &PathBuf, to: &PathBuf) -> Result<(), SourceError> {
    // Create the to path because we're going to copy the contents only
    create_all(to, true).unwrap();

    // Setup copy options, overwrite if needed, only copy the contents as we want to specify the dir name manually
    let mut options = CopyOptions::new();
    options.overwrite = true;
    options.content_only = true;

    match copy(from, to, &options) {
        Ok(_) => tracing::info!(
            "Copied {} to {}",
            from.to_string_lossy(),
            to.to_string_lossy()
        ),
        // Use matches as the ErrorKind does not support `==`
        Err(e) if matches!(e.kind, PermissionDenied) => {
            tracing::debug!("Permission error in cache, this often happens when the previous run was exited in a faulty way. Removing the cache and retrying the copy.");
            if let Err(remove_error) = remove(to) {
                tracing::error!("Failed to remove cache directory: {}", remove_error);
                return Err(SourceError::FileSystemError(e));
            } else if let Err(retry_error) = copy(from, to, &options) {
                tracing::error!("Failed to retry the copy operation: {}", retry_error);
                return Err(SourceError::FileSystemError(e));
            } else {
                tracing::debug!(
                    "Successfully retried copying {} to {}",
                    from.to_string_lossy(),
                    to.to_string_lossy()
                );
            }
        }
        Err(e) => return Err(SourceError::FileSystemError(e)),
    }
    Ok(())
}
