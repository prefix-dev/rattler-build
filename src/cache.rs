//! Functions to deal with the build cache
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use content_inspector::ContentType;
use fs_err as fs;
use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    env_vars,
    metadata::{Output, build_reindexed_channels},
    packaging::{Files, contains_prefix_binary, contains_prefix_text, rewrite_prefix_in_file},
    recipe::{
        Jinja,
        parser::{Dependency, Requirements, Source},
    },
    render::resolved_dependencies::{
        FinalizedDependencies, RunExportsDownload, install_environments, resolve_dependencies,
    },
    source::{
        copy_dir::{CopyDir, CopyOptions, copy_file},
        fetch_sources,
        patch::apply_patch_custom,
    },
};

/// Check if a file contains the prefix and determine if it's binary or text
/// Returns (has_prefix, is_text)
fn check_file_for_prefix(file_path: &Path, prefix: &Path) -> (bool, bool) {
    let content = match fs::read(file_path) {
        Ok(content) => content,
        Err(_) => return (false, false),
    };
    let content_type = content_inspector::inspect(&content);
    let is_text = content_type.is_text()
        && matches!(content_type, ContentType::UTF_8 | ContentType::UTF_8_BOM);

    if is_text {
        match contains_prefix_text(file_path, prefix) {
            Ok(Some(_)) => (true, true),
            Ok(None) => (false, true),
            Err(_) => (false, true),
        }
    } else {
        #[cfg(target_family = "unix")]
        {
            match contains_prefix_binary(file_path, prefix) {
                Ok(has_prefix) => (has_prefix, false),
                Err(_) => (false, false),
            }
        }
        #[cfg(target_family = "windows")]
        {
            if let Ok(contents) = fs::read(file_path) {
                let prefix_bytes = prefix.to_string_lossy().as_bytes();
                (
                    contents
                        .windows(prefix_bytes.len())
                        .any(|window| window == prefix_bytes),
                    false,
                )
            } else {
                (false, false)
            }
        }
    }
}

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

    /// The work_dir that was used at build time (used to rewrite restored files
    /// if the absolute path changes between cache build and restore)
    #[serde(default)]
    pub work_dir: PathBuf,

    /// The run exports declared by the cache at build time (rendered form is computed later)
    #[serde(default)]
    pub run_exports: crate::recipe::parser::RunExports,

    /// Files (relative to prefix/work_dir) that contain the old prefix string and
    /// should be rewritten when restoring to a different location.
    #[serde(default)]
    pub files_with_prefix: Vec<PathBuf>,

    /// Files (relative to prefix/work_dir) that contain the old prefix string and
    /// are binary files. These need to be handled during prefix replacement.
    #[serde(default)]
    pub binary_files_with_prefix: Vec<PathBuf>,

    /// Files (relative to work_dir) that contain the old work_dir path and
    /// should be rewritten when restoring to a different location.
    #[serde(default)]
    pub files_with_work_dir: Vec<PathBuf>,

    /// Source files from the cache build (relative to work_dir)
    /// Used to detect potential conflicts when outputs add additional sources
    #[serde(default)]
    pub source_files: Vec<PathBuf>,
}

impl Output {
    /// Compute cache key for a specific cache output
    pub fn cache_key_for(
        &self,
        cache_name: &str,
        cache_reqs: &crate::recipe::parser::CacheRequirements,
    ) -> Result<String, CacheKeyError> {
        let mut requirement_names: HashSet<_> = cache_reqs
            .build
            .iter()
            .chain(cache_reqs.host.iter())
            .filter_map(|x| {
                if let crate::recipe::parser::Dependency::Spec(spec) = x {
                    if spec.version.is_none() && spec.build.is_none() {
                        spec.name.as_ref().map(|n| n.as_normalized().to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        // Also include explicit variant.use keys from the cache build (if any)
        if let Some(cache) = &self.recipe.cache {
            for key in cache.build.variant.use_keys.iter() {
                requirement_names.insert(key.clone());
            }
        }

        let mut selected_variant = BTreeMap::new();
        for key in &requirement_names {
            if let Some(value) = self.variant().get(&key.as_str().into()) {
                selected_variant.insert(key.as_str(), value.clone());
            }
        }

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

        // Include cache name for uniqueness. Do NOT include absolute paths to keep
        // the cache key stable across different build roots.
        let rebuild_key = (cache_name, cache_reqs, selected_variant);
        let mut hasher = Sha256::new();
        rebuild_key.serialize(&mut serde_json::Serializer::new(&mut hasher))?;
        Ok(format!("{:x}", hasher.finalize()))
    }

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

            // Do NOT include absolute paths to keep the cache key stable across
            // different build roots.
            let cache_key = (cache, selected_variant);
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
            .overwrite(true)
            .on_overwrite(|path| {
                tracing::warn!(
                    "File {} restored from cache will be overwritten during output build",
                    path.display()
                );
            })
            .run()
            .into_diagnostic()?;

        // restore the work dir files
        let cache_dir_work = cache_dir.join("work_dir");
        let copied_cache = CopyDir::new(
            &cache_dir_work,
            &self.build_configuration.directories.work_dir,
        )
        .overwrite(true)
        .on_overwrite(|path| {
            tracing::warn!(
                "File {} restored from cache will be overwritten during output build",
                path.display()
            );
        })
        .run()
        .into_diagnostic()?;

        // Track cached files for conflict detection.
        let cached_prefix_files = copied_prefix.copied_paths_owned();
        let cached_work_files = copied_cache.copied_paths_owned();

        // If the output also specifies additional sources, proactively warn when
        // extraction would clobber files restored from cache work_dir.
        if !self.recipe.source.is_empty() {
            for rel in &cached_work_files {
                let target = self.build_configuration.directories.work_dir.join(rel);
                if target.exists() {
                    tracing::warn!(
                        "Source extraction may overwrite restored cache work file: {}",
                        target.display()
                    );
                    self.record_warning(&format!(
                        "Source extraction may overwrite restored cache work file: {}",
                        target.display()
                    ));
                }
            }
        }
        let combined_files = cached_prefix_files.len() + cached_work_files.len();
        tracing::info!(
            "Restored {} source and prefix files from cache",
            combined_files
        );

        // If the cache was built under a different prefix, rewrite occurrences of
        // the old prefix in restored text and binary files.
        if cache.prefix != *self.prefix() {
            for rel in cache
                .files_with_prefix
                .iter()
                .chain(cache.binary_files_with_prefix.iter())
            {
                for base in [
                    self.prefix(),
                    &self.build_configuration.directories.work_dir,
                ] {
                    let path = base.join(rel);
                    if !path.exists() {
                        continue;
                    }

                    if let Err(e) = rewrite_prefix_in_file(&path, &cache.prefix, self.prefix()) {
                        tracing::warn!("Failed to rewrite restored file {}: {}", path.display(), e);
                    }
                }
            }
        }

        // If the cache was built under a different work_dir, rewrite occurrences
        // of the old work_dir path in restored text files located under the new work_dir.
        if !cache.work_dir.as_os_str().is_empty()
            && cache.work_dir != self.build_configuration.directories.work_dir
        {
            for rel in cache.files_with_work_dir.iter() {
                let path = self.build_configuration.directories.work_dir.join(rel);
                if !path.exists() {
                    continue;
                }
                if let Err(e) = rewrite_prefix_in_file(
                    &path,
                    &cache.work_dir,
                    &self.build_configuration.directories.work_dir,
                ) {
                    tracing::warn!(
                        "Failed to rewrite restored work_dir file {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        Ok(Output {
            finalized_cache_dependencies: Some(cache.finalized_dependencies.clone()),
            finalized_cache_sources: Some(cache.finalized_sources.clone()),
            // Recipe already has run_exports merged during inheritance resolution,
            // so we don't need to merge them again here
            recipe: self.recipe.clone(),
            restored_cache_prefix_files: Some(cached_prefix_files),
            restored_cache_work_dir_files: Some(cached_work_files.clone()),
            ..self.clone()
        })
    }

    /// Build or fetch a specific cache output
    pub async fn build_or_fetch_cache_output(
        mut self,
        cache_output: &crate::recipe::parser::CacheOutput,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<Self, miette::Error> {
        let cache_name = cache_output.name.as_normalized();
        let cache_key = self
            .cache_key_for(cache_name, &cache_output.requirements)
            .into_diagnostic()?;

        tracing::info!("Building cache: {} with key: {}", cache_name, cache_key);

        let cache_dir = self
            .build_configuration
            .directories
            .cache_dir
            .join(format!("{}_{}", cache_name, cache_key));

        // Check if cache exists
        if cache_dir.exists() {
            let cache_json = cache_dir.join("cache.json");
            if let Ok(text) = fs::read_to_string(&cache_json) {
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
                        tracing::warn!(
                            "Failed to parse cache.json at {}: {} - rebuilding",
                            cache_json.display(),
                            e
                        );
                        fs::remove_dir_all(&cache_dir).into_diagnostic()?;
                    }
                }
            }
        }

        // Build the cache
        let rendered_sources = fetch_sources(
            &cache_output.source,
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

        let channels = build_reindexed_channels(&self.build_configuration, tool_configuration)
            .await
            .into_diagnostic()?;

        // Convert CacheRequirements to Requirements
        let requirements = crate::recipe::parser::Requirements {
            build: cache_output.requirements.build.clone(),
            host: cache_output.requirements.host.clone(),
            run: Vec::new(),
            run_constraints: Vec::new(),
            run_exports: crate::recipe::parser::RunExports::default(),
            ignore_run_exports: cache_output.ignore_run_exports.clone().unwrap_or_default(),
        };

        let finalized_dependencies = resolve_dependencies(
            &requirements,
            &self,
            &channels,
            tool_configuration,
            RunExportsDownload::DownloadMissing,
        )
        .await
        .into_diagnostic()?;

        install_environments(&self, &finalized_dependencies, tool_configuration)
            .await
            .into_diagnostic()?;

        let selector_config = self.build_configuration.selector_config();
        let mut jinja = Jinja::new(selector_config);
        for (k, v) in self.recipe.context.iter() {
            jinja.context_mut().insert(k.clone(), v.clone().into());
        }

        let build_prefix = if cache_output.build.script.is_some() {
            Some(&self.build_configuration.directories.build_prefix)
        } else {
            None
        };

        if let Some(script) = &cache_output.build.script {
            script
                .run_script(
                    env_vars,
                    &self.build_configuration.directories.work_dir,
                    &self.build_configuration.directories.recipe_dir,
                    &self.build_configuration.directories.host_prefix,
                    build_prefix,
                    Some(jinja),
                    None,
                    self.build_configuration.debug,
                )
                .await
                .into_diagnostic()?;
        }

        // Collect new files and save cache
        let new_files = Files::from_prefix(
            self.prefix(),
            &cache_output.build.always_include_files,
            &cache_output.build.files,
        )
        .into_diagnostic()?;

        fs::create_dir_all(&cache_dir).into_diagnostic()?;
        let prefix_cache_dir = cache_dir.join("prefix");
        fs::create_dir_all(&prefix_cache_dir).into_diagnostic()?;

        let mut copied_files = Vec::new();
        let copy_options = CopyOptions::default();
        let mut creation_cache = HashSet::new();

        // Track files that contain the old prefix for later path rewriting
        let mut files_with_prefix: Vec<PathBuf> = Vec::new();
        let mut binary_files_with_prefix: Vec<PathBuf> = Vec::new();

        for file in &new_files.new_files {
            if file.is_dir() && !file.is_symlink() {
                continue;
            }
            let stripped = file.strip_prefix(self.prefix()).unwrap();
            let dest = prefix_cache_dir.join(stripped);
            copy_file(file, &dest, &mut creation_cache, &copy_options).into_diagnostic()?;
            copied_files.push(stripped.to_path_buf());
            let (has_prefix, is_text) = check_file_for_prefix(file, self.prefix());

            if has_prefix {
                match is_text {
                    true => files_with_prefix.push(stripped.to_path_buf()),
                    false => binary_files_with_prefix.push(stripped.to_path_buf()),
                }
            }
        }

        let work_dir_files = CopyDir::new(
            &self.build_configuration.directories.work_dir,
            &cache_dir.join("work_dir"),
        )
        .run()
        .into_diagnostic()?;

        let cache = Cache {
            requirements: requirements.clone(),
            finalized_dependencies: finalized_dependencies.clone(),
            finalized_sources: rendered_sources.clone(),
            prefix_files: copied_files,
            work_dir_files: work_dir_files.copied_paths().to_vec(),
            prefix: self.prefix().to_path_buf(),
            work_dir: self.build_configuration.directories.work_dir.clone(),
            run_exports: cache_output.run_exports.clone(),
            files_with_prefix,
            binary_files_with_prefix,
            files_with_work_dir: {
                let mut files = Vec::new();
                for rel in work_dir_files.copied_paths() {
                    let abs = self.build_configuration.directories.work_dir.join(rel);
                    if abs.is_dir() {
                        continue;
                    }
                    match contains_prefix_text(&abs, &self.build_configuration.directories.work_dir)
                    {
                        Ok(Some(_)) => files.push(rel.to_path_buf()),
                        Ok(None) => {}
                        Err(_) => {}
                    }
                }
                files
            },
            source_files: work_dir_files.copied_paths().to_vec(),
        };

        let cache_json = serde_json::to_string(&cache).into_diagnostic()?;
        fs::write(cache_dir.join("cache.json"), cache_json).into_diagnostic()?;

        // The files are already in PREFIX from the build script, so we don't need to restore them.
        // However, we need to track them so subsequent cache builds that inherit from this one
        // know which files are available (e.g., extended-cache inheriting from base-cache).
        // The files will remain in PREFIX for the next cache build in the sequence.

        let mut all_restored_prefix_files = self.restored_cache_prefix_files.unwrap_or_default();
        all_restored_prefix_files.extend(cache.prefix_files.clone());

        let mut all_restored_work_dir_files =
            self.restored_cache_work_dir_files.unwrap_or_default();
        all_restored_work_dir_files.extend(cache.work_dir_files.clone());

        Ok(Output {
            finalized_cache_dependencies: Some(finalized_dependencies),
            finalized_cache_sources: Some(rendered_sources),
            restored_cache_prefix_files: Some(all_restored_prefix_files),
            restored_cache_work_dir_files: Some(all_restored_work_dir_files),
            ..self
        })
    }

    /// This will fetch sources and build the cache if it doesn't exist
    /// Note: this modifies the output in place
    pub(crate) async fn build_or_fetch_cache(
        self,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<Self, miette::Error> {
        if let Some(synthetic_cache) = self.recipe.synthetic_cache_output() {
            // Convert to synthetic cache output
            self.build_or_fetch_cache_output(&synthetic_cache, tool_configuration)
                .await
        } else {
            Ok(self)
        }
    }

    /// Didn't remove this one completely just in case.
    #[allow(dead_code)]
    async fn build_or_fetch_cache_legacy(
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

            let finalized_dependencies = resolve_dependencies(
                &cache.requirements,
                &self,
                &channels,
                tool_configuration,
                RunExportsDownload::DownloadMissing,
            )
            .await
            .into_diagnostic()?;

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
            let mut files_with_prefix: Vec<PathBuf> = Vec::new();
            let mut binary_files_with_prefix: Vec<PathBuf> = Vec::new();

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
                let (has_prefix, is_text) = check_file_for_prefix(file, self.prefix());
                if has_prefix {
                    match is_text {
                        true => files_with_prefix.push(stripped.to_path_buf()),
                        false => binary_files_with_prefix.push(stripped.to_path_buf()),
                    }
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
                finalized_sources: rendered_sources.clone(),
                prefix_files: copied_files,
                work_dir_files: work_dir_files.copied_paths().to_vec(),
                prefix: self.prefix().to_path_buf(),
                work_dir: self.build_configuration.directories.work_dir.clone(),
                run_exports: crate::recipe::parser::RunExports::default(),
                files_with_prefix,
                binary_files_with_prefix,
                files_with_work_dir: Vec::new(),
                source_files: work_dir_files.copied_paths().to_vec(),
            };

            let cache_file = cache_dir.join("cache.json");
            let cache_json = serde_json::to_string(&cache).into_diagnostic()?;
            fs::write(cache_file, cache_json).into_diagnostic()?;

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
