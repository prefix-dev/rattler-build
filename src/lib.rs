#![deny(missing_docs)]

//! rattler-build library.

pub mod build;
pub mod cache;
pub mod conda_build_config;
pub mod console_utils;
pub mod metadata;
mod normalized_key;
pub mod opt;
pub mod package_test;
pub mod packaging;
pub mod recipe;
pub mod render;
pub mod script;
pub mod selectors;
pub mod source;
pub mod system_tools;
pub mod tool_configuration;
#[cfg(feature = "tui")]
pub mod tui;
mod url_with_trailing_slash;
pub mod used_variables;
pub mod utils;
pub mod variant_config;
mod variant_render;

mod consts;
mod env_vars;
pub mod hash;
mod linux;
mod macos;
mod post_process;
pub mod rebuild;
#[cfg(feature = "recipe-generation")]
pub mod recipe_generator;
mod run_exports;
mod unix;
pub mod upload;
mod windows;

mod package_cache_reporter;
pub mod source_code;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use build::{run_build, skip_existing};
use console_utils::LoggingOutputHandler;
use dunce::canonicalize;
use fs_err as fs;
use futures::FutureExt;
use metadata::{
    BuildConfiguration, BuildSummary, Directories, Output, PackageIdentifier, PackagingSettings,
    build_reindexed_channels,
};
use miette::{Context, IntoDiagnostic};
pub use normalized_key::NormalizedKey;
use opt::*;
use package_test::TestConfiguration;
use petgraph::{algo::toposort, graph::DiGraph, visit::DfsPostOrder};
use rattler_conda_types::{
    GenericVirtualPackage, MatchSpec, NamedChannelOrUrl, PackageName, Platform,
    compression_level::CompressionLevel, package::ArchiveType,
};
use rattler_config::config::build::PackageFormatAndCompression;
use rattler_solve::SolveStrategy;
use rattler_virtual_packages::{VirtualPackage, VirtualPackageOverrides};
use recipe::parser::{Dependency, TestType, find_outputs_from_src};
use selectors::SelectorConfig;
use source::patch::apply_patch_custom;
use source_code::Source;
use system_tools::SystemTools;
use tool_configuration::{Configuration, ContinueOnFailure, SkipExisting, TestStrategy};
use variant_config::VariantConfig;

use crate::metadata::Debug;
use crate::metadata::PlatformWithVirtualPackages;

/// Returns the recipe path.
pub fn get_recipe_path(path: &Path) -> miette::Result<PathBuf> {
    let recipe_path = canonicalize(path);
    if let Err(e) = &recipe_path {
        match e.kind() {
            std::io::ErrorKind::NotFound => {
                return Err(miette::miette!(
                    "The file {} could not be found.",
                    path.to_string_lossy()
                ));
            }
            std::io::ErrorKind::PermissionDenied => {
                return Err(miette::miette!(
                    "Permission denied when trying to access the file {}.",
                    path.to_string_lossy()
                ));
            }
            _ => {
                return Err(miette::miette!(
                    "An unknown error occurred while trying to access the file {}: {:?}",
                    path.to_string_lossy(),
                    e
                ));
            }
        }
    }
    let mut recipe_path = recipe_path.into_diagnostic()?;

    // If the recipe_path is a directory, look for "recipe.yaml" in the directory.
    if recipe_path.is_dir() {
        let recipe_yaml_path = recipe_path.join("recipe.yaml");
        if recipe_yaml_path.exists() && recipe_yaml_path.is_file() {
            recipe_path = recipe_yaml_path;
        } else {
            return Err(miette::miette!(
                "'recipe.yaml' not found in the directory {}",
                path.to_string_lossy()
            ));
        }
    }

    Ok(recipe_path)
}

/// Returns the tool configuration.
pub fn get_tool_config(
    build_data: &BuildData,
    fancy_log_handler: &Option<LoggingOutputHandler>,
) -> miette::Result<Configuration> {
    let client = tool_configuration::reqwest_client_from_auth_storage(
        build_data.common.auth_file.clone(),
        build_data.common.s3_config.clone(),
        build_data.common.mirror_config.clone(),
        build_data.common.allow_insecure_host.clone(),
    )
    .into_diagnostic()?;

    let configuration_builder = Configuration::builder()
        .with_keep_build(build_data.keep_build)
        .with_compression_threads(build_data.compression_threads)
        .with_reqwest_client(client)
        .with_test_strategy(build_data.test)
        .with_skip_existing(build_data.skip_existing)
        .with_continue_on_failure(build_data.continue_on_failure)
        .with_noarch_build_platform(build_data.noarch_build_platform)
        .with_channel_priority(build_data.common.channel_priority)
        .with_allow_insecure_host(build_data.common.allow_insecure_host.clone())
        .with_error_prefix_in_binary(build_data.error_prefix_in_binary)
        .with_allow_symlinks_on_windows(build_data.allow_symlinks_on_windows);

    let configuration_builder = if let Some(fancy_log_handler) = fancy_log_handler {
        configuration_builder.with_logging_output_handler(fancy_log_handler.clone())
    } else {
        configuration_builder
    };

    Ok(configuration_builder.finish())
}

/// Returns the output for the build.
pub async fn get_build_output(
    build_data: &BuildData,
    recipe_path: &Path,
    tool_config: &Configuration,
) -> miette::Result<Vec<Output>> {
    let mut output_dir = build_data.common.output_dir.clone();
    if output_dir.exists() {
        output_dir = canonicalize(&output_dir).into_diagnostic()?;
    }

    if build_data.target_platform == Platform::NoArch
        || build_data.build_platform == Platform::NoArch
    {
        return Err(miette::miette!(
            "target-platform / build-platform cannot be `noarch` - that should be defined in the recipe"
        ));
    }

    // Determine virtual packages of the system. These packages define the
    // capabilities of the system. Some packages depend on these virtual
    // packages to indicate compatibility with the hardware of the system.
    let virtual_packages = tool_config
        .fancy_log_handler
        .wrap_in_progress("determining virtual packages", move || {
            VirtualPackage::detect(&VirtualPackageOverrides::from_env()).map(|vpkgs| {
                vpkgs
                    .iter()
                    .map(|vpkg| GenericVirtualPackage::from(vpkg.clone()))
                    .collect::<Vec<_>>()
            })
        })
        .into_diagnostic()?;

    tracing::debug!(
        "Platforms: build: {}, host: {}, target: {}",
        build_data.build_platform,
        build_data.host_platform,
        build_data.target_platform
    );

    let selector_config = SelectorConfig {
        // We ignore noarch here
        target_platform: build_data.target_platform,
        host_platform: build_data.host_platform,
        hash: None,
        build_platform: build_data.build_platform,
        variant: BTreeMap::new(),
        experimental: build_data.common.experimental,
        // allow undefined while finding the variants
        allow_undefined: true,
        recipe_path: Some(recipe_path.to_path_buf()),
    };

    let span = tracing::info_span!("Finding outputs from recipe");
    let enter = span.enter();

    // First find all outputs from the recipe
    let named_source = Source::from_path(recipe_path).into_diagnostic()?;
    let outputs = find_outputs_from_src(named_source.clone())?;

    // Check if there is a `variants.yaml` or `conda_build_config.yaml` file next to
    // the recipe that we should potentially use.
    let mut detected_variant_config = None;

    // find either variants_config_file or conda_build_config_file automatically
    for file in [
        consts::VARIANTS_CONFIG_FILE,
        consts::CONDA_BUILD_CONFIG_FILE,
    ] {
        if let Some(variant_path) = recipe_path.parent().map(|parent| parent.join(file)) {
            if variant_path.is_file() {
                if !build_data.ignore_recipe_variants {
                    let mut configs = build_data.variant_config.clone();
                    configs.push(variant_path);
                    detected_variant_config = Some(configs);
                } else {
                    tracing::debug!(
                        "Ignoring variants from {} because \"--ignore-recipe-variants\" was specified",
                        variant_path.display()
                    );
                }
                break;
            }
        };
    }

    // If `-m foo.yaml` is passed as variant config, we should use that instead of
    // the auto-detected one. For that reason we add them to the end of the list.
    let mut variant_configs = detected_variant_config.unwrap_or_default();
    variant_configs.extend(build_data.variant_config.clone());

    let variant_config = VariantConfig::from_files(&variant_configs, &selector_config)?;

    let outputs_and_variants =
        variant_config.find_variants(&outputs, named_source, &selector_config)?;

    tracing::info!("Found {} variants\n", outputs_and_variants.len());
    for discovered_output in &outputs_and_variants {
        let skipped = if discovered_output.recipe.build().skip() {
            console::style(" (skipped)").red().to_string()
        } else {
            "".to_string()
        };

        tracing::info!(
            "\nBuild variant: {}-{}-{}{}",
            discovered_output.name,
            discovered_output.version,
            discovered_output.build_string,
            skipped
        );

        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(["Variant", "Version"]);
        for (key, value) in discovered_output.used_vars.iter() {
            table.add_row([key.normalize(), format!("{:?}", value)]);
        }
        tracing::info!("\n{}\n", table);
    }
    drop(enter);

    let mut subpackages = BTreeMap::new();
    let mut outputs = Vec::new();

    let global_build_name = outputs_and_variants
        .first()
        .map(|o| o.name.clone())
        .unwrap_or_default();

    for discovered_output in outputs_and_variants {
        let recipe = &discovered_output.recipe;

        if recipe.build().skip() {
            continue;
        }

        subpackages.insert(
            recipe.package().name().clone(),
            PackageIdentifier {
                name: recipe.package().name().clone(),
                version: recipe.package().version().clone(),
                build_string: discovered_output.build_string.clone(),
            },
        );

        let build_name = if recipe.cache.is_some() {
            global_build_name.clone()
        } else {
            recipe.package().name().as_normalized().to_string()
        };

        let variant_channels = if let Some(channel_sources) = discovered_output
            .used_vars
            .get(&NormalizedKey("channel_sources".to_string()))
        {
            Some(
                channel_sources
                    .to_string()
                    .split(',')
                    .map(str::trim)
                    .map(|s| NamedChannelOrUrl::from_str(s).into_diagnostic())
                    .collect::<miette::Result<Vec<_>>>()?,
            )
        } else {
            None
        };

        // priorities
        // 1. channel_sources from variant file
        // 2. channels from args
        // 3. channels from pixi_config
        // 4. conda-forge as fallback
        if variant_channels.is_some() && build_data.channels.is_some() {
            return Err(miette::miette!(
                "channel_sources and channels cannot both be set at the same time"
            ));
        }
        let channels = variant_channels.unwrap_or_else(|| {
            build_data
                .channels
                .clone()
                .unwrap_or(vec![NamedChannelOrUrl::Name("conda-forge".to_string())])
        });

        let channels = channels
            .into_iter()
            .map(|c| c.into_base_url(&tool_config.channel_config))
            .collect::<Result<Vec<_>, _>>()
            .into_diagnostic()?;

        let timestamp = chrono::Utc::now();

        let output = metadata::Output {
            recipe: recipe.clone(),
            build_configuration: BuildConfiguration {
                target_platform: discovered_output.target_platform,
                host_platform: PlatformWithVirtualPackages {
                    platform: build_data.host_platform,
                    virtual_packages: virtual_packages.clone(),
                },
                build_platform: PlatformWithVirtualPackages {
                    platform: build_data.build_platform,
                    virtual_packages: virtual_packages.clone(),
                },
                hash: discovered_output.hash.clone(),
                variant: discovered_output.used_vars.clone(),
                directories: Directories::setup(
                    &build_name,
                    recipe_path,
                    &output_dir,
                    build_data.no_build_id,
                    &timestamp,
                    recipe.build().merge_build_and_host_envs(),
                )
                .into_diagnostic()?,
                channels,
                channel_priority: tool_config.channel_priority,
                solve_strategy: SolveStrategy::Highest,
                timestamp,
                subpackages: subpackages.clone(),
                packaging_settings: PackagingSettings::from_args(
                    build_data.package_format.archive_type,
                    build_data.package_format.compression_level,
                ),
                store_recipe: !build_data.no_include_recipe,
                force_colors: build_data.color_build_log && console::colors_enabled(),
                sandbox_config: build_data.sandbox_configuration.clone(),
                debug: build_data.debug,
            },
            finalized_dependencies: None,
            finalized_sources: None,
            finalized_cache_dependencies: None,
            finalized_cache_sources: None,
            system_tools: SystemTools::new(),
            build_summary: Arc::new(Mutex::new(BuildSummary::default())),
            extra_meta: Some(
                build_data
                    .extra_meta
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            ),
        };

        outputs.push(output);
    }

    Ok(outputs)
}

fn can_test(output: &Output, all_output_names: &[&PackageName], done_outputs: &[Output]) -> bool {
    let check_if_matches = |spec: &MatchSpec, output: &Output| -> bool {
        if spec.name.as_ref() != Some(output.name()) {
            return false;
        }
        if let Some(version_spec) = &spec.version {
            if !version_spec.matches(output.recipe.package().version()) {
                return false;
            }
        }
        if let Some(build_string_spec) = &spec.build {
            if !build_string_spec.matches(&output.build_string()) {
                return false;
            }
        }
        true
    };

    // Check if any run dependencies are not built yet
    if let Some(ref deps) = output.finalized_dependencies {
        for dep in &deps.run.depends {
            if all_output_names
                .iter()
                .any(|o| Some(*o) == dep.spec().name.as_ref())
            {
                // this dependency might not be built yet
                if !done_outputs.iter().any(|o| check_if_matches(dep.spec(), o)) {
                    return false;
                }
            }
        }
    }

    // Also check that for all script tests
    for test in output.recipe.tests() {
        if let TestType::Command(command) = test {
            for dep in command
                .requirements
                .build
                .iter()
                .chain(command.requirements.run.iter())
            {
                let dep_spec: MatchSpec = dep.parse().expect("Could not parse MatchSpec");
                if all_output_names
                    .iter()
                    .any(|o| Some(*o) == dep_spec.name.as_ref())
                {
                    // this dependency might not be built yet
                    if !done_outputs.iter().any(|o| check_if_matches(&dep_spec, o)) {
                        return false;
                    }
                }
            }
        }
    }

    true
}

/// Runs build.
pub async fn run_build_from_args(
    build_output: Vec<Output>,
    tool_configuration: Configuration,
) -> miette::Result<()> {
    let mut outputs = Vec::new();
    let mut test_queue = Vec::new();

    let outputs_to_build = skip_existing(build_output, &tool_configuration).await?;

    let all_output_names = outputs_to_build
        .iter()
        .map(|o| o.name())
        .collect::<Vec<_>>();

    for (index, output) in outputs_to_build.iter().enumerate() {
        let (output, archive) = match run_build(output.clone(), &tool_configuration)
            .boxed_local()
            .await
        {
            Ok((output, archive)) => {
                output.record_build_end();
                (output, archive)
            }
            Err(e) => {
                if tool_configuration.continue_on_failure == ContinueOnFailure::Yes {
                    tracing::error!("Build failed for {}: {}", output.identifier(), e);
                    output.record_warning(&format!("Build failed: {}", e));
                    continue;
                }
                return Err(e);
            }
        };

        outputs.push(output.clone());

        // We can now run the tests for the output. However, we need to check if
        // all dependencies that are needed for the test are already built.

        // Decide whether the tests should be skipped or not
        let (skip_test, skip_test_reason) = match tool_configuration.test_strategy {
            TestStrategy::Skip => (true, "the argument --test=skip was set".to_string()),
            TestStrategy::Native => {
                // Skip if `host_platform != build_platform` and `target_platform != noarch`
                if output.build_configuration.target_platform != Platform::NoArch
                    && output.build_configuration.host_platform.platform
                        != output.build_configuration.build_platform.platform
                {
                    let reason = format!(
                        "the argument --test=native was set and the build is a cross-compilation (target_platform={}, build_platform={}, host_platform={})",
                        output.build_configuration.target_platform,
                        output.build_configuration.build_platform.platform,
                        output.build_configuration.host_platform.platform
                    );

                    (true, reason)
                } else {
                    (false, "".to_string())
                }
            }
            TestStrategy::NativeAndEmulated => (false, "".to_string()),
        };
        if skip_test {
            tracing::info!("Skipping tests because {}", skip_test_reason);
            build_reindexed_channels(&output.build_configuration, &tool_configuration)
                .await
                .into_diagnostic()
                .context("failed to reindex output channel")?;
        } else {
            test_queue.push((output, archive));

            let is_last_iteration = index == outputs_to_build.len() - 1;
            let to_test = if is_last_iteration {
                // On last iteration, test everything in the queue
                std::mem::take(&mut test_queue)
            } else {
                // Update the test queue with the tests that we can't run yet
                let (to_test, new_test_queue) = test_queue
                    .into_iter()
                    .partition(|(output, _)| can_test(output, &all_output_names, &outputs));
                test_queue = new_test_queue;
                to_test
            };

            for (output, archive) in &to_test {
                match package_test::run_test(
                    archive,
                    &TestConfiguration {
                        test_prefix: output
                            .build_configuration
                            .directories
                            .output_dir
                            .join("test"),
                        target_platform: Some(output.build_configuration.target_platform),
                        host_platform: Some(output.build_configuration.host_platform.clone()),
                        current_platform: output.build_configuration.build_platform.clone(),
                        keep_test_prefix: tool_configuration.no_clean,
                        channels: build_reindexed_channels(
                            &output.build_configuration,
                            &tool_configuration,
                        )
                        .await
                        .into_diagnostic()
                        .context("failed to reindex output channel")?,
                        channel_priority: tool_configuration.channel_priority,
                        solve_strategy: SolveStrategy::Highest,
                        tool_configuration: tool_configuration.clone(),
                        test_index: None,
                        output_dir: output.build_configuration.directories.output_dir.clone(),
                        debug: output.build_configuration.debug,
                    },
                    None,
                )
                .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        // move the package file to the failed directory
                        let failed_dir = output
                            .build_configuration
                            .directories
                            .output_dir
                            .join("broken");
                        fs::create_dir_all(&failed_dir).into_diagnostic()?;
                        fs::rename(archive, failed_dir.join(archive.file_name().unwrap()))
                            .into_diagnostic()?;

                        if tool_configuration.continue_on_failure == ContinueOnFailure::Yes {
                            tracing::error!("Test failed for {}: {}", output.identifier(), e);
                            output.record_warning(&format!("Test failed: {}", e));
                        } else {
                            return Err(miette::miette!("Test failed: {}", e));
                        }
                    }
                }
            }
        }
    }

    let span = tracing::info_span!("Build summary");
    let _enter = span.enter();
    for output in outputs {
        // print summaries for each output
        let _ = output.log_build_summary().map_err(|e| {
            tracing::error!("Error writing build summary: {}", e);
            e
        });
    }

    Ok(())
}

/// Check if the noarch builds should be skipped because the noarch platform has
/// been set
pub async fn skip_noarch(
    mut outputs: Vec<Output>,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<Vec<Output>> {
    if let Some(noarch_build_platform) = tool_configuration.noarch_build_platform {
        outputs.retain(|output| {
            // Skip the build if:
            // - target_platform is "noarch"
            // and
            // - build_platform != noarch_build_platform
            let should_skip = output.build_configuration.target_platform == Platform::NoArch
                && output.build_configuration.build_platform.platform != noarch_build_platform;

            if should_skip {
                // The identifier should always be set at this point
                tracing::info!(
                    "Skipping build because noarch_build_platform is set to {} for {}",
                    noarch_build_platform,
                    output.identifier()
                );
            }
            !should_skip
        });
    }

    Ok(outputs)
}

/// Runs test.
pub async fn run_test(
    test_data: TestData,
    fancy_log_handler: Option<LoggingOutputHandler>,
) -> miette::Result<()> {
    let package_file = canonicalize(test_data.package_file).into_diagnostic()?;

    let mut tool_config_builder = Configuration::builder();

    // Determine virtual packages of the system. These packages define the
    // capabilities of the system. Some packages depend on these virtual
    // packages to indicate compatibility with the hardware of the system.
    let current_platform = if let Some(fancy_log_handler) = fancy_log_handler {
        tool_config_builder =
            tool_config_builder.with_logging_output_handler(fancy_log_handler.clone());

        fancy_log_handler
            .wrap_in_progress("determining virtual packages", move || {
                PlatformWithVirtualPackages::detect(&VirtualPackageOverrides::from_env())
            })
            .into_diagnostic()?
    } else {
        PlatformWithVirtualPackages::detect(&VirtualPackageOverrides::from_env())
            .into_diagnostic()?
    };

    let tool_config = tool_config_builder
        .with_keep_build(true)
        .with_compression_threads(test_data.compression_threads)
        .with_reqwest_client(
            tool_configuration::reqwest_client_from_auth_storage(
                test_data.common.auth_file,
                test_data.common.s3_config,
                test_data.common.mirror_config,
                test_data.common.allow_insecure_host.clone(),
            )
            .into_diagnostic()?,
        )
        .with_channel_priority(test_data.common.channel_priority)
        .finish();

    let channels = test_data
        .channels
        .unwrap_or(vec![NamedChannelOrUrl::Name("conda-forge".to_string())]);
    let channels = channels
        .into_iter()
        .map(|c| c.into_base_url(&tool_config.channel_config))
        .collect::<Result<Vec<_>, _>>()
        .into_diagnostic()?;

    let tempdir = tempfile::tempdir().into_diagnostic()?;

    let test_options = TestConfiguration {
        test_prefix: tempdir.path().to_path_buf(),
        target_platform: None,
        host_platform: None,
        current_platform,
        keep_test_prefix: false,
        test_index: test_data.test_index,
        channels,
        channel_priority: tool_config.channel_priority,
        solve_strategy: SolveStrategy::Highest,
        tool_configuration: tool_config,
        output_dir: test_data.common.output_dir,
        debug: test_data.debug,
    };

    let package_name = package_file
        .file_name()
        .ok_or_else(|| miette::miette!("Could not get file name from package file"))?
        .to_string_lossy()
        .to_string();

    let span = tracing::info_span!("Running tests for", package = %package_name);
    let _enter = span.enter();
    package_test::run_test(&package_file, &test_options, None)
        .await
        .into_diagnostic()?;

    Ok(())
}

/// Rebuild.
pub async fn rebuild(
    rebuild_data: RebuildData,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    tracing::info!("Rebuilding {}", rebuild_data.package_file.to_string_lossy());
    // we extract the recipe folder from the package file (info/recipe/*)
    // and then run the rendered recipe with the same arguments as the original
    // build
    let temp_folder = tempfile::tempdir().into_diagnostic()?;

    rebuild::extract_recipe(&rebuild_data.package_file, temp_folder.path()).into_diagnostic()?;

    let temp_dir = temp_folder.keep();

    tracing::info!("Extracted recipe to: {:?}", temp_dir);

    let rendered_recipe =
        fs::read_to_string(temp_dir.join("rendered_recipe.yaml")).into_diagnostic()?;

    let mut output: metadata::Output = serde_yaml::from_str(&rendered_recipe).into_diagnostic()?;

    // set recipe dir to the temp folder
    output.build_configuration.directories.recipe_dir = temp_dir;

    // create output dir and set it in the config
    let output_dir = rebuild_data.common.output_dir;

    fs::create_dir_all(&output_dir).into_diagnostic()?;
    output.build_configuration.directories.output_dir =
        canonicalize(output_dir).into_diagnostic()?;

    let tool_config = Configuration::builder()
        .with_logging_output_handler(fancy_log_handler)
        .with_keep_build(true)
        .with_compression_threads(rebuild_data.compression_threads)
        .with_reqwest_client(
            tool_configuration::reqwest_client_from_auth_storage(
                rebuild_data.common.auth_file,
                rebuild_data.common.s3_config.clone(),
                rebuild_data.common.mirror_config.clone(),
                rebuild_data.common.allow_insecure_host.clone(),
            )
            .into_diagnostic()?,
        )
        .with_test_strategy(rebuild_data.test)
        .finish();

    output
        .build_configuration
        .directories
        .recreate_directories()
        .into_diagnostic()?;

    run_build(output, &tool_config).await?;

    Ok(())
}

/// Upload.
pub async fn upload_from_args(args: UploadOpts) -> miette::Result<()> {
    if args.package_files.is_empty() {
        return Err(miette::miette!("No package files were provided."));
    }

    for package_file in &args.package_files {
        if ArchiveType::try_from(package_file).is_none() {
            return Err(miette::miette!(
                "The file {} does not appear to be a conda package.",
                package_file.to_string_lossy()
            ));
        }
    }

    let store = tool_configuration::get_auth_store(args.common.auth_file).into_diagnostic()?;

    match args.server_type {
        ServerType::Quetz(quetz_opts) => {
            let quetz_data = QuetzData::from(quetz_opts);
            upload::upload_package_to_quetz(&store, &args.package_files, quetz_data).await
        }
        ServerType::Artifactory(artifactory_opts) => {
            let artifactory_data = ArtifactoryData::try_from(artifactory_opts)?;

            upload::upload_package_to_artifactory(&store, &args.package_files, artifactory_data)
                .await
        }
        ServerType::Prefix(prefix_opts) => {
            let prefix_data = PrefixData::from(prefix_opts);
            upload::upload_package_to_prefix(&store, &args.package_files, prefix_data).await
        }
        ServerType::Anaconda(anaconda_opts) => {
            let anaconda_data = AnacondaData::from(anaconda_opts);
            upload::upload_package_to_anaconda(&store, &args.package_files, anaconda_data).await
        }
        ServerType::S3(s3_opts) => {
            upload::upload_package_to_s3(
                &store,
                s3_opts.channel,
                s3_opts.endpoint_url,
                s3_opts.region,
                s3_opts.force_path_style,
                s3_opts.access_key_id,
                s3_opts.secret_access_key,
                s3_opts.session_token,
                &args.package_files,
            )
            .await
        }
        ServerType::CondaForge(conda_forge_opts) => {
            let conda_forge_data = CondaForgeData::from(conda_forge_opts);
            upload::conda_forge::upload_packages_to_conda_forge(
                &args.package_files,
                conda_forge_data,
            )
            .await
        }
    }
}

/// Sort the build outputs (recipes) topologically based on their dependencies.
pub fn sort_build_outputs_topologically(
    outputs: &mut Vec<Output>,
    up_to: Option<&str>,
) -> miette::Result<()> {
    let mut graph = DiGraph::<usize, ()>::new();
    let mut name_to_index = HashMap::new();

    // Index outputs by their produced names for quick lookup
    for (idx, output) in outputs.iter().enumerate() {
        let idx = graph.add_node(idx);
        name_to_index.insert(output.name().clone(), idx);
    }

    // Add edges based on dependencies
    for output in outputs.iter() {
        let output_idx = *name_to_index
            .get(output.name())
            .expect("We just inserted it");
        for dep in output.recipe.requirements().run_build_host() {
            let dep_name = match dep {
                Dependency::Spec(spec) => spec
                    .name
                    .clone()
                    .expect("MatchSpec should always have a name"),
                Dependency::PinSubpackage(pin) => pin.pin_value().name.clone(),
                Dependency::PinCompatible(pin) => pin.pin_value().name.clone(),
            };

            if let Some(&dep_idx) = name_to_index.get(&dep_name) {
                // do not point to self (circular dependency) - this can happen with
                // pin_subpackage in run_exports, for example.
                if output_idx == dep_idx {
                    continue;
                }
                graph.add_edge(output_idx, dep_idx, ());
            }
        }
    }

    let sorted_indices = if let Some(up_to) = up_to {
        // Find the node index for the "up-to" package
        let up_to_index = name_to_index.get(up_to).copied().ok_or_else(|| {
            miette::miette!("The package '{}' was not found in the outputs", up_to)
        })?;

        // Perform a DFS post-order traversal from the "up-to" node to find all
        // dependencies
        let mut dfs = DfsPostOrder::new(&graph, up_to_index);
        let mut sorted_indices = Vec::new();
        while let Some(nx) = dfs.next(&graph) {
            sorted_indices.push(nx);
        }

        sorted_indices
    } else {
        // Perform topological sort
        let mut sorted_indices = toposort(&graph, None).map_err(|cycle| {
            let node = cycle.node_id();
            let name = outputs[node.index()].name();
            miette::miette!("Cycle detected in dependencies: {}", name.as_source())
        })?;
        sorted_indices.reverse();
        sorted_indices
    };

    sorted_indices
        .iter()
        .map(|idx| &outputs[idx.index()])
        .for_each(|output| {
            tracing::debug!("Ordered output: {:?}", output.name().as_normalized());
        });

    // Reorder outputs based on the sorted indices
    *outputs = sorted_indices
        .iter()
        .map(|node| outputs[node.index()].clone())
        .collect();

    Ok(())
}

/// Get the version of rattler-build.
pub fn get_rattler_build_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Build rattler-build recipes
pub async fn build_recipes(
    recipe_paths: Vec<std::path::PathBuf>,
    build_data: BuildData,
    log_handler: &Option<console_utils::LoggingOutputHandler>,
) -> Result<(), miette::Error> {
    let tool_config = get_tool_config(&build_data, log_handler)?;
    let mut outputs = Vec::new();
    for recipe_path in &recipe_paths {
        let output = get_build_output(&build_data, recipe_path, &tool_config).await?;
        outputs.extend(output);
    }

    if build_data.render_only {
        let outputs = if build_data.with_solve {
            let mut updated_outputs = Vec::new();
            for output in outputs {
                updated_outputs.push(
                    output
                        .resolve_dependencies(&tool_config)
                        .await
                        .into_diagnostic()?,
                );
            }
            updated_outputs
        } else {
            outputs
        };

        println!(
            "{}",
            serde_json::to_string_pretty(&outputs).into_diagnostic()?
        );
        return Ok(());
    }

    // Skip noarch builds before the topological sort
    outputs = skip_noarch(outputs, &tool_config).await?;

    sort_build_outputs_topologically(&mut outputs, build_data.up_to.as_deref())?;
    run_build_from_args(outputs, tool_config).await?;

    Ok(())
}

/// Debug a recipe by setting up the environment without running the build script
pub async fn debug_recipe(
    debug_data: DebugData,
    log_handler: &Option<LoggingOutputHandler>,
) -> miette::Result<()> {
    let recipe_path = get_recipe_path(&debug_data.recipe_path)?;

    let build_data = BuildData {
        build_platform: debug_data.build_platform,
        target_platform: debug_data.target_platform,
        host_platform: debug_data.host_platform,
        channels: debug_data.channels,
        common: debug_data.common,
        keep_build: true,
        debug: Debug::new(true),
        test: TestStrategy::Skip,
        up_to: None,
        variant_config: Vec::new(),
        ignore_recipe_variants: false,
        render_only: false,
        with_solve: true,
        no_build_id: false,
        package_format: PackageFormatAndCompression {
            archive_type: ArchiveType::Conda,
            compression_level: CompressionLevel::Default,
        },
        compression_threads: None,
        io_concurrency_limit: num_cpus::get(),
        no_include_recipe: false,
        color_build_log: true,
        tui: false,
        skip_existing: SkipExisting::None,
        noarch_build_platform: None,
        extra_meta: None,
        sandbox_configuration: None,
        continue_on_failure: ContinueOnFailure::No,
        error_prefix_in_binary: false,
        allow_symlinks_on_windows: false,
    };

    let tool_config = get_tool_config(&build_data, log_handler)?;

    let mut outputs = get_build_output(&build_data, &recipe_path, &tool_config).await?;

    if let Some(output_name) = &debug_data.output_name {
        let original_count = outputs.len();
        outputs.retain(|output| output.name().as_normalized() == output_name);

        if outputs.is_empty() {
            return Err(miette::miette!(
                "Output with name '{}' not found in recipe. Available outputs: {}",
                output_name,
                original_count
            ));
        }
    } else if outputs.len() > 1 {
        let output_names: Vec<String> = outputs
            .iter()
            .map(|output| output.name().as_normalized().to_string())
            .collect();

        return Err(miette::miette!(
            "Multiple outputs found in recipe ({}). Please specify which output to debug using --output-name. Available outputs: {}",
            outputs.len(),
            output_names.join(", ")
        ));
    }

    tracing::info!("Build and/or host environments created for debugging.");

    for output in outputs {
        output
            .build_configuration
            .directories
            .recreate_directories()
            .into_diagnostic()?;
        let output = output
            .fetch_sources(&tool_config, apply_patch_custom)
            .await
            .into_diagnostic()?;
        let output = output
            .resolve_dependencies(&tool_config)
            .await
            .into_diagnostic()?;
        output
            .install_environments(&tool_config)
            .await
            .into_diagnostic()?;

        output.create_build_script().await.into_diagnostic()?;

        if let Some(deps) = &output.finalized_dependencies {
            if deps.build.is_some() {
                tracing::info!(
                    "\nBuild dependencies available in {}",
                    output
                        .build_configuration
                        .directories
                        .build_prefix
                        .display()
                );
            }
            if deps.host.is_some() {
                tracing::info!(
                    "Host dependencies available in {}",
                    output.build_configuration.directories.host_prefix.display()
                );
            }
        }

        tracing::info!("\nTo run the actual build, use:");
        tracing::info!(
            "rattler-build build --recipe {}",
            output.build_configuration.directories.recipe_path.display()
        );
        tracing::info!("Or run the build script directly with:");
        if cfg!(windows) {
            tracing::info!(
                "cd {} && ./conda_build.bat",
                output.build_configuration.directories.work_dir.display()
            );
        } else {
            tracing::info!(
                "cd {} && ./conda_build.sh",
                output.build_configuration.directories.work_dir.display()
            );
        }
    }

    Ok(())
}
