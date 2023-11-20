use std::{
    fs,
    path::{Path, StripPrefixError},
    process::Command,
};

use crate::recipe::parser::Source;

pub mod copy_dir;
pub mod host_git_source;
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

    #[error("File not found!")]
    FileNotFound,

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

    #[error("Failed to run git command: {0}")]
    GitError(String),

    #[error("Failed to run git command: {0}")]
    GitErrorStr(&'static str),

    #[error("{0}")]
    UnknownError(String),

    #[error("{0}")]
    UnknownErrorStr(&'static str),

    #[error("Could not walk dir")]
    IgnoreError(#[from] ignore::Error),

    #[error("Failed to parse glob pattern")]
    Glob(#[from] globset::Error),
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
                tracing::info!("Fetching source from git repo: {}", src.url());
                let result = host_git_source::git_src(src, &cache_src, recipe_dir)?;
                let dest_dir = if let Some(folder) = src.folder() {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                crate::source::copy_dir::CopyDir::new(&result, &dest_dir)
                    .use_gitignore(false)
                    .run()?;
                if !src.patches().is_empty() {
                    patch::apply_patches(src.patches(), work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url());
                let res = url_source::url_src(
                    src,
                    &cache_src,
                    src.checksums()
                        .first()
                        .ok_or(SourceError::UnknownErrorStr("Checksums not provided"))?,
                )
                .await?;
                let mut dest_dir = if let Some(folder) = src.folder() {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                const KNOWN_ARCHIVE_EXTENSIONS: [&str; 5] =
                    ["tar", "tar.gz", "tar.xz", "tar.bz2", "zip"];
                if KNOWN_ARCHIVE_EXTENSIONS.iter().any(|ext| {
                    res.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .ends_with(ext)
                }) {
                    extract(&res, &dest_dir)?;
                    tracing::info!("Extracted to {:?}", dest_dir);
                } else {
                    if !dest_dir.exists() {
                        fs::create_dir_all(&dest_dir)?;
                    }
                    if let Some(file_name) = src.file_name() {
                        dest_dir = dest_dir.join(file_name);
                    } else {
                        dest_dir = dest_dir.join(res.file_name().ok_or_else(|| {
                            SourceError::UnknownError(format!(
                                "Failed to get filename for `{}`",
                                res.display()
                            ))
                        })?);
                    }
                    fs::copy(&res, &dest_dir)?;
                    tracing::info!("Downloaded to {:?}", dest_dir);
                }

                if !src.patches().is_empty() {
                    patch::apply_patches(src.patches(), work_dir, recipe_dir)?;
                }
            }
            Source::Path(src) => {
                let src_path = recipe_dir.join(src.path()).canonicalize()?;
                tracing::info!("Copying source from path: {:?}", src_path);

                let dest_dir = if let Some(folder) = src.folder() {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                let _ = copy_dir::CopyDir::new(&src_path, &dest_dir)
                    .use_gitignore(true)
                    .run()?;

                if !src.patches().is_empty() {
                    patch::apply_patches(src.patches(), work_dir, recipe_dir)?;
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
