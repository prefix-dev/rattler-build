//! Fetch the sources of a recipe into the source cache *without* building.
//!
//! This downloads (and extracts) every source of an output into the same
//! source cache that a regular build uses (`<output_dir>/src_cache`), but it
//! does not copy the sources into a work directory, does not apply patches and
//! does not run any build scripts. The result tells you where the pristine
//! sources live on disk, so that they can be inspected (e.g. scanned or
//! audited) before the recipe is executed. A subsequent build re-uses the
//! cached sources.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use rattler_build_recipe::stage1::{Source, source::GitRev};
use rattler_build_source_cache::{
    Source as CacheSource, SourceCache, SourceCacheBuilder, cache::is_tarball,
};
use rattler_build_types::NormalizedKey;
use serde::Serialize;

use super::{SourceError, convert_git_source, convert_url_source};
use crate::{tool_configuration, types::Output};

/// The kind of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FetchedSourceKind {
    /// A git repository
    Git,
    /// A URL (archive or single file)
    Url,
    /// A local path
    Path,
}

/// A single source that was fetched into the source cache.
#[derive(Debug, Clone, Serialize)]
pub struct FetchedSource {
    /// The kind of the source.
    pub kind: FetchedSourceKind,

    /// The location the source was fetched from (URL, git URL or the
    /// resolved local path).
    pub location: String,

    /// Location of the pristine (unpatched) source on disk. For archives this
    /// is the extracted directory, for single files the downloaded file, for
    /// git sources the checkout and for path sources the path itself (path
    /// sources are not cached).
    pub path: PathBuf,

    /// For git sources: the resolved commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,

    /// The sub-directory of the work directory the source is placed in during
    /// a build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,

    /// Patches that are applied to this source during the build (resolved
    /// paths). They are *not* applied to [`Self::path`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<PathBuf>,
}

/// The sources of a staging output that a package output inherits from.
#[derive(Debug, Clone, Serialize)]
pub struct FetchedStagingSources {
    /// Name of the staging output.
    pub name: String,
    /// The fetched sources of the staging output.
    pub sources: Vec<FetchedSource>,
}

/// All fetched sources of a single (variant of an) output.
#[derive(Debug, Clone, Serialize)]
pub struct FetchedOutputSources {
    /// Name of the output.
    pub name: String,
    /// Version of the output.
    pub version: String,
    /// Build string of the output.
    pub build_string: String,
    /// The target platform of the output.
    pub target_platform: String,
    /// The variant that was used to render this output.
    pub variant: BTreeMap<NormalizedKey, rattler_build_jinja::Variable>,
    /// The path to the recipe file.
    pub recipe_path: PathBuf,
    /// The source cache directory the sources were fetched into.
    pub source_cache: PathBuf,
    /// The sources of the output itself.
    pub sources: Vec<FetchedSource>,
    /// The sources of the staging output this output inherits from (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staging: Option<FetchedStagingSources>,
}

/// Fetch a list of sources into the given source cache without copying them
/// into a work directory or applying patches.
pub async fn fetch_sources_to_cache(
    sources: &[Source],
    source_cache: &SourceCache,
    recipe_dir: &Path,
    resolve_var: impl Fn(&str) -> Option<PathBuf>,
) -> Result<Vec<FetchedSource>, SourceError> {
    let mut fetched = Vec::with_capacity(sources.len());

    for source in sources {
        let (patches, target_directory) = match source {
            Source::Git(s) => (&s.patches, &s.target_directory),
            Source::Url(s) => (&s.patches, &s.target_directory),
            Source::Path(s) => (&s.patches, &s.target_directory),
        };
        let patches = patches
            .iter()
            .map(|p| recipe_dir.join(p.resolve(&resolve_var)))
            .collect();

        let (kind, location, path, git_commit) = match source {
            Source::Git(git_src) => {
                tracing::info!("Fetching source from git repo: {}", git_src.url);
                let cache_source = convert_git_source(git_src, recipe_dir)?;
                let result = source_cache
                    .get_source(&CacheSource::Git(cache_source))
                    .await
                    .map_err(|e| SourceError::UnknownError(e.to_string()))?;
                let git_commit = result.git_commit.or_else(|| match &git_src.rev {
                    GitRev::Commit(c) => Some(c.clone()),
                    _ => None,
                });
                (
                    FetchedSourceKind::Git,
                    git_src.url.to_string(),
                    result.path,
                    git_commit,
                )
            }
            Source::Url(url_src) => {
                let first_url = url_src
                    .url
                    .first()
                    .expect("we should have at least one URL");
                tracing::info!("Fetching source from url: {}", first_url);
                let cache_source = convert_url_source(url_src)?;
                let result = source_cache
                    .get_source(&CacheSource::Url(cache_source))
                    .await
                    .map_err(|e| SourceError::UnknownError(e.to_string()))?;
                (
                    FetchedSourceKind::Url,
                    first_url.to_string(),
                    result.path,
                    None,
                )
            }
            Source::Path(path_src) => {
                let src_path = recipe_dir.join(&path_src.path);
                let src_path = fs_err::canonicalize(&src_path)
                    .map_err(|_| SourceError::FileNotFound(src_path.clone()))?;
                let location = src_path.display().to_string();

                // Archives are extracted during a build, so extract them into
                // the cache here as well (mirrors `fetch_source`).
                let is_archive = src_path.is_file()
                    && path_src.file_name.is_none()
                    && src_path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        is_tarball(&name) || name.ends_with(".zip") || name.ends_with(".7z")
                    });

                let path = if is_archive {
                    let url_src = rattler_build_recipe::stage1::source::UrlSource {
                        url: vec![url::Url::from_file_path(&src_path).map_err(|_| {
                            SourceError::UnknownError(format!(
                                "Invalid file path: {}",
                                src_path.display()
                            ))
                        })?],
                        md5: path_src.md5,
                        sha256: path_src.sha256,
                        patches: Vec::new(),
                        file_name: None,
                        target_directory: None,
                        attestation: None,
                        filter: path_src.filter.clone(),
                    };
                    source_cache
                        .get_source(&CacheSource::Url(convert_url_source(&url_src)?))
                        .await
                        .map_err(|e| SourceError::UnknownError(e.to_string()))?
                        .path
                } else {
                    src_path
                };
                (FetchedSourceKind::Path, location, path, None)
            }
        };

        fetched.push(FetchedSource {
            kind,
            location,
            path,
            git_commit,
            target_directory: target_directory.clone(),
            patches,
        });
    }

    Ok(fetched)
}

impl Output {
    /// The source cache directory used for this output
    /// (`<output_dir>/src_cache`, same as during a build).
    pub fn source_cache_dir(&self) -> PathBuf {
        self.build_configuration
            .directories
            .output_dir
            .join("src_cache")
    }

    /// Fetch all sources of this output (and of the staging output it
    /// inherits from) into the source cache, without applying patches or
    /// building anything. Returns where each source is located on disk.
    pub async fn fetch_sources_only(
        &self,
        tool_configuration: &tool_configuration::Configuration,
    ) -> Result<FetchedOutputSources, SourceError> {
        let directories = &self.build_configuration.directories;
        let cache_dir = self.source_cache_dir();

        let source_cache = SourceCacheBuilder::new()
            .cache_dir(&cache_dir)
            .client(tool_configuration.client.clone())
            .build()
            .await
            .map_err(|e| SourceError::UnknownError(e.to_string()))?;

        let resolve_var = |var: &str| match var {
            "SRC_DIR" => Some(directories.work_dir.clone()),
            "RECIPE_DIR" => Some(directories.recipe_dir.clone()),
            "BUILD_DIR" => Some(directories.build_dir.clone()),
            _ => None,
        };

        let sources = fetch_sources_to_cache(
            &self.recipe.source,
            &source_cache,
            &directories.recipe_dir,
            resolve_var,
        )
        .await?;

        let staging = match &self.recipe.inherits_from {
            Some(inherits) => {
                let staging = self
                    .recipe
                    .staging_caches
                    .iter()
                    .find(|s| s.name == inherits.cache_name)
                    .ok_or_else(|| {
                        SourceError::UnknownError(format!(
                            "Staging cache '{}' not found in recipe",
                            inherits.cache_name
                        ))
                    })?;
                Some(FetchedStagingSources {
                    name: staging.name.clone(),
                    sources: fetch_sources_to_cache(
                        &staging.source,
                        &source_cache,
                        &directories.recipe_dir,
                        resolve_var,
                    )
                    .await?,
                })
            }
            None => None,
        };

        Ok(FetchedOutputSources {
            name: self.name().as_normalized().to_string(),
            version: self.version().to_string(),
            build_string: self.build_string().to_string(),
            target_platform: self.target_platform().to_string(),
            variant: self.variant().clone(),
            recipe_path: directories.recipe_path.clone(),
            source_cache: cache_dir,
            sources,
            staging,
        })
    }
}
