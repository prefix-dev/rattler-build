//! Staging cache support for multi-output recipes
//!
//! This module handles building and caching staging outputs in multi-output recipes.
//! Staging outputs are built once and cached, then package outputs can inherit from them
//! to avoid redundant rebuilds of common dependencies.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use fs_err as fs;
use miette::{Context, IntoDiagnostic};
use minijinja::Value;
use rattler_build_jinja::{Jinja, Variable};
use rattler_build_recipe::stage1::{InheritsFrom, StagingCache};
use rattler_build_script::Debug as ScriptDebug;
use rattler_build_types::NormalizedKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    env_vars,
    metadata::{Output, build_reindexed_channels},
    packaging::Files,
    render::resolved_dependencies::{
        FinalizedDependencies, RunExportsDownload, install_environments, resolve_dependencies,
    },
    source::{copy_dir::CopyDir, fetch_sources},
};

/// Error type for staging cache operations
#[derive(Debug, thiserror::Error)]
pub enum StagingError {
    /// Error serializing staging cache metadata
    #[error("Error serializing staging cache: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Error during I/O operations
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Staging cache not found
    #[error("Staging cache '{0}' not found")]
    CacheNotFound(String),

    /// Invalid staging cache reference
    #[error("Invalid staging cache reference: {0}")]
    InvalidReference(String),
}

/// Metadata for a built staging cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagingCacheMetadata {
    /// The name of the staging cache
    pub name: String,

    /// The finalized dependencies that were used to build the cache
    pub finalized_dependencies: FinalizedDependencies,

    /// The finalized sources (with exact git hashes, etc.)
    pub finalized_sources: Vec<rattler_build_recipe::stage1::Source>,

    /// The files that were produced in the prefix
    pub prefix_files: Vec<PathBuf>,

    /// The files from the work directory that are included in the cache
    pub work_dir_files: Vec<PathBuf>,

    /// The prefix path that was used at build time
    pub prefix: PathBuf,

    /// The variant configuration that was used
    pub variant: BTreeMap<NormalizedKey, Variable>,
}

impl Output {
    /// Compute a cache key for a staging cache
    ///
    /// The cache key is based on:
    /// - The staging cache configuration (requirements, sources)
    /// - The relevant variant variables (those used in the requirements)
    /// - The build and host platforms
    pub fn staging_cache_key(&self, staging: &StagingCache) -> Result<String, StagingError> {
        // Collect variant variable names that are used in the staging requirements
        // We look at build and host requirements (build time dependencies)
        let requirement_names: std::collections::HashSet<String> = staging
            .requirements
            .build
            .iter()
            .chain(staging.requirements.host.iter())
            .filter_map(|dep| {
                if let rattler_build_recipe::stage1::Dependency::Spec(spec) = dep {
                    // Only include variables that appear in simple specs without version/build
                    if spec.version.is_none()
                        && spec.build.is_none()
                        && let Some(matcher) = spec.name.as_ref()
                        && let rattler_conda_types::PackageNameMatcher::Exact(name) = matcher
                    {
                        return Some(name.as_normalized().to_string());
                    }
                }
                None
            })
            .collect();

        // Select only the variant variables that are relevant
        let mut selected_variant = BTreeMap::new();
        for key in requirement_names.iter() {
            if let Some(value) = self.variant().get(&key.as_str().into()) {
                selected_variant.insert(key.as_ref(), value.clone());
            }
        }

        // Always include platform information
        selected_variant.insert(
            "host_platform",
            self.host_platform().platform.to_string().into(),
        );
        selected_variant.insert(
            "build_platform",
            self.build_configuration
                .build_platform
                .platform
                .to_string()
                .into(),
        );

        // Create the cache key from staging config + selected variant + prefix
        let cache_key = (&staging, &selected_variant, self.prefix());

        // Serialize to JSON and hash
        let mut hasher = Sha256::new();
        cache_key.serialize(&mut serde_json::Serializer::new(&mut hasher))?;
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Build a staging cache or restore it if it already exists
    ///
    /// This will:
    /// 1. Check if the cache exists and is valid
    /// 2. If yes, restore the cached files to the prefix
    /// 3. If no, build the staging cache and save it
    ///
    /// Returns the finalized dependencies and sources from the staging cache
    pub async fn build_or_restore_staging_cache(
        &self,
        staging: &StagingCache,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<
        (
            FinalizedDependencies,
            Vec<rattler_build_recipe::stage1::Source>,
        ),
        miette::Error,
    > {
        let span = tracing::info_span!("Building or restoring staging cache", cache = staging.name);
        let _enter = span.enter();

        let cache_key = self
            .staging_cache_key(staging)
            .into_diagnostic()
            .context("Failed to compute staging cache key")?;

        tracing::info!("Staging cache key: {}", cache_key);

        let cache_dir = self
            .build_configuration
            .directories
            .cache_dir
            .join(format!("staging_{}", cache_key));

        // Try to restore existing cache
        if cache_dir.exists() {
            let metadata_path = cache_dir.join("metadata.json");
            if metadata_path.exists() {
                match fs::read_to_string(&metadata_path) {
                    Ok(text) => match serde_json::from_str::<StagingCacheMetadata>(&text) {
                        Ok(metadata) => {
                            tracing::info!("Restoring staging cache from {}", cache_dir.display());
                            return self.restore_staging_cache(metadata, &cache_dir).await;
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse staging cache metadata at {}: {:?} - rebuilding",
                                metadata_path.display(),
                                e
                            );
                            // Remove corrupted cache and rebuild
                            fs::remove_dir_all(&cache_dir).into_diagnostic()?;
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to read staging cache metadata at {}: {:?} - rebuilding",
                            metadata_path.display(),
                            e
                        );
                        fs::remove_dir_all(&cache_dir).into_diagnostic()?;
                    }
                }
            }
        }

        // Build new cache
        self.build_staging_cache(staging, &cache_dir, tool_configuration)
            .await
    }

    /// Build a new staging cache from scratch
    async fn build_staging_cache(
        &self,
        staging: &StagingCache,
        cache_dir: &Path,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<
        (
            FinalizedDependencies,
            Vec<rattler_build_recipe::stage1::Source>,
        ),
        miette::Error,
    > {
        tracing::info!("Building new staging cache: {}", staging.name);

        // Fetch sources for the staging build
        let finalized_sources = fetch_sources(
            &staging.source,
            &self.build_configuration.directories,
            &self.system_tools,
            tool_configuration,
            crate::apply_patch_custom,
        )
        .await
        .into_diagnostic()?;

        // Resolve dependencies
        let channels = build_reindexed_channels(&self.build_configuration, tool_configuration)
            .await
            .into_diagnostic()
            .context("failed to reindex output channel")?;

        let finalized_dependencies = resolve_dependencies(
            &staging.requirements,
            self,
            &channels,
            tool_configuration,
            RunExportsDownload::DownloadMissing,
        )
        .await
        .into_diagnostic()?;

        // Install environments
        install_environments(self, &finalized_dependencies, tool_configuration)
            .await
            .into_diagnostic()?;

        // Run the build script
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(self.prefix(), &target_platform));

        // Create Jinja context
        let selector_config = self.build_configuration.selector_config();
        let mut jinja = Jinja::new(selector_config.clone());

        // Add context from the recipe
        for (k, v) in self.recipe.context().iter() {
            jinja.context_mut().insert(k.clone(), v.clone().into());
        }

        // Add env vars to jinja context
        for (k, v) in &env_vars {
            if let Some(value) = v {
                jinja
                    .context_mut()
                    .insert(k.clone(), Value::from_safe_string(value.clone()));
            }
        }

        let jinja_renderer = |template: &str| -> Result<String, String> {
            jinja.render_str(template).map_err(|e| e.to_string())
        };

        let build_prefix = if staging.build.merge_build_and_host_envs {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        staging
            .build
            .script
            .run_script(
                env_vars,
                &self.build_configuration.directories.work_dir,
                &self.build_configuration.directories.recipe_dir,
                &self.build_configuration.directories.host_prefix,
                build_prefix,
                Some(jinja_renderer),
                self.build_configuration.sandbox_config(),
                ScriptDebug::new(self.build_configuration.debug.is_enabled()),
            )
            .await
            .into_diagnostic()?;

        // Find the new files in the prefix
        let new_files = Files::from_prefix(
            self.prefix(),
            &staging.build.always_include_files,
            &staging.build.files,
            None,
        )
        .into_diagnostic()?;

        // Create cache directory and copy files
        let prefix_cache_dir = cache_dir.join("prefix");
        fs::create_dir_all(&prefix_cache_dir).into_diagnostic()?;

        // Copy only the new files to the cache
        // We can't use CopyDir with a filter, so we copy files individually
        let copied_files: Vec<PathBuf> = new_files
            .new_files
            .iter()
            .filter_map(|file| {
                // Skip directories (unless they are symlinks)
                if file.is_dir() && !file.is_symlink() {
                    return None;
                }

                let stripped = file.strip_prefix(self.prefix()).ok()?;
                let dest = prefix_cache_dir.join(stripped);

                // Create parent directories if needed
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                // Copy the file (or symlink)
                if file.is_symlink() {
                    if let Ok(link_target) = fs_err::read_link(file) {
                        #[cfg(unix)]
                        {
                            let _ = std::os::unix::fs::symlink(&link_target, &dest);
                        }
                        #[cfg(windows)]
                        {
                            if link_target.is_dir() {
                                let _ = std::os::windows::fs::symlink_dir(&link_target, &dest);
                            } else {
                                let _ = std::os::windows::fs::symlink_file(&link_target, &dest);
                            }
                        }
                    }
                } else {
                    let _ = fs_err::copy(file, &dest);
                }

                Some(stripped.to_path_buf())
            })
            .collect();

        // Copy work directory files
        let work_dir_cache = cache_dir.join("work_dir");
        let copied_work_dir = CopyDir::new(
            &self.build_configuration.directories.work_dir,
            &work_dir_cache,
        )
        .run()
        .into_diagnostic()?;

        // Save metadata
        let metadata = StagingCacheMetadata {
            name: staging.name.clone(),
            finalized_dependencies: finalized_dependencies.clone(),
            finalized_sources: finalized_sources.clone(),
            prefix_files: copied_files,
            work_dir_files: copied_work_dir.copied_paths().to_vec(),
            prefix: self.prefix().to_path_buf(),
            variant: staging.used_variant.clone(),
        };

        let metadata_json = serde_json::to_string_pretty(&metadata).into_diagnostic()?;
        fs::write(cache_dir.join("metadata.json"), metadata_json).into_diagnostic()?;

        tracing::info!(
            "Staging cache built with {} prefix files and {} work dir files",
            metadata.prefix_files.len(),
            metadata.work_dir_files.len()
        );

        Ok((finalized_dependencies, finalized_sources))
    }

    /// Restore a staging cache from disk
    async fn restore_staging_cache(
        &self,
        metadata: StagingCacheMetadata,
        cache_dir: &Path,
    ) -> Result<
        (
            FinalizedDependencies,
            Vec<rattler_build_recipe::stage1::Source>,
        ),
        miette::Error,
    > {
        let prefix_cache_dir = cache_dir.join("prefix");
        let work_dir_cache = cache_dir.join("work_dir");

        // IMPORTANT: Clean the prefix directory before restoring from cache
        // The prefix may already have files from dependency installation or previous builds
        if self.prefix().exists() {
            tracing::debug!("Removing existing prefix before cache restoration");
            fs::remove_dir_all(self.prefix()).into_diagnostic()?;
        }

        // Clean the work directory as well
        if self.build_configuration.directories.work_dir.exists() {
            tracing::debug!("Removing existing work directory before cache restoration");
            fs::remove_dir_all(&self.build_configuration.directories.work_dir).into_diagnostic()?;
        }

        // Restore prefix files
        let copied_prefix = CopyDir::new(&prefix_cache_dir, self.prefix())
            .run()
            .into_diagnostic()?;

        // Restore work directory files
        let copied_work_dir = CopyDir::new(
            &work_dir_cache,
            &self.build_configuration.directories.work_dir,
        )
        .run()
        .into_diagnostic()?;

        let total_files = copied_prefix.copied_paths().len() + copied_work_dir.copied_paths().len();
        tracing::info!(
            "Restored {} files from staging cache '{}'",
            total_files,
            metadata.name
        );

        Ok((metadata.finalized_dependencies, metadata.finalized_sources))
    }

    /// Process all staging caches for this output
    ///
    /// This will build or restore all staging caches that this output depends on.
    /// If the output inherits from a staging cache, the dependencies and sources
    /// from that cache will be returned.
    pub async fn process_staging_caches(
        &self,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<
        Option<(
            FinalizedDependencies,
            Vec<rattler_build_recipe::stage1::Source>,
        )>,
        miette::Error,
    > {
        tracing::debug!(
            "Processing staging caches for output '{:?}': {} caches, inherits_from: {:?}",
            self.name(),
            self.recipe.staging_caches.len(),
            self.recipe.inherits_from.as_ref().map(|i| &i.cache_name)
        );

        // Build all staging caches that are dependencies
        for staging_cache in &self.recipe.staging_caches {
            tracing::info!(
                "Building or restoring staging cache: {}",
                staging_cache.name
            );
            let (_deps, _sources) = self
                .build_or_restore_staging_cache(staging_cache, tool_configuration)
                .await?;
        }

        // If this output inherits from a staging cache, we need to find it and restore it
        if let Some(inherits) = &self.recipe.inherits_from {
            // Find the staging cache by name
            let staging = self
                .recipe
                .staging_caches
                .iter()
                .find(|s| s.name == inherits.cache_name)
                .ok_or_else(|| {
                    miette::miette!(
                        "Staging cache '{}' not found in recipe",
                        inherits.cache_name
                    )
                })?;

            // Get or build the cache
            let (deps, sources) = self
                .build_or_restore_staging_cache(staging, tool_configuration)
                .await?;

            Ok(Some((deps, sources)))
        } else {
            Ok(None)
        }
    }
}

/// Helper to check if a staging cache should be inherited
pub fn should_inherit_staging_cache(inherits: &Option<InheritsFrom>) -> bool {
    inherits.is_some()
}

/// Get the staging cache name from an inheritance configuration
pub fn get_staging_cache_name(inherits: &InheritsFrom) -> &str {
    &inherits.cache_name
}

/// Check if run_exports should be inherited from the staging cache
pub fn should_inherit_run_exports(inherits: &InheritsFrom) -> bool {
    inherits.inherit_run_exports
}
