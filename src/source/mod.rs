//! Module for fetching sources and applying patches

use std::{
    ffi::OsStr,
    path::{PathBuf, StripPrefixError},
};

use crate::{
    metadata::{Directories, Output},
    recipe::parser::{GitRev, GitSource, Source},
    source::{
        checksum::Checksum,
        extract::{extract_tar, extract_zip, is_tarball},
    },
    system_tools::ToolError,
    tool_configuration,
};

use fs_err as fs;

use crate::system_tools::SystemTools;
pub mod checksum;
pub mod copy_dir;
pub mod extract;
pub mod git_source;
pub mod patch;
pub mod url_source;

#[allow(missing_docs)]
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to download source from url: {0}")]
    Url(#[from] reqwest::Error),

    #[error("Url does not point to a file: {0}")]
    UrlNotFile(url::Url),

    #[error("WalkDir Error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("FileSystem error: '{0}'")]
    FileSystemError(std::io::Error),

    #[error("StripPrefixError Error: {0}")]
    StripPrefixError(#[from] StripPrefixError),

    #[error("Download could not be validated with checksum!")]
    ValidationFailed,

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Could not find `patch` executable")]
    PatchExeNotFound,

    #[error("Patch file not found: {0}")]
    PatchNotFound(PathBuf),

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

    #[error("Failed to extract archive: {0}")]
    TarExtractionError(String),

    #[error("Failed to extract zip archive: {0}")]
    ZipExtractionError(String),

    #[error("Failed to read from zip: {0}")]
    InvalidZip(String),

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

    #[error("No checksum found for url: {0}")]
    NoChecksum(String),

    #[error("Failed to find git executable: {0}")]
    GitNotFound(#[from] ToolError),
}

/// Fetches all sources in a list of sources and applies specified patches
pub async fn fetch_sources(
    sources: &[Source],
    directories: &Directories,
    system_tools: &SystemTools,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<Vec<Source>, SourceError> {
    if sources.is_empty() {
        tracing::info!("No sources to fetch");
        return Ok(Vec::new());
    }

    // Figure out the directories we need
    let work_dir = &directories.work_dir;
    let recipe_dir = &directories.recipe_dir;
    let cache_src = directories.output_dir.join("src_cache");
    fs::create_dir_all(&cache_src)?;

    let mut rendered_sources = Vec::new();

    for src in sources {
        match &src {
            Source::Git(src) => {
                tracing::info!("Fetching source from git repo: {}", src.url());
                let result = git_source::git_src(system_tools, src, &cache_src, recipe_dir)?;
                let dest_dir = if let Some(target_directory) = src.target_directory() {
                    work_dir.join(target_directory)
                } else {
                    work_dir.to_path_buf()
                };

                rendered_sources.push(Source::Git(GitSource {
                    rev: GitRev::Commit(result.1),
                    ..src.clone()
                }));

                let copy_result = tool_configuration.fancy_log_handler.wrap_in_progress(
                    "copying source into isolated environment",
                    || {
                        copy_dir::CopyDir::new(&result.0, &dest_dir)
                            .use_gitignore(false)
                            .run()
                    },
                )?;
                tracing::info!(
                    "Copied {} files into isolated environment",
                    copy_result.copied_paths().len()
                );

                if !src.patches().is_empty() {
                    patch::apply_patches(system_tools, src.patches(), &dest_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                let first_url = src.urls().first().expect("we should have at least one URL");
                let file_name_from_url = first_url
                    .path_segments()
                    .and_then(|segments| segments.last().map(|last| last.to_string()))
                    .ok_or_else(|| SourceError::UrlNotFile(first_url.clone()))?;

                let res = url_source::url_src(src, &cache_src, tool_configuration).await?;

                let dest_dir = if let Some(target_directory) = src.target_directory() {
                    work_dir.join(target_directory)
                } else {
                    work_dir.to_path_buf()
                };

                // Create folder if it doesn't exist
                if !dest_dir.exists() {
                    fs::create_dir_all(&dest_dir)?;
                }

                // Copy source code to work dir
                if res.is_dir() {
                    tracing::info!(
                        "Copying source from url: {} to {}",
                        res.display(),
                        dest_dir.display()
                    );
                    tool_configuration.fancy_log_handler.wrap_in_progress(
                        "copying source into isolated environment",
                        || {
                            copy_dir::CopyDir::new(&res, &dest_dir)
                                .use_gitignore(false)
                                .run()
                        },
                    )?;
                } else {
                    tracing::info!(
                        "Copying source from url: {} to {}",
                        res.display(),
                        dest_dir.display()
                    );

                    let file_name = src.file_name().unwrap_or(&file_name_from_url);
                    let target = dest_dir.join(file_name);
                    fs::copy(&res, &target)?;
                }

                if !src.patches().is_empty() {
                    patch::apply_patches(system_tools, src.patches(), &dest_dir, recipe_dir)?;
                }

                rendered_sources.push(Source::Url(src.clone()));
            }
            Source::Path(src) => {
                let src_path = recipe_dir.join(src.path()).canonicalize()?;
                tracing::info!("Fetching source from path: {}", src_path.display());

                let dest_dir = if let Some(target_directory) = src.target_directory() {
                    work_dir.join(target_directory)
                } else {
                    work_dir.to_path_buf()
                };

                // Create folder if it doesn't exist
                if !dest_dir.exists() {
                    fs::create_dir_all(&dest_dir)?;
                }

                if !src_path.exists() {
                    return Err(SourceError::FileNotFound(src_path));
                }

                // check if the source path is a directory
                if src_path.is_dir() {
                    let copy_result = tool_configuration.fancy_log_handler.wrap_in_progress(
                        "copying source into isolated environment",
                        || {
                            copy_dir::CopyDir::new(&src_path, &dest_dir)
                                .use_gitignore(src.use_gitignore())
                                .run()
                        },
                    )?;
                    tracing::info!(
                        "Copied {} files into isolated environment",
                        copy_result.copied_paths().len()
                    );
                } else if is_tarball(
                    src_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                ) {
                    extract_tar(&src_path, &dest_dir, &tool_configuration.fancy_log_handler)?;
                    tracing::info!("Extracted to {}", dest_dir.display());
                } else if src_path.extension() == Some(OsStr::new("zip")) {
                    extract_zip(&src_path, &dest_dir, &tool_configuration.fancy_log_handler)?;
                    tracing::info!("Extracted zip to {}", dest_dir.display());
                } else if let Some(file_name) = src
                    .file_name()
                    .cloned()
                    .or_else(|| src_path.file_name().map(PathBuf::from))
                {
                    let dest = dest_dir.join(&file_name);
                    tracing::info!(
                        "Copying source from path: {} to {}",
                        src_path.display(),
                        dest.display()
                    );
                    if let Some(checksum) = Checksum::from_path_source(src) {
                        if !checksum.validate(&src_path) {
                            return Err(SourceError::ValidationFailed);
                        }
                    }
                    fs::copy(&src_path, dest)?;
                } else {
                    return Err(SourceError::FileNotFound(src_path));
                }

                if !src.patches().is_empty() {
                    patch::apply_patches(system_tools, src.patches(), &dest_dir, recipe_dir)?;
                }

                rendered_sources.push(Source::Path(src.clone()));
            }
        }
    }
    Ok(rendered_sources)
}

impl Output {
    /// Fetches the sources for the given output and returns a new output with the finalized sources attached
    pub async fn fetch_sources(
        self,
        tool_configuration: &tool_configuration::Configuration,
    ) -> Result<Self, SourceError> {
        let span = tracing::info_span!("Fetching source code");
        let _enter = span.enter();

        let rendered_sources = fetch_sources(
            self.finalized_sources
                .as_deref()
                .unwrap_or(self.recipe.sources()),
            &self.build_configuration.directories,
            &self.system_tools,
            tool_configuration,
        )
        .await?;

        Ok(Output {
            finalized_sources: Some(rendered_sources),
            ..self
        })
    }
}
