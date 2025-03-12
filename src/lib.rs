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
    sync::{Arc, Mutex},
};

use build::{run_build, skip_existing};
use console_utils::LoggingOutputHandler;
use dunce::canonicalize;
use fs_err as fs;
use futures::FutureExt;
use metadata::{
    build_reindexed_channels, BuildConfiguration, BuildSummary, Directories, Output,
    PackageIdentifier, PackagingSettings,
};
use miette::{Context, IntoDiagnostic};
pub use normalized_key::NormalizedKey;
use opt::*;
use package_test::TestConfiguration;
use petgraph::{algo::toposort, graph::DiGraph, visit::DfsPostOrder};
use rattler_conda_types::{
    package::ArchiveType, Channel, GenericVirtualPackage, MatchSpec, PackageName, Platform,
};
use rattler_solve::SolveStrategy;
use rattler_virtual_packages::{VirtualPackage, VirtualPackageOverrides};
use recipe::parser::{find_outputs_from_src, Dependency, TestType};
use selectors::SelectorConfig;
use source_code::Source;
use system_tools::SystemTools;
use tool_configuration::{Configuration, TestStrategy};
use variant_config::VariantConfig;

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
    let client =
        tool_configuration::reqwest_client_from_auth_storage(build_data.common.auth_file.clone())
            .into_diagnostic()?;

    let configuration_builder = Configuration::builder()
        .with_keep_build(build_data.keep_build)
        .with_compression_threads(build_data.compression_threads)
        .with_reqwest_client(client)
        .with_test_strategy(build_data.test)
        .with_skip_existing(build_data.skip_existing)
        .with_noarch_build_platform(build_data.noarch_build_platform)
        .with_channel_priority(build_data.common.channel_priority);

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
                version: recipe.package().version().version().clone(),
                build_string: discovered_output.build_string.clone(),
            },
        );

        let build_name = if recipe.cache.is_some() {
            global_build_name.clone()
        } else {
            recipe.package().name().as_normalized().to_string()
        };

        // Add the channels from the args and by default always conda-forge
        let channels = build_data
            .channels
            .clone()
            .into_iter()
            .map(|c| Channel::from_str(c, &tool_config.channel_config).map(|c| c.base_url))
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
                    let reason = format!("the argument --test=native was set and the build is a cross-compilation (target_platform={}, build_platform={}, host_platform={})", output.build_configuration.target_platform, output.build_configuration.build_platform.platform, output.build_configuration.host_platform.platform);

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

            // let testable = can_test(&test_queue, &all_output_names, &outputs_to_build);
            for (output, archive) in &to_test {
                package_test::run_test(
                    archive,
                    &TestConfiguration {
                        test_prefix: output.build_configuration.directories.work_dir.join("test"),
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
                    },
                    None,
                )
                .await
                .into_diagnostic()?;
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
            tool_configuration::reqwest_client_from_auth_storage(test_data.common.auth_file)
                .into_diagnostic()?,
        )
        .with_channel_priority(test_data.common.channel_priority)
        .finish();

    let channels = test_data
        .channels
        .into_iter()
        .map(|name| Channel::from_str(name, &tool_config.channel_config).map(|c| c.base_url))
        .collect::<Result<Vec<_>, _>>()
        .into_diagnostic()?;

    let tempdir = tempfile::tempdir().into_diagnostic()?;

    let test_options = TestConfiguration {
        test_prefix: tempdir.path().to_path_buf(),
        target_platform: None,
        host_platform: None,
        current_platform,
        keep_test_prefix: false,
        channels,
        channel_priority: tool_config.channel_priority,
        solve_strategy: SolveStrategy::Highest,
        tool_configuration: tool_config,
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
    args: RebuildData,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    tracing::info!("Rebuilding {}", args.package_file.to_string_lossy());
    // we extract the recipe folder from the package file (info/recipe/*)
    // and then run the rendered recipe with the same arguments as the original
    // build
    let temp_folder = tempfile::tempdir().into_diagnostic()?;

    rebuild::extract_recipe(&args.package_file, temp_folder.path()).into_diagnostic()?;

    let temp_dir = temp_folder.into_path();

    tracing::info!("Extracted recipe to: {:?}", temp_dir);

    let rendered_recipe =
        fs::read_to_string(temp_dir.join("rendered_recipe.yaml")).into_diagnostic()?;

    let mut output: metadata::Output = serde_yaml::from_str(&rendered_recipe).into_diagnostic()?;

    // set recipe dir to the temp folder
    output.build_configuration.directories.recipe_dir = temp_dir;

    // create output dir and set it in the config
    let output_dir = args.common.output_dir;

    fs::create_dir_all(&output_dir).into_diagnostic()?;
    output.build_configuration.directories.output_dir =
        canonicalize(output_dir).into_diagnostic()?;

    let tool_config = Configuration::builder()
        .with_logging_output_handler(fancy_log_handler)
        .with_keep_build(true)
        .with_compression_threads(args.compression_threads)
        .with_reqwest_client(
            tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file)
                .into_diagnostic()?,
        )
        .with_test_strategy(args.test)
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
