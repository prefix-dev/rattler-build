//! Functions to deal with the build cache
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use fs_err as fs;
use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    env_vars,
    metadata::{Output, build_reindexed_channels},
    packaging::Files,
    recipe::{
        Jinja,
        parser::{Dependency, Requirements, Source},
    },
    render::resolved_dependencies::{
        FinalizedDependencies, install_environments, resolve_dependencies,
    },
    source::{
        copy_dir::{CopyDir, CopyOptions, copy_file},
        fetch_sources,
        patch::apply_patch_custom,
    },
};

/// Error type for cache key generation
#[derive(Debug, thiserror::Error)]
pub enum CacheKeyError {
    /// No cache key available (when no `cache` section is present in the
    /// recipe)
    #[error("No cache key available")]
    NoCacheKeyAvailable,
    /// Error serializing cache key with serde_json
    #[error("Error serializing cache: {0}")]
    Serde(#[from] serde_json::Error),
}

///  Cache information for a build
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    /// The requirements that were used to build the cache
    pub requirements: Requirements,

    /// The finalized dependencies
    pub finalized_dependencies: FinalizedDependencies,

    /// The finalized sources
    pub finalized_sources: Vec<Source>,

    /// The prefix files that are included in the cache
    pub prefix_files: Vec<PathBuf>,

    /// The (dirty) source files that are included in the cache
    pub work_dir_files: Vec<PathBuf>,

    /// The prefix that was used at build time (needs to be replaced when
    /// restoring the files)
    pub prefix: PathBuf,
}

impl Output {
    /// Compute a cache key that contains all the information that was used to
    /// build the cache, including the relevant variant information.
    pub fn cache_key(&self) -> Result<String, CacheKeyError> {
        // we have a variant, and we need to find the used variables that are used in
        // the cache to create a hash for the cache ...
        if let Some(cache) = &self.recipe.cache {
            // we need to apply the variant to the cache requirements though
            let requirement_names = cache
                .requirements
                .build_time()
                .filter_map(|x| {
                    if let Dependency::Spec(spec) = x {
                        if spec.version.is_none() && spec.build.is_none() {
                            if let Some(name) = spec.name.as_ref() {
                                return Some(name.as_normalized().to_string());
                            }
                        }
                    }
                    None
                })
                .collect::<HashSet<_>>();

            // intersect variant with requirements
            let mut selected_variant = BTreeMap::new();
            for key in requirement_names.iter() {
                if let Some(value) = self.variant().get(&key.as_str().into()) {
                    selected_variant.insert(key.as_ref(), value.clone());
                }
            }
            // always insert the target platform and build platform
            // we are using the `host_platform` here because for the cache it should not
            // matter whether it's being build for `noarch` or not (one can have
            // mixed outputs, in fact).
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

            let cache_key = (cache, selected_variant, self.prefix());
            // serialize to json and hash
            let mut hasher = Sha256::new();
            cache_key.serialize(&mut serde_json::Serializer::new(&mut hasher))?;
            Ok(format!("{:x}", hasher.finalize()))
        } else {
            Err(CacheKeyError::NoCacheKeyAvailable)
        }
    }

    /// Restore an existing cache from a cache directory
    async fn restore_cache(
        &self,
        cache: Cache,
        cache_dir: PathBuf,
    ) -> Result<Output, miette::Error> {
        let cache_prefix_dir = cache_dir.join("prefix");
        let copied_prefix = CopyDir::new(&cache_prefix_dir, self.prefix())
            .run()
            .into_diagnostic()?;

        // restore the work dir files
        let cache_dir_work = cache_dir.join("work_dir");
        let copied_cache = CopyDir::new(
            &cache_dir_work,
            &self.build_configuration.directories.work_dir,
        )
        .run()
        .into_diagnostic()?;

        let combined_files = copied_prefix.copied_paths().len() + copied_cache.copied_paths().len();
        tracing::info!(
            "Restored {} source and prefix files from cache",
            combined_files
        );

        Ok(Output {
            finalized_cache_dependencies: Some(cache.finalized_dependencies.clone()),
            finalized_cache_sources: Some(cache.finalized_sources.clone()),
            ..self.clone()
        })
    }

    /// This will fetch sources and build the cache if it doesn't exist
    /// Note: this modifies the output in place
    pub(crate) async fn build_or_fetch_cache(
        mut self,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<Self, miette::Error> {
        if let Some(cache) = self.recipe.cache.clone() {
            // if we don't have a cache, we need to run the cache build with our current
            // workdir, and then return the cache
            let span = tracing::info_span!("Running cache build");
            let _enter = span.enter();

            tracing::info!("using cache key: {:?}", self.cache_key().into_diagnostic()?);
            let cache_key = format!("bld_{}", self.cache_key().into_diagnostic()?);

            let cache_dir = self
                .build_configuration
                .directories
                .cache_dir
                .join(cache_key);

            // restore the cache if it exists by copying the files to the prefix
            if cache_dir.exists() {
                let text = fs::read_to_string(cache_dir.join("cache.json")).into_diagnostic()?;
                match serde_json::from_str::<Cache>(&text) {
                    Ok(cache) => {
                        tracing::info!("Restoring cache from {:?}", cache_dir);
                        self = self
                            .fetch_sources(tool_configuration, apply_patch_custom)
                            .await
                            .into_diagnostic()?;
                        return self.restore_cache(cache, cache_dir).await;
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to parse cache at {}: {:?} - rebuilding",
                            cache_dir.join("cache.json").display(),
                            e
                        );
                        // remove the cache dir and run as normal
                        fs::remove_dir_all(&cache_dir).into_diagnostic()?;
                    }
                }
            }

            // fetch the sources for the `cache` section
            let rendered_sources = fetch_sources(
                self.finalized_cache_sources
                    .as_ref()
                    .unwrap_or(&cache.source),
                &self.build_configuration.directories,
                &self.system_tools,
                tool_configuration,
                apply_patch_custom,
            )
            .await
            .into_diagnostic()?;

            let target_platform = self.build_configuration.target_platform;
            let mut env_vars = env_vars::vars(&self, "BUILD");
            env_vars.extend(env_vars::os_vars(self.prefix(), &target_platform));

            // Reindex the channels
            let channels = build_reindexed_channels(&self.build_configuration, tool_configuration)
                .await
                .into_diagnostic()
                .context("failed to reindex output channel")?;

            let finalized_dependencies =
                resolve_dependencies(&cache.requirements, &self, &channels, tool_configuration)
                    .await
                    .unwrap();

            install_environments(&self, &finalized_dependencies, tool_configuration)
                .await
                .into_diagnostic()?;

            let selector_config = self.build_configuration.selector_config();
            let mut jinja = Jinja::new(selector_config.clone());
            for (k, v) in self.recipe.context.iter() {
                jinja.context_mut().insert(k.clone(), v.clone().into());
            }

            let build_prefix = if cache.build.merge_build_and_host_envs {
                None
            } else {
                Some(&self.build_configuration.directories.build_prefix)
            };

            cache
                .build
                .script()
                .run_script(
                    env_vars,
                    &self.build_configuration.directories.work_dir,
                    &self.build_configuration.directories.recipe_dir,
                    &self.build_configuration.directories.host_prefix,
                    build_prefix,
                    Some(jinja),
                    None, // sandbox config
                    self.build_configuration.debug,
                )
                .await
                .into_diagnostic()?;

            // find the new files in the prefix and add them to the cache
            let new_files = Files::from_prefix(
                self.prefix(),
                cache.build.always_include_files(),
                cache.build.files(),
            )
            .into_diagnostic()?;

            // create the cache dir and copy the new files to it
            let prefix_cache_dir = cache_dir.join("prefix");
            fs::create_dir_all(&prefix_cache_dir).into_diagnostic()?;

            let mut creation_cache = HashSet::new();
            let mut copied_files = Vec::new();
            let copy_options = CopyOptions::default();

            for file in &new_files.new_files {
                // skip directories (if they are not a symlink)
                // directories are implicitly created by the files
                if file.is_dir() && !file.is_symlink() {
                    continue;
                }
                let stripped = file
                    .strip_prefix(self.prefix())
                    .expect("File should be in prefix");
                let dest = &prefix_cache_dir.join(stripped);
                copy_file(file, dest, &mut creation_cache, &copy_options).into_diagnostic()?;
                copied_files.push(stripped.to_path_buf());
            }

            // We also need to copy the work dir files to the cache
            let work_dir_files = CopyDir::new(
                &self.build_configuration.directories.work_dir.clone(),
                &cache_dir.join("work_dir"),
            )
            .run()
            .into_diagnostic()?;

            // save the cache
            let cache = Cache {
                requirements: cache.requirements.clone(),
                finalized_dependencies: finalized_dependencies.clone(),
                finalized_sources: rendered_sources.clone(),
                prefix_files: copied_files,
                work_dir_files: work_dir_files.copied_paths().to_vec(),
                prefix: self.prefix().to_path_buf(),
            };

            let cache_file = cache_dir.join("cache.json");
            fs::write(cache_file, serde_json::to_string(&cache).unwrap()).into_diagnostic()?;

            // remove prefix to get it in pristine state and restore the cache
            fs::remove_dir_all(self.prefix()).into_diagnostic()?;
            let _ = CopyDir::new(&prefix_cache_dir, self.prefix())
                .run()
                .into_diagnostic()?;

            Ok(Output {
                finalized_cache_dependencies: Some(finalized_dependencies),
                finalized_cache_sources: Some(rendered_sources),
                ..self
            })
        } else {
            Ok(self)
        }
    }
}
