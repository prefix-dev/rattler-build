//! The build module contains the code for running the build process for a given [`Output`]
use rattler_conda_types::{Channel, MatchSpec, ParseStrictness};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::{fs, vec};

use miette::IntoDiagnostic;
use rattler_index::index;
use rattler_solve::{ChannelPriority, SolveStrategy};

use crate::metadata::Output;
use crate::package_test::TestConfiguration;
use crate::packaging::Files;
use crate::recipe::parser::{Requirements, TestType};
use crate::render::resolved_dependencies::{resolve_dependencies, FinalizedDependencies};
use crate::render::solver::load_repodatas;
use crate::source::copy_dir::{copy_file, CopyOptions};
use crate::{env_vars, package_test, tool_configuration};

/// Check if the build should be skipped because it already exists in any of the channels
pub async fn skip_existing(
    mut outputs: Vec<Output>,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<Vec<Output>> {
    let span = tracing::info_span!("Checking existing builds");
    let _enter = span.enter();

    let only_local = match tool_configuration.skip_existing {
        tool_configuration::SkipExisting::Local => true,
        tool_configuration::SkipExisting::All => false,
        tool_configuration::SkipExisting::None => return Ok(outputs),
    };

    // If we should skip existing builds, check if the build already exists
    let Some(first_output) = outputs.first() else {
        return Ok(outputs);
    };

    let all_channels = first_output.reindex_channels().into_diagnostic()?;

    let match_specs = outputs
        .iter()
        .map(|o| {
            MatchSpec::from_str(o.name().as_normalized(), ParseStrictness::Strict).into_diagnostic()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let channels = if only_local {
        vec![
            Channel::from_directory(&first_output.build_configuration.directories.output_dir)
                .base_url,
        ]
    } else {
        all_channels
    };

    let existing = load_repodatas(
        &channels,
        first_output.host_platform(),
        &match_specs,
        tool_configuration,
    )
    .await
    .map_err(|e| miette::miette!("Failed to load repodata: {e}."))?;

    let existing_set = existing
        .iter()
        .flatten()
        .map(|p| {
            format!(
                "{}-{}-{}",
                p.package_record.name.as_normalized(),
                p.package_record.version,
                p.package_record.build
            )
        })
        .collect::<std::collections::HashSet<_>>();

    // Retain only the outputs that do not exist yet
    outputs.retain(|output| {
        let exists = existing_set.contains(&format!(
            "{}-{}-{}",
            output.name().as_normalized(),
            output.version(),
            output.build_string().unwrap_or_default()
        ));
        if exists {
            // The identifier should always be set at this point
            tracing::info!(
                "Skipping build for {}",
                output.identifier().as_deref().unwrap_or("unknown")
            );
        }
        !exists
    });

    Ok(outputs)
}

/// Run the build for the given output. This will fetch the sources, resolve the dependencies,
/// and execute the build script. Returns the path to the resulting package.
pub async fn run_build(
    output: Output,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<(Output, PathBuf)> {
    if output.build_string().is_none() {
        miette::bail!("Build string is not set for {:?}", output.name());
    }

    output
        .build_configuration
        .directories
        .create_build_dir()
        .into_diagnostic()?;

    let span = tracing::info_span!("Running build for", recipe = output.identifier().unwrap());
    let _enter = span.enter();
    output.record_build_start();

    let directories = output.build_configuration.directories.clone();

    index(
        &directories.output_dir,
        Some(&output.build_configuration.target_platform.clone()),
    )
    .into_diagnostic()?;

    let output = output
        .fetch_sources(tool_configuration)
        .await
        .into_diagnostic()?;

    let output = output.build_or_fetch_cache(tool_configuration).await?;

    let output = output
        .resolve_dependencies(tool_configuration)
        .await
        .into_diagnostic()?;

    output.run_build_script().await.into_diagnostic()?;

    // Package all the new files
    let (result, paths_json) = output
        .create_package(tool_configuration)
        .await
        .into_diagnostic()?;

    output.record_artifact(&result, &paths_json);

    let span = tracing::info_span!("Running package tests");
    let enter = span.enter();

    // We run all the package content tests
    for test in output.recipe.tests() {
        // TODO we could also run each of the (potentially multiple) test scripts and collect the errors
        if let TestType::PackageContents(package_contents) = test {
            package_contents
                .run_test(&paths_json, &output.build_configuration.target_platform)
                .into_diagnostic()?;
        }
    }

    if !tool_configuration.no_clean {
        directories.clean().into_diagnostic()?;
    }

    if tool_configuration.no_test {
        tracing::info!("Skipping tests");
    } else {
        package_test::run_test(
            &result,
            &TestConfiguration {
                test_prefix: directories.work_dir.join("test"),
                target_platform: Some(output.build_configuration.host_platform),
                keep_test_prefix: tool_configuration.no_clean,
                channels: output.reindex_channels().into_diagnostic()?,
                channel_priority: ChannelPriority::Strict,
                solve_strategy: SolveStrategy::Highest,
                tool_configuration: tool_configuration.clone(),
            },
        )
        .await
        .into_diagnostic()?;
    }

    drop(enter);

    if !tool_configuration.no_clean {
        directories.clean().into_diagnostic()?;
    }

    Ok((output, result))
}

///  Cache for a build
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    /// ads
    pub requirements: Requirements,
    /// asd
    pub finalized_dependencies: FinalizedDependencies,
    /// asd
    pub prefix_files: Vec<PathBuf>,
}

impl Output {
    pub(crate) async fn build_or_fetch_cache(
        &self,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<Self, miette::Error> {
        // if we don't have a cache, we need to run the cache build with our current workdir, and then return the cache
        let span = tracing::info_span!("Running cache build");
        let _enter = span.enter();

        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));

        if let Some(cache) = &self.recipe.cache {
            // TODO this is a placeholder for the cache key
            println!("Cache key: {}", self.cache_key().into_diagnostic()?);
            let cache_key = format!("bld_{}", self.cache_key().into_diagnostic()?);

            let cache_dir = self
                .build_configuration
                .directories
                .cache_dir
                .join(cache_key);

            // restore the cache if it exists by copying the files to the prefix
            if cache_dir.exists() {
                tracing::info!("Restoring cache from {:?}", cache_dir);
                let cache: Cache = serde_json::from_str(
                    &fs::read_to_string(cache_dir.join("cache.json")).into_diagnostic()?,
                )
                .unwrap();
                let copy_options = CopyOptions {
                    skip_exist: true,
                    ..Default::default()
                };
                let mut paths_created = HashSet::new();
                for f in &cache.prefix_files {
                    tracing::info!("Restoring: {:?}", f);
                    let dest = &host_prefix.join(f);
                    let source = &cache_dir.join("prefix").join(f);
                    copy_file(source, dest, &mut paths_created, &copy_options).into_diagnostic()?;
                }

                return Ok(Output {
                    finalized_cache_dependencies: Some(cache.finalized_dependencies.clone()),
                    ..self.clone()
                });
            }

            // create directories (would be done by env creation)
            fs::create_dir_all(&self.build_configuration.directories.work_dir).into_diagnostic()?;
            fs::create_dir_all(&self.build_configuration.directories.host_prefix)
                .into_diagnostic()?;
            fs::create_dir_all(&self.build_configuration.directories.build_prefix)
                .into_diagnostic()?;

            let channels = self.reindex_channels().unwrap();

            let finalized_dependencies =
                resolve_dependencies(&cache.requirements, self, &channels, tool_configuration)
                    .await
                    .unwrap();

            cache
                .build
                .script()
                .run_script(
                    env_vars,
                    &self.build_configuration.directories.work_dir,
                    &self.build_configuration.directories.recipe_dir,
                    &self.build_configuration.directories.host_prefix,
                    Some(&self.build_configuration.directories.build_prefix),
                )
                .await
                .into_diagnostic()?;

            // find the new files in the prefix and add them to the cache
            let new_files = Files::from_prefix(
                &self.build_configuration.directories.host_prefix,
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
                let stripped = file
                    .strip_prefix(&host_prefix)
                    .expect("File should be in prefix");
                let dest = &prefix_cache_dir.join(stripped);
                copy_file(file, dest, &mut creation_cache, &copy_options).into_diagnostic()?;
                copied_files.push(stripped.to_path_buf());
            }

            // save the cache
            let cache = Cache {
                requirements: cache.requirements.clone(),
                finalized_dependencies: finalized_dependencies.clone(),
                prefix_files: copied_files,
            };
            let cache_file = cache_dir.join("cache.json");
            fs::write(cache_file, serde_json::to_string(&cache).unwrap()).into_diagnostic()?;
            Ok(Output {
                finalized_cache_dependencies: Some(finalized_dependencies),
                ..self.clone()
            })
        } else {
            Ok(self.clone())
        }
    }
}
