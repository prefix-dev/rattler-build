//! Module for fetching sources and applying patches
use std::{
    ffi::OsStr,
    path::{Path, PathBuf, StripPrefixError},
};

use crate::{
    metadata::{Directories, Output},
    recipe::parser::Source,
    system_tools::ToolError,
    tool_configuration,
};

use fs_err as fs;
use rattler_build_source_cache::cache::is_tarball;
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

/// Copies content from a cache result to the destination directory
fn copy_from_cache(
    cache_path: &Path,
    dest_dir: &Path,
    file_name: Option<&str>,
    tool_config: &tool_configuration::Configuration,
) -> Result<(), SourceError> {
    if cache_path.is_dir() {
        tracing::info!(
            "Copying source from cache: {} to {}",
            cache_path.display(),
            dest_dir.display()
        );
        tool_config.fancy_log_handler.wrap_in_progress(
            "copying source into isolated environment",
            || {
                copy_dir::CopyDir::new(cache_path, dest_dir)
                    .use_gitignore(false)
                    .run()
            },
        )?;
    } else {
        let file_name = file_name.ok_or_else(|| {
            SourceError::UnknownError("Missing file name for file copy".to_string())
        })?;
        let target = dest_dir.join(file_name);
        tracing::info!(
            "Copying source from cache: {} to {}",
            cache_path.display(),
            target.display()
        );
        fs::copy(cache_path, &target)?;
    }
    Ok(())
}

/// Computes the destination directory from an optional target directory
fn compute_dest_dir(work_dir: &Path, target_directory: Option<&PathBuf>) -> PathBuf {
    if let Some(target_directory) = target_directory {
        work_dir.join(target_directory)
    } else {
        work_dir.to_path_buf()
    }
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
        Source as CacheSource, SourceCacheBuilder, UrlSource as CacheUrlSource,
    };

    if sources.is_empty() {
        tracing::info!("No sources to fetch");
        return Ok(Vec::new());
    }

    // Figure out the directories we need
    let work_dir = &directories.work_dir;
    let recipe_dir = &directories.recipe_dir;
    let cache_src = directories.output_dir.join("src_cache");

    // Create the source cache using the client from tool_configuration
    let source_cache = SourceCacheBuilder::new()
        .cache_dir(&cache_src)
        .client(tool_configuration.client.clone())
        .build()
        .await
        .map_err(|e| SourceError::UnknownError(e.to_string()))?;

    let mut rendered_sources = Vec::new();

    for src in sources {
        match src {
            Source::Git(git_src) => {
                tracing::info!("Fetching source from git repo: {}", git_src.url());

                // Convert to cache git source, passing recipe_dir for relative path resolution
                let cache_git_source = git_src
                    .to_cache_source(recipe_dir)
                    .map_err(SourceError::UnknownError)?;

                let result = source_cache
                    .get_source(&CacheSource::Git(cache_git_source))
                    .await
                    .map_err(|e| SourceError::UnknownError(e.to_string()))?;

                let dest_dir = compute_dest_dir(work_dir, git_src.target_directory());
                fs::create_dir_all(&dest_dir)?;

                // Copy from cache to work directory (Git sources are always directories)
                tool_configuration.fancy_log_handler.wrap_in_progress(
                    "copying source into isolated environment",
                    || {
                        copy_dir::CopyDir::new(&result.path, &dest_dir)
                            .use_gitignore(false)
                            .run()
                    },
                )?;

                patch::apply_patches(git_src.patches(), &dest_dir, recipe_dir, apply_patch)?;

                // Update the git source with the resolved commit SHA
                let updated_src = if let Some(commit_sha) = result.git_commit {
                    let mut updated_git_src = git_src.clone();
                    updated_git_src.rev = crate::recipe::parser::GitRev::Commit(commit_sha);
                    Source::Git(updated_git_src)
                } else {
                    src.clone()
                };

                rendered_sources.push(updated_src);
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

                let result = source_cache
                    .get_source(&CacheSource::Url(cache_url_source))
                    .await
                    .map_err(|e| SourceError::UnknownError(e.to_string()))?;

                let dest_dir = compute_dest_dir(work_dir, url_src.target_directory());
                fs::create_dir_all(&dest_dir)?;

                // Copy source code to work dir
                if result.path.is_dir() {
                    copy_from_cache(&result.path, &dest_dir, None, tool_configuration)?;
                } else {
                    // For non-extracted files, we need to determine the file name
                    let file_name_from_url = first_url
                        .path_segments()
                        .and_then(|mut segments| segments.next_back().map(|last| last.to_string()))
                        .ok_or_else(|| SourceError::UrlNotFile(first_url.clone()))?;

                    let file_name = url_src.file_name().unwrap_or(&file_name_from_url);
                    copy_from_cache(&result.path, &dest_dir, Some(file_name), tool_configuration)?;
                }

                patch::apply_patches(url_src.patches(), &dest_dir, recipe_dir, apply_patch)?;

                rendered_sources.push(src.clone());
            }
            Source::Path(path_src) => {
                let rel_src_path = path_src.path();
                tracing::debug!("Processing source path '{}'", rel_src_path.display());
                let src_path = fs::canonicalize(recipe_dir.join(rel_src_path))?;
                tracing::info!("Fetching source from path: {}", src_path.display());

                if !src_path.exists() {
                    return Err(SourceError::FileNotFound(src_path));
                }

                let dest_dir = compute_dest_dir(work_dir, path_src.target_directory());
                fs::create_dir_all(&dest_dir)?;

                if src_path.is_dir() {
                    // Copy directory with gitignore and filter support
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
                    // Handle single file
                    let file_name_from_path = src_path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string());
                    let file_name = path_src
                        .file_name()
                        .cloned()
                        .or(file_name_from_path)
                        .ok_or_else(|| SourceError::FileNotFound(src_path.clone()))?;

                    // Determine if we should extract this file
                    let should_extract = path_src.file_name().is_none()
                        && (is_tarball(&file_name)
                            || src_path.extension() == Some(OsStr::new("zip"))
                            || src_path.extension() == Some(OsStr::new("7z")));

                    if should_extract {
                        // Extract archive via cache
                        let file_url = url::Url::from_file_path(&src_path).unwrap();
                        let temp_url_source = crate::recipe::parser::UrlSource {
                            url: vec![file_url],
                            md5: path_src.md5,
                            sha256: path_src.sha256,
                            patches: path_src.patches.clone(),
                            file_name: None, // Don't set file_name for extraction
                            target_directory: path_src.target_directory.clone(),
                        };

                        let cache_url_source = CacheUrlSource::try_from(&temp_url_source)
                            .map_err(SourceError::UnknownError)?;
                        let result = source_cache
                            .get_source(&CacheSource::Url(cache_url_source))
                            .await
                            .map_err(|e| SourceError::UnknownError(e.to_string()))?;

                        copy_from_cache(
                            &result.path,
                            &dest_dir,
                            Some(&file_name),
                            tool_configuration,
                        )?;
                    } else {
                        // Copy file directly without extraction
                        let dest = dest_dir.join(&file_name);
                        tracing::info!(
                            "Copying source from path: {} to {}",
                            src_path.display(),
                            dest.display()
                        );

                        // Validate checksum if provided
                        if let Some(checksum) = checksum::Checksum::from_path_source(path_src) {
                            if !checksum.validate(&src_path) {
                                return Err(SourceError::ValidationFailed);
                            }
                        }

                        fs::copy(&src_path, dest)?;
                    }
                }

                patch::apply_patches(path_src.patches(), &dest_dir, recipe_dir, apply_patch)?;

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
