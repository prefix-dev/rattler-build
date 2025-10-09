//! Module for fetching sources and applying patches
#![allow(dead_code)]

use std::path::{Path, PathBuf, StripPrefixError};

use crate::{
    metadata::{Directories, Output},
    recipe::parser::Source,
    system_tools::ToolError,
    tool_configuration,
};

use fs_err as fs;
use serde::{Deserialize, Serialize};

use crate::system_tools::SystemTools;
pub mod checksum;
pub mod copy_dir;
pub mod create_patch;
pub mod patch;

#[allow(missing_docs)]
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

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

    #[error("Patch application error: {0}")]
    PatchApplyError(#[from] diffy::ApplyError),

    #[error("Failed to parse patch: {0}")]
    PatchParseFailed(PathBuf),

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

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
    _system_tools: &SystemTools, // Not needed with new cache
    tool_configuration: &tool_configuration::Configuration,
    apply_patch: impl Fn(&Path, &Path) -> Result<(), SourceError> + Copy,
) -> Result<Vec<Source>, SourceError> {
    use rattler_build_source_cache::{
        GitSource as CacheGitSource, Source as CacheSource, SourceCacheBuilder,
        UrlSource as CacheUrlSource,
    };

    if sources.is_empty() {
        tracing::info!("No sources to fetch");
        return Ok(Vec::new());
    }

    // Figure out the directories we need
    let work_dir = &directories.work_dir;
    let recipe_dir = &directories.recipe_dir;
    let cache_src = directories.output_dir.join("src_cache");

    // Create the source cache using the authenticated client from tool_configuration
    let source_cache = SourceCacheBuilder::new()
        .cache_dir(&cache_src)
        .client(tool_configuration.client.get_client().clone())
        .build()
        .await
        .map_err(|e| SourceError::UnknownError(e.to_string()))?;

    let mut rendered_sources = Vec::new();

    for src in sources {
        match src {
            Source::Git(git_src) => {
                tracing::info!("Fetching source from git repo: {}", git_src.url());

                // Convert to cache git source using TryFrom
                let cache_git_source =
                    CacheGitSource::try_from(git_src).map_err(SourceError::UnknownError)?;

                let result_path = source_cache
                    .get_source(&CacheSource::Git(cache_git_source))
                    .await
                    .map_err(|e| SourceError::UnknownError(e.to_string()))?;

                let dest_dir = if let Some(target_directory) = git_src.target_directory() {
                    work_dir.join(target_directory)
                } else {
                    work_dir.to_path_buf()
                };

                // Copy from cache to work directory
                if !dest_dir.exists() {
                    fs::create_dir_all(&dest_dir)?;
                }

                tool_configuration.fancy_log_handler.wrap_in_progress(
                    "copying source into isolated environment",
                    || {
                        copy_dir::CopyDir::new(&result_path, &dest_dir)
                            .use_gitignore(false)
                            .run()
                    },
                )?;

                if !git_src.patches().is_empty() {
                    patch::apply_patches(git_src.patches(), &dest_dir, recipe_dir, apply_patch)?;
                }

                rendered_sources.push(src.clone());
            }
            Source::Url(url_src) => {
                let first_url = url_src
                    .urls()
                    .first()
                    .expect("we should have at least one URL");
                tracing::info!("Fetching source from url: {}", first_url);

                // Convert to cache URL source using TryFrom
                let cache_url_source =
                    CacheUrlSource::try_from(url_src).map_err(SourceError::UnknownError)?;

                let result_path = source_cache
                    .get_source(&CacheSource::Url(cache_url_source))
                    .await
                    .map_err(|e| SourceError::UnknownError(e.to_string()))?;

                let dest_dir = if let Some(target_directory) = url_src.target_directory() {
                    work_dir.join(target_directory)
                } else {
                    work_dir.to_path_buf()
                };

                // Create folder if it doesn't exist
                if !dest_dir.exists() {
                    fs::create_dir_all(&dest_dir)?;
                }

                // Copy source code to work dir
                if result_path.is_dir() {
                    tracing::info!(
                        "Copying source from cache: {} to {}",
                        result_path.display(),
                        dest_dir.display()
                    );
                    tool_configuration.fancy_log_handler.wrap_in_progress(
                        "copying source into isolated environment",
                        || {
                            copy_dir::CopyDir::new(&result_path, &dest_dir)
                                .use_gitignore(false)
                                .run()
                        },
                    )?;
                } else {
                    let file_name_from_url = first_url
                        .path_segments()
                        .and_then(|mut segments| segments.next_back().map(|last| last.to_string()))
                        .ok_or_else(|| SourceError::UrlNotFile(first_url.clone()))?;

                    let file_name = url_src.file_name().unwrap_or(&file_name_from_url);
                    let target = dest_dir.join(file_name);
                    tracing::info!(
                        "Copying source from cache: {} to {}",
                        result_path.display(),
                        target.display()
                    );
                    fs::copy(&result_path, &target)?;
                }

                if !url_src.patches().is_empty() {
                    patch::apply_patches(url_src.patches(), &dest_dir, recipe_dir, apply_patch)?;
                }

                rendered_sources.push(src.clone());
            }
            Source::Path(path_src) => {
                // Path sources work the same way as before
                let rel_src_path = path_src.path();
                tracing::debug!("Processing source path '{}'", rel_src_path.display());
                let src_path = fs::canonicalize(recipe_dir.join(rel_src_path))?;
                tracing::info!("Fetching source from path: {}", src_path.display());

                let dest_dir = if let Some(target_directory) = path_src.target_directory() {
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

                // Copy path sources as before (they're not cached)
                if src_path.is_dir() {
                    let copy_result = tool_configuration.fancy_log_handler.wrap_in_progress(
                        "copying source into isolated environment",
                        || {
                            copy_dir::CopyDir::new(&src_path, &dest_dir)
                                .use_gitignore(path_src.use_gitignore())
                                .with_globvec(&path_src.filter)
                                .run()
                        },
                    )?;
                    tracing::info!(
                        "Copied {} files into isolated environment",
                        copy_result.copied_paths().len()
                    );
                } else {
                    // Handle file copying as before
                    let file_name = path_src
                        .file_name()
                        .cloned()
                        .or_else(|| src_path.file_name().map(PathBuf::from));

                    if let Some(file_name) = file_name {
                        let dest = dest_dir.join(&file_name);
                        tracing::info!(
                            "Copying source from path: {} to {}",
                            src_path.display(),
                            dest.display()
                        );
                        if let Some(checksum) = checksum::Checksum::from_path_source(path_src) {
                            if !checksum.validate(&src_path) {
                                return Err(SourceError::ValidationFailed);
                            }
                        }
                        fs::copy(&src_path, dest)?;
                    } else {
                        return Err(SourceError::FileNotFound(src_path));
                    }
                }

                if !path_src.patches().is_empty() {
                    patch::apply_patches(path_src.patches(), &dest_dir, recipe_dir, apply_patch)?;
                }

                rendered_sources.push(src.clone());
            }
        }
    }

    // add a hidden JSON file with the source information (for compatibility)
    let source_info = SourceInformation {
        recipe_path: directories.recipe_path.clone(),
        source_cache: cache_src,
        sources: rendered_sources.clone(),
    };
    let source_info_path = work_dir.join(".source_info.json");
    fs::write(
        &source_info_path,
        serde_json::to_string(&source_info).expect("should serialize"),
    )?;

    Ok(rendered_sources)
}

/// Represents the source information for a recipe, including the path to the recipe and the sources used
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInformation {
    /// The path to the recipe file
    pub recipe_path: PathBuf,

    /// Path to the source cache directory
    pub source_cache: PathBuf,

    /// The sources used in the recipe
    pub sources: Vec<Source>,
}

impl Output {
    /// Fetches the sources for the given output and returns a new output with the finalized sources attached
    pub async fn fetch_sources(
        self,
        tool_configuration: &tool_configuration::Configuration,
        apply_patch: impl Fn(&Path, &Path) -> Result<(), SourceError> + Copy,
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
            apply_patch,
        )
        .await?;

        Ok(Output {
            finalized_sources: Some(rendered_sources),
            ..self
        })
    }
}
