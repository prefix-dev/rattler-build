//! Functions to deal with the build cache
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use fs_err as fs;
use memchr::memmem;
use memmap2::Mmap;
use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    env_vars,
    metadata::{build_reindexed_channels, Output},
    packaging::{contains_prefix_binary, contains_prefix_text, content_type, Files},
    recipe::parser::{Dependency, Requirements, Source},
    render::resolved_dependencies::{
        install_environments, resolve_dependencies, FinalizedDependencies,
    },
    source::copy_dir::{copy_file, create_symlink, CopyDir, CopyOptions},
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

    /// The prefix files that are included in the cache
    pub prefix_files: Vec<(PathBuf, bool)>,

    /// The (dirty) source files that are included in the cache
    pub work_dir_files: Vec<PathBuf>,

    /// The prefix that was used at build time (needs to be replaced when
    /// restoring the files)
    pub prefix: PathBuf,

    /// The sources that were already present in the `work_dir`
    pub sources: Vec<Source>,
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
                if let Some(value) = self.variant().get(key) {
                    selected_variant.insert(key.as_ref(), value.clone());
                }
            }
            // always insert the target platform and build platform
            // we are using the `host_platform` here because for the cache it should not
            // matter whether it's being build for `noarch` or not (one can have
            // mixed outputs, in fact).
            selected_variant.insert("host_platform", self.host_platform().platform.to_string());
            selected_variant.insert(
                "build_platform",
                self.build_configuration.build_platform.platform.to_string(),
            );

            let cache_key = (cache, selected_variant);
            // serialize to json and hash
            let mut hasher = Sha256::new();
            let serialized = serde_json::to_string(&cache_key)?;
            hasher.update(serialized.as_bytes());
            let result = hasher.finalize();
            Ok(format!("{:x}", result))
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
        let copy_options = CopyOptions {
            skip_exist: true,
            ..Default::default()
        };
        let cache_prefix = cache.prefix;

        let mut paths_created = HashSet::new();
        for (file, has_prefix) in &cache.prefix_files {
            tracing::info!("Restoring from cache: {:?}", file);
            let dest = self.prefix().join(file);
            let source = &cache_dir.join("prefix").join(file);
            copy_file(source, &dest, &mut paths_created, &copy_options).into_diagnostic()?;

            // check if the symlink starts with the old prefix, and if yes, make the symlink
            // absolute with the new prefix
            if source.is_symlink() {
                let symlink_target = fs::read_link(source).into_diagnostic()?;
                if let Ok(rest) = symlink_target.strip_prefix(&cache_prefix) {
                    let new_symlink_target = self.prefix().join(rest);
                    fs::remove_file(&dest).into_diagnostic()?;
                    create_symlink(&new_symlink_target, &dest).into_diagnostic()?;
                }
            }

            if *has_prefix {
                replace_prefix(&dest, &cache_prefix, self.prefix())?;
            }
        }

        // restore the work dir files
        let cache_dir_work = cache_dir.join("work_dir");
        CopyDir::new(
            &cache_dir_work,
            &self.build_configuration.directories.work_dir,
        )
        .run()
        .into_diagnostic()?;

        tracing::info!("Restored source files from cache");

        Ok(Output {
            finalized_cache_dependencies: Some(cache.finalized_dependencies.clone()),
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

            tracing::info!("Cache key: {:?}", self.cache_key().into_diagnostic()?);
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
                            .fetch_sources(Some(cache.sources.clone()), tool_configuration)
                            .await
                            .into_diagnostic()?;
                        return self.restore_cache(cache, cache_dir).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse cache: {:?} - rebuilding", e);
                        tracing::info!("JSON: {}", text);
                        // remove the cache dir and run as normal
                        fs::remove_dir_all(&cache_dir).into_diagnostic()?;
                    }
                }
            }

            self = self
                .fetch_sources(None, tool_configuration)
                .await
                .into_diagnostic()?;

            let target_platform = self.build_configuration.target_platform;
            let mut env_vars = env_vars::vars(&self, "BUILD");
            env_vars.extend(env_vars::os_vars(self.prefix(), &target_platform));

            // Reindex the channels
            let channels = build_reindexed_channels(&self.build_configuration, tool_configuration)
                .into_diagnostic()
                .context("failed to reindex output channel")?;

            let finalized_dependencies =
                resolve_dependencies(&cache.requirements, &self, &channels, tool_configuration)
                    .await
                    .unwrap();

            install_environments(&self, &finalized_dependencies, tool_configuration)
                .await
                .into_diagnostic()?;

            cache
                .build
                .script()
                .run_script(
                    env_vars,
                    &self.build_configuration.directories.work_dir,
                    &self.build_configuration.directories.recipe_dir,
                    &self.build_configuration.directories.host_prefix,
                    Some(&self.build_configuration.directories.build_prefix),
                    None, // TODO fix this to be proper Jinja context
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

                // Defend against broken symlinks here!
                if !file.is_symlink() {
                    // check if the file contains the prefix
                    let content_type = content_type(file).into_diagnostic()?;
                    let has_prefix = if content_type.map(|c| c.is_text()).unwrap_or(false) {
                        contains_prefix_text(file, self.prefix(), self.target_platform())
                    } else {
                        contains_prefix_binary(file, self.prefix())
                    }
                    .into_diagnostic()?;
                    copied_files.push((stripped.to_path_buf(), has_prefix));
                } else {
                    copied_files.push((stripped.to_path_buf(), false));
                }
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
                prefix_files: copied_files,
                work_dir_files: work_dir_files.copied_paths().to_vec(),
                prefix: self.prefix().to_path_buf(),
                sources: self.recipe.source.clone(),
            };

            let cache_file = cache_dir.join("cache.json");
            fs::write(cache_file, serde_json::to_string(&cache).unwrap()).into_diagnostic()?;

            Ok(Output {
                finalized_cache_dependencies: Some(finalized_dependencies),
                ..self
            })
        } else {
            Ok(self)
        }
    }
}

/// Simple replace prefix function that does a direct replacement without any
/// padding considerations because we know that the prefix is the same length as
/// the original prefix.
fn replace_prefix(file: &Path, old_prefix: &Path, new_prefix: &Path) -> Result<(), miette::Error> {
    // mmap the file, and use the fast string search to find the prefix
    let output = {
        let map_file = fs::File::open(file).into_diagnostic()?;
        let mmap = unsafe { Mmap::map(&map_file).into_diagnostic()? };
        let new_prefix_bytes = new_prefix.as_os_str().as_encoded_bytes();
        let old_prefix_bytes = old_prefix.as_os_str().as_encoded_bytes();

        // if the prefix is the same, we don't need to do anything
        if old_prefix == new_prefix {
            return Ok(());
        }

        assert_eq!(
            new_prefix_bytes.len(),
            old_prefix_bytes.len(),
            "Prefixes must have the same length: {:?} != {:?}",
            new_prefix,
            old_prefix
        );

        let mut output = Vec::with_capacity(mmap.len());
        let mut last_match_end = 0;
        let finder = memmem::Finder::new(old_prefix_bytes);

        while let Some(index) = finder.find(&mmap[last_match_end..]) {
            let absolute_index = last_match_end + index;
            output.extend_from_slice(&mmap[last_match_end..absolute_index]);
            output.extend_from_slice(new_prefix_bytes);
            last_match_end = absolute_index + new_prefix_bytes.len();
        }
        output.extend_from_slice(&mmap[last_match_end..]);
        output
        // close file & mmap at end of scope
    };

    // overwrite the file
    fs::write(file, output).into_diagnostic()
}
