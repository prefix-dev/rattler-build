#![deny(missing_docs)]

//! rattler-build library.

pub mod build;
pub mod bump_recipe;
// pub mod cache;
// pub mod conda_build_config;
pub mod console_utils;
pub mod metadata;
pub mod opt;
pub mod package_test;
pub mod packaging;
pub mod render;
pub mod script;
pub mod source;
pub mod staging;
pub mod system_tools;
pub mod tool_configuration;

// Re-export recipe generator
#[cfg(feature = "recipe-generation")]
pub use rattler_build_recipe_generator as recipe_generator;
#[cfg(feature = "tui")]
pub mod tui;
pub mod types;
pub mod utils;

mod consts;
pub mod env_vars;
mod linux;
mod macos;
mod package_info;
mod post_process;
pub mod publish;
pub mod rebuild;
mod unix;
mod windows;

mod package_cache_reporter;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{Arc, Mutex},
};

use build::{WorkingDirectoryBehavior, run_build, skip_existing};
use console_utils::LoggingOutputHandler;
use dunce::canonicalize;
use fs_err as fs;
use futures::FutureExt;
use miette::{Context, IntoDiagnostic};
use opt::*;
use package_test::TestConfiguration;
use rattler_build_recipe::{
    stage0,
    stage1::{Recipe, TestType},
    variant_render::{RenderConfig, render_recipe_with_variant_config},
};
use rattler_build_variant_config::VariantConfig;

// Re-export types needed by Python bindings and external consumers
pub use rattler_build_jinja::Variable;
pub use rattler_build_recipe::stage1::build::BuildString;
pub use rattler_build_recipe::stage1::{HashInfo, HashInput};
pub use rattler_build_types::NormalizedKey;
use rattler_conda_types::{
    MatchSpec, NamedChannelOrUrl, PackageName, Platform, compression_level::CompressionLevel,
    package::CondaArchiveType,
};
use rattler_config::config::build::PackageFormatAndCompression;
use rattler_index::ensure_channel_initialized_fs;
#[cfg(feature = "s3")]
use rattler_index::ensure_channel_initialized_s3;
use rattler_solve::SolveStrategy;
use rattler_virtual_packages::VirtualPackageOverrides;
use render::resolved_dependencies::RunExportsDownload;
use source::patch::apply_patch_custom;
use system_tools::SystemTools;
use tool_configuration::{Configuration, ContinueOnFailure, SkipExisting, TestStrategy};
use types::Directories;
use types::{
    BuildConfiguration, BuildSummary, PackageIdentifier, PackagingSettings,
    build_reindexed_channels,
};

use crate::metadata::{Debug, Output, PlatformWithVirtualPackages};
use crate::publish::{
    BuildNumberOverride, apply_build_number_override, fetch_highest_build_numbers,
    upload_and_index_channel,
};
use indexmap::IndexSet;
use rattler_conda_types::NoArchType;

/// A discovered output from variant expansion
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct DiscoveredOutput {
    pub name: String,
    pub version: String,
    pub build_string: String,
    pub noarch_type: NoArchType,
    pub target_platform: Platform,
    pub used_vars: BTreeMap<NormalizedKey, Variable>,
    pub recipe: Recipe,
    pub hash: HashInfo,
}

impl Eq for DiscoveredOutput {}

impl PartialEq for DiscoveredOutput {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.build_string == other.build_string
            && self.noarch_type == other.noarch_type
            && self.target_platform == other.target_platform
    }
}

impl std::hash::Hash for DiscoveredOutput {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.version.hash(state);
        self.build_string.hash(state);
        self.noarch_type.hash(state);
        self.target_platform.hash(state);
    }
}

/// Result of finding variants, including the top-level recipe name if available
struct FoundVariants {
    outputs: IndexSet<DiscoveredOutput>,
    /// Top-level recipe name from multi-output recipes (if set and concrete)
    recipe_name: Option<String>,
}

/// Find all variants from the recipe and variant config
fn find_variants(
    variant_config: &VariantConfig,
    recipe_path: &std::path::Path,
    recipe_content: &str,
    target_platform: Platform,
    build_platform: Platform,
    host_platform: Platform,
    experimental: bool,
) -> Result<FoundVariants, miette::Error> {
    // Parse the recipe
    let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_content)
        .map_err(|e| {
            let source = rattler_build_recipe::source_code::Source::from_string(
                recipe_path.display().to_string(),
                recipe_content.to_string(),
            );
            // Use ParseErrorWithSource for better span highlighting
            let error_with_source = rattler_build_recipe::ParseErrorWithSource::new(source, e);
            miette::Report::new(error_with_source)
        })
        .wrap_err("Failed to parse recipe")?;

    // Extract the top-level recipe name from multi-output recipes (if concrete)
    let recipe_name = match &stage0_recipe {
        stage0::Recipe::MultiOutput(multi) => multi
            .recipe
            .name
            .as_ref()
            .and_then(|v| v.as_concrete())
            .map(|name| name.0.as_normalized().to_string()),
        stage0::Recipe::SingleOutput(_) => None,
    };

    // Get OS environment variable keys that can be overridden by variant config
    // We use an empty prefix path since we just need the keys, not the values
    let os_env_var_keys = env_vars::os_vars(&std::path::PathBuf::new(), &host_platform)
        .keys()
        .cloned()
        .collect();

    // Build render config with platform information, experimental flag, and recipe path
    let render_config = RenderConfig::new()
        .with_target_platform(target_platform)
        .with_build_platform(build_platform)
        .with_host_platform(host_platform)
        .with_experimental(experimental)
        .with_recipe_path(recipe_path)
        .with_os_env_var_keys(os_env_var_keys);

    // Render with variant config (handles both single and multi-output recipes)
    let rendered_variants =
        render_recipe_with_variant_config(&stage0_recipe, variant_config, render_config)
            .map_err(|e| {
                let source = miette::NamedSource::new(
                    recipe_path.display().to_string(),
                    recipe_content.to_string(),
                );
                miette::Report::new(e).with_source_code(source)
            })
            .wrap_err("Failed to render recipe with variants")?;

    // Apply topological sorting to ensure correct build order for multi-output recipes
    let rendered_variants =
        rattler_build_recipe::variant_render::topological_sort_variants(rendered_variants)
            .map_err(|e| miette::miette!("{}", e))?;

    // Convert to DiscoveredOutputs
    let mut recipes = IndexSet::new();
    for rendered in rendered_variants {
        let recipe = rendered.recipe;
        let variant = rendered.variant;

        let effective_target_platform = if recipe.build().noarch.is_none() {
            target_platform
        } else {
            Platform::NoArch
        };

        // The recipe has already been evaluated and has its build string resolved
        // (including proper variant filtering for noarch python)
        // Extract the build string and hash from the already-evaluated recipe
        let build_string = recipe
            .build()
            .string
            .as_resolved()
            .expect("Recipe build string should be resolved after evaluation");

        recipes.insert(DiscoveredOutput {
            name: recipe.package().name().as_source().to_string(),
            version: recipe.package().version().to_string(),
            build_string: build_string.to_string(),
            noarch_type: recipe.build().noarch.unwrap_or(NoArchType::none()),
            target_platform: effective_target_platform,
            used_vars: variant,
            recipe,
            hash: rendered.hash_info.expect("Should be set after evaluation"),
        });
    }

    Ok(FoundVariants {
        outputs: recipes,
        recipe_name,
    })
}

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
        #[cfg(feature = "s3")]
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
        .with_allow_symlinks_on_windows(build_data.allow_symlinks_on_windows)
        .with_allow_absolute_license_paths(build_data.allow_absolute_license_paths)
        .with_io_concurrency_limit(Some(build_data.io_concurrency_limit))
        .with_zstd_repodata_enabled(build_data.common.use_zstd)
        .with_bz2_repodata_enabled(build_data.common.use_bz2)
        .with_sharded_repodata_enabled(build_data.common.use_sharded)
        .with_jlap_enabled(build_data.common.use_jlap);

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

    tracing::debug!(
        "Platforms: build: {}, host: {}, target: {}",
        build_data.build_platform,
        build_data.host_platform,
        build_data.target_platform
    );

    let span = tracing::info_span!("Finding outputs from recipe");
    let enter = span.enter();

    // Read the recipe content
    let recipe_content = fs::read_to_string(recipe_path).into_diagnostic()?;

    // Check if there is a `variants.yaml` or `conda_build_config.yaml` file next to
    // the recipe that we should potentially use.
    let mut detected_variant_config = None;

    // find either variants_config_file or conda_build_config_file automatically
    for file in [
        consts::VARIANTS_CONFIG_FILE,
        consts::CONDA_BUILD_CONFIG_FILE,
    ] {
        if let Some(variant_path) = recipe_path.parent().map(|parent| parent.join(file))
            && variant_path.is_file()
        {
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
        };
    }

    // If `-m foo.yaml` is passed as variant config, we should use that instead of
    // the auto-detected one. For that reason we add them to the end of the list.
    let mut variant_configs = detected_variant_config.unwrap_or_default();
    variant_configs.extend(build_data.variant_config.clone());

    let mut variant_config =
        VariantConfig::from_files(&variant_configs, build_data.target_platform).map_err(|e| {
            // Check if this is a ParseError with a file path
            if let rattler_build_variant_config::VariantConfigError::ParseError { path, source } =
                &e
            {
                // Read the file to provide source code context
                if let Ok(content) = fs_err::read_to_string(path) {
                    let source_code = rattler_build_recipe::source_code::Source::from_string(
                        path.display().to_string(),
                        content,
                    );
                    let error_with_source = rattler_build_recipe::ParseErrorWithSource::new(
                        source_code,
                        source.clone(),
                    );
                    return miette::Report::new(error_with_source);
                }
            }
            // Fallback to original error if we can't provide source context
            miette::Report::new(e)
        })?;

    // Warn if target_platform is set in variant config - it's not supported and will be ignored
    if variant_config.variants.contains_key(&"target_platform".into()) {
        tracing::warn!(
            "Setting 'target_platform' in a variant config file is not supported and will be ignored. \
            Please use the '--target-platform' command-line flag to specify the target platform."
        );
    }

    // Always insert target_platform and build_platform
    variant_config.variants.insert(
        "target_platform".into(),
        vec![Variable::from(build_data.target_platform.to_string())],
    );
    variant_config.variants.insert(
        "build_platform".into(),
        vec![Variable::from(build_data.build_platform.to_string())],
    );

    // Apply variant overrides from command line
    for (key, values) in &build_data.variant_overrides {
        let normalized_key = NormalizedKey::from(key.as_str());
        let variables: Vec<Variable> = values.iter().map(|v| Variable::from_string(v)).collect();
        variant_config.variants.insert(normalized_key, variables);
    }

    let FoundVariants {
        outputs: outputs_and_variants,
        recipe_name,
    } = find_variants(
        &variant_config,
        recipe_path,
        &recipe_content,
        build_data.target_platform,
        build_data.build_platform,
        build_data.host_platform,
        build_data.common.experimental,
    )?;

    tracing::info!("Found {} variants\n", outputs_and_variants.len());
    for discovered_output in &outputs_and_variants {
        let skipped = if discovered_output.recipe.build().skip {
            console::style(" (skipped)").red().to_string()
        } else {
            String::new()
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

    // For multi-output recipes, all outputs (including staging caches) need to use the same
    // build directory so that paths are consistent across outputs.
    // Use the top-level recipe name if available, otherwise fall back to the first output name.
    let global_build_name = recipe_name
        .or_else(|| outputs_and_variants.first().map(|o| o.name.clone()))
        .unwrap_or_else(|| "build".to_string());

    for discovered_output in outputs_and_variants {
        let recipe = &discovered_output.recipe;

        // Check if this build should be skipped based on skip conditions
        if recipe.build().skip {
            tracing::info!(
                "Skipping {} {} - skip conditions evaluated to true",
                recipe.package().name().as_normalized(),
                recipe.package().version()
            );
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

        // Use the global build name for outputs that inherit from staging caches
        // This ensures staging caches and their dependent packages share the same build directory
        // Otherwise, use the output's own name for the build directory
        let build_name = if recipe.inherits_from.is_some() {
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
        let virtual_package_override = VirtualPackageOverrides::from_env();
        let output = Output {
            recipe: discovered_output.recipe.clone(),
            build_configuration: BuildConfiguration {
                target_platform: discovered_output.target_platform,
                host_platform: PlatformWithVirtualPackages::detect_for_platform(
                    build_data.host_platform,
                    &virtual_package_override,
                )
                .into_diagnostic()?,
                build_platform: PlatformWithVirtualPackages::detect_for_platform(
                    build_data.build_platform,
                    &virtual_package_override,
                )
                .into_diagnostic()?,
                hash: discovered_output.hash.clone(),
                variant: discovered_output.used_vars.clone(),
                directories: Directories::builder(
                    &build_name,
                    recipe_path,
                    &output_dir,
                    &timestamp,
                )
                .no_build_id(build_data.no_build_id)
                .merge_build_and_host(recipe.build().merge_build_and_host_envs)
                .skip_directory_creation(build_data.render_only)
                .build()
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
                exclude_newer: build_data.exclude_newer,
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

    // Override build numbers if --build-num was specified
    if let Some(build_num_override) = build_data.build_num_override {
        tracing::info!(
            "Overriding build number to {} for all outputs",
            build_num_override
        );
        for output in &mut outputs {
            // Update the build number
            output.recipe.build.number = Some(build_num_override);

            // Extract the hash from the current build string and recompute with new build number
            // Build string format is: {hash}_{build_number}
            let current_build_string = output
                .recipe
                .build
                .string
                .as_resolved()
                .expect("Build string should be resolved at this point");

            // Split on last '_' to separate hash from build number
            // TODO should we fail if we do not have a "standard" build string with build number at the end?
            if let Some(last_underscore) = current_build_string.rfind('_') {
                let hash_part = &current_build_string[..last_underscore];
                let new_build_string = format!("{}_{}", hash_part, build_num_override);
                output.recipe.build.string = BuildString::Resolved(new_build_string);
            }
        }
    }

    Ok(outputs)
}

fn can_test(output: &Output, all_output_names: &[&PackageName], done_outputs: &[Output]) -> bool {
    let check_if_matches = |spec: &MatchSpec, output: &Output| -> bool {
        if spec.name.as_ref()
            != Some(&rattler_conda_types::PackageNameMatcher::Exact(
                output.name().clone(),
            ))
        {
            return false;
        }
        if let Some(version_spec) = &spec.version
            && !version_spec.matches(output.recipe.package().version())
        {
            return false;
        }
        if let Some(build_string_spec) = &spec.build
            && !build_string_spec.matches(&output.build_string())
        {
            return false;
        }
        true
    };

    // Check if any run dependencies are not built yet
    if let Some(ref deps) = output.finalized_dependencies {
        for dep in &deps.run.depends {
            if all_output_names.iter().any(|o| {
                Some(&rattler_conda_types::PackageNameMatcher::Exact(
                    (*o).clone(),
                )) == dep.spec().name.as_ref()
            }) {
                // this dependency might not be built yet
                if !done_outputs.iter().any(|o| check_if_matches(dep.spec(), o)) {
                    return false;
                }
            }
        }
    }

    // Also check that for all script tests
    for test in output.recipe.tests() {
        if let TestType::Commands(command) = test {
            for dep in command
                .requirements
                .build
                .iter()
                .chain(command.requirements.run.iter())
            {
                let dep_name = dep.name();
                if all_output_names.iter().any(|o| Some(*o) == dep_name) {
                    // this dependency might not be built yet
                    // For pin_subpackage/pin_compatible, we only check name match
                    // For regular specs, we also check version/build if specified
                    let is_built = match dep {
                        rattler_build_recipe::stage1::Dependency::Spec(spec) => {
                            done_outputs.iter().any(|o| check_if_matches(spec, o))
                        }
                        _ => {
                            // For pins, just check if any output with that name is built
                            done_outputs.iter().any(|o| Some(o.name()) == dep_name)
                        }
                    };
                    if !is_built {
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
    tracing::info!("Starting build of {} outputs", outputs_to_build.len());
    for (index, output) in outputs_to_build.iter().enumerate() {
        let (output, archive) = match run_build(
            output.clone(),
            &tool_configuration,
            WorkingDirectoryBehavior::Cleanup,
        )
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
                        exclude_newer: output.build_configuration.exclude_newer,
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

                        // Reindex the output directory so that the broken package is no longer
                        // listed in the repodata. This is important for --skip-existing to work
                        // correctly on subsequent builds.
                        if let Err(e) = build_reindexed_channels(
                            &output.build_configuration,
                            &tool_configuration,
                        )
                        .await
                        {
                            tracing::warn!(
                                "Failed to reindex output directory after moving package to broken folder: {}",
                                e
                            );
                        }

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
                #[cfg(feature = "s3")]
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
        exclude_newer: None,
    };

    let package_name = package_file
        .file_name()
        .ok_or_else(|| miette::miette!("Could not get file name from package file"))?
        .to_string_lossy()
        .to_string();

    let span = tracing::info_span!("Running tests for", package = %package_name, span_color = package_name);
    let _enter = span.enter();
    package_test::run_test(&package_file, &test_options, None)
        .await
        .into_diagnostic()?;

    Ok(())
}

/// Result of rebuilding a package.
#[derive(Debug, Clone)]
pub struct RebuildOutput {
    /// Path to the original package
    pub original_path: PathBuf,
    /// Path to the rebuilt package
    pub rebuilt_path: PathBuf,
    /// SHA256 hash of the original package (hex-encoded)
    pub original_sha256: String,
    /// SHA256 hash of the rebuilt package (hex-encoded)
    pub rebuilt_sha256: String,
}

impl RebuildOutput {
    /// Returns true if the original and rebuilt packages are bit-for-bit identical.
    pub fn is_identical(&self) -> bool {
        self.original_sha256 == self.rebuilt_sha256
    }
}

/// Core rebuild logic that extracts the recipe from a package and rebuilds it.
///
/// This function is the reusable core of the rebuild functionality, returning
/// the result data for programmatic use (e.g., Python bindings).
pub async fn rebuild_package_core(
    rebuild_data: RebuildData,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<RebuildOutput> {
    let reqwest_client = tool_configuration::reqwest_client_from_auth_storage(
        rebuild_data.common.auth_file,
        #[cfg(feature = "s3")]
        rebuild_data.common.s3_config.clone(),
        rebuild_data.common.mirror_config.clone(),
        rebuild_data.common.allow_insecure_host.clone(),
    )
    .into_diagnostic()?;

    // Check if the input is a URL or local path
    let (_temp_dir_guard, package_path) = match rebuild_data.package_file {
        PackageSource::Url(ref url) => {
            // Download the package to a temporary location
            tracing::info!("Downloading package from {}", url);

            let response = reqwest_client
                .get_client()
                .get(url.as_str())
                .send()
                .await
                .into_diagnostic()?;

            if !response.status().is_success() {
                miette::bail!("Failed to download package: HTTP {}", response.status());
            }

            // Extract filename from URL or use a default
            let Some(filename) = url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|s| s.to_string())
            else {
                miette::bail!("Failed to extract filename from URL: {}", url);
            };

            let temp_dir = tempfile::tempdir().into_diagnostic()?;
            let package_path = temp_dir.path().join(filename);

            let bytes = response.bytes().await.into_diagnostic()?;
            fs::write(&package_path, &bytes).into_diagnostic()?;

            tracing::info!("Downloaded package to: {:?}", package_path);

            // Keep the temp directory alive for the duration
            (Some(temp_dir), package_path)
        }
        PackageSource::Path(ref path) => {
            // Use the local path directly
            (None, path.clone())
        }
    };

    // Calculate SHA256 of the original package
    let original_sha = rattler_digest::compute_file_digest::<rattler_digest::Sha256>(&package_path)
        .into_diagnostic()?;

    tracing::info!("Original package SHA256: {:x}", original_sha);
    tracing::info!("Rebuilding \"{}\"", package_path.display());

    // we extract the recipe folder from the package file (info/recipe/*)
    // and then run the rendered recipe with the same arguments as the original
    // build
    let temp_folder = tempfile::tempdir().into_diagnostic()?;

    rebuild::extract_recipe(&package_path, temp_folder.path()).into_diagnostic()?;

    let temp_dir = temp_folder.keep();

    tracing::info!("Extracted recipe to: {:?}", temp_dir);

    let rendered_recipe =
        fs::read_to_string(temp_dir.join("rendered_recipe.yaml")).into_diagnostic()?;

    let mut output: Output = serde_yaml::from_str(&rendered_recipe).into_diagnostic()?;

    // set recipe dir to the temp folder
    output.build_configuration.directories.recipe_dir = temp_dir;

    // Use a temporary directory for the build output to avoid overwriting the original
    let temp_output_dir = tempfile::tempdir().into_diagnostic()?;
    let temp_output_path = temp_output_dir.path().to_path_buf();

    fs::create_dir_all(&temp_output_path).into_diagnostic()?;
    output.build_configuration.directories.output_dir = temp_output_path.clone();

    let tool_config = Configuration::builder()
        .with_logging_output_handler(fancy_log_handler)
        .with_keep_build(true)
        .with_compression_threads(rebuild_data.compression_threads)
        .with_reqwest_client(reqwest_client)
        .with_test_strategy(rebuild_data.test)
        .finish();

    output
        .build_configuration
        .directories
        .recreate_directories()
        .into_diagnostic()?;

    let (rebuilt_output, temp_rebuilt_path) =
        run_build(output, &tool_config, WorkingDirectoryBehavior::Cleanup).await?;

    // Generate timestamp for the rebuilt package
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");

    // Create final output directory
    let final_output_dir = rebuild_data.common.output_dir.clone();
    fs::create_dir_all(&final_output_dir).into_diagnostic()?;

    // Insert timestamp before the extension
    let new_filename = format!(
        "{}-{}-{}-rebuilt-{timestamp}{}",
        rebuilt_output.name().as_normalized(),
        rebuilt_output.version(),
        rebuilt_output.build_string(),
        rebuilt_output
            .build_configuration
            .packaging_settings
            .archive_type
            .extension()
    );

    let rebuilt_path = final_output_dir.join(&new_filename);

    // Move the rebuilt package to final location with new name
    // Use copy+remove as fallback for cross-device moves
    if let Err(e) = fs::rename(&temp_rebuilt_path, &rebuilt_path) {
        if e.kind() == std::io::ErrorKind::CrossesDevices {
            fs::copy(&temp_rebuilt_path, &rebuilt_path).into_diagnostic()?;
            fs::remove_file(&temp_rebuilt_path).into_diagnostic()?;
        } else {
            return Err(e).into_diagnostic();
        }
    }

    // Now we can drop the temp directory
    drop(temp_output_dir);

    // Calculate SHA256 of the rebuilt package
    let rebuilt_sha = rattler_digest::compute_file_digest::<rattler_digest::Sha256>(&rebuilt_path)
        .into_diagnostic()?;

    tracing::info!("Rebuilt package SHA256: {:x}", rebuilt_sha);
    tracing::info!("Rebuilt package saved to: \"{:?}\"", rebuilt_path);

    Ok(RebuildOutput {
        original_path: package_path,
        rebuilt_path,
        original_sha256: format!("{:x}", original_sha),
        rebuilt_sha256: format!("{:x}", rebuilt_sha),
    })
}

/// Rebuild a package from its embedded recipe (CLI entry point).
///
/// This function wraps [`rebuild_package_core`] and adds interactive features
/// like diffoscope comparison prompts that are suitable for CLI use.
pub async fn rebuild(
    rebuild_data: RebuildData,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    let result = rebuild_package_core(rebuild_data, fancy_log_handler).await?;

    // Compare the SHA hashes
    if result.is_identical() {
        tracing::info!(
            "✅ Rebuild successful! SHA256 hashes match. Packages are bit-for-bit identical!"
        );
    } else {
        tracing::warn!("❌ Rebuild produced different output! SHA256 hashes do not match.");
        tracing::info!("❌ Rebuild produced different output!");
        tracing::info!("  Original SHA256: {}", result.original_sha256);
        tracing::info!("  Rebuilt SHA256:  {}", result.rebuilt_sha256);
        tracing::info!("  Rebuilt package: {}", result.rebuilt_path.display());

        // Check if diffoscope is available
        let diffoscope_available = Command::new("diffoscope").arg("--version").output().is_ok();

        if diffoscope_available {
            // In interactive mode, ask the user; in CI/non-TTY, run automatically
            let should_run = if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
                dialoguer::Confirm::new()
                    .with_prompt("Do you want to run diffoscope?")
                    .interact()
                    .unwrap_or(true)
            } else {
                true
            };

            if should_run {
                let mut command = Command::new("diffoscope");
                command
                    .arg(&result.original_path)
                    .arg(&result.rebuilt_path)
                    .arg("--text-color")
                    .arg("always");

                tracing::info!("Running diffoscope: {:?}", command);

                let output = command.output().into_diagnostic()?;

                tracing::info!("{}", String::from_utf8_lossy(&output.stdout));
                if !output.stderr.is_empty() {
                    tracing::info!("{}", String::from_utf8_lossy(&output.stderr));
                }
            }
        } else {
            tracing::info!("\nHint: Install diffoscope to see detailed differences:");
            tracing::info!("  pixi global install diffoscope");
        }
    }

    Ok(())
}

// /// Sort the build outputs (recipes) topologically based on their dependencies.
// pub fn sort_build_outputs_topologically(
//     outputs: &mut Vec<Output>,
//     up_to: Option<&str>,
// ) -> miette::Result<()> {
//     let mut graph = DiGraph::<usize, ()>::new();
//     // Store all node indices for each package name (multiple variants produce same name)
//     let mut name_to_indices: HashMap<PackageName, Vec<NodeIndex>> = HashMap::new();
//     // Also store direct mapping from output index to node index
//     let mut output_to_node: Vec<NodeIndex> = Vec::with_capacity(outputs.len());

//     // Index outputs by their produced names for quick lookup
//     for (idx, output) in outputs.iter().enumerate() {
//         let node_idx = graph.add_node(idx);
//         output_to_node.push(node_idx);
//         name_to_indices
//             .entry(output.name().clone())
//             .or_default()
//             .push(node_idx);
//     }

//     // Add edges based on dependencies
//     for (output_idx, output) in outputs.iter().enumerate() {
//         let output_node = output_to_node[output_idx];
//         for dep in output.recipe.requirements().build_host() {
//             let dep_name: Option<PackageName> = match dep {
//                 Dependency::Spec(spec) => spec.name.clone().and_then(|matcher| {
//                     use rattler_conda_types::PackageNameMatcher;
//                     match matcher {
//                         PackageNameMatcher::Exact(name) => Some(name),
//                         _ => None,
//                     }
//                 }),
//                 Dependency::PinSubpackage(pin) => Some(pin.pin_subpackage.name.clone()),
//                 Dependency::PinCompatible(pin) => Some(pin.pin_compatible.name.clone()),
//             };

//             if let Some(dep_name) = dep_name
//                 && let Some(dep_nodes) = name_to_indices.get(&dep_name)
//             {
//                 // Add edge to ALL variants of the dependency package
//                 for &dep_node in dep_nodes {
//                     // do not point to self (circular dependency) - this can happen with
//                     // pin_subpackage in run_exports, for example.
//                     if output_node == dep_node {
//                         continue;
//                     }
//                     graph.add_edge(output_node, dep_node, ());
//                 }
//             }
//         }
//     }

//     let sorted_indices = if let Some(up_to) = up_to {
//         // Find the node indices for the "up-to" package (may have multiple variants)
//         let up_to_name = PackageName::from_str(up_to)
//             .map_err(|_| miette::miette!("Invalid package name: '{}'", up_to))?;
//         let up_to_indices = name_to_indices.get(&up_to_name).ok_or_else(|| {
//             miette::miette!("The package '{}' was not found in the outputs", up_to)
//         })?;

//         // Perform DFS post-order traversal from ALL variants of the "up-to" package
//         let mut sorted_indices = Vec::new();
//         let mut visited = HashSet::new();
//         for &up_to_index in up_to_indices {
//             let mut dfs = DfsPostOrder::new(&graph, up_to_index);
//             while let Some(nx) = dfs.next(&graph) {
//                 if visited.insert(nx) {
//                     sorted_indices.push(nx);
//                 }
//             }
//         }

//         sorted_indices
//     } else {
//         // Perform topological sort
//         let mut sorted_indices = toposort(&graph, None).map_err(|cycle| {
//             let node = cycle.node_id();
//             let name = outputs[node.index()].name();
//             miette::miette!("Cycle detected in dependencies: {}", name.as_source())
//         })?;
//         sorted_indices.reverse();
//         sorted_indices
//     };

//     sorted_indices
//         .iter()
//         .map(|idx| &outputs[idx.index()])
//         .for_each(|output| {
//             tracing::debug!("Ordered output: {:?}", output.name().as_normalized());
//         });

//     // Reorder outputs based on the sorted indices
//     *outputs = sorted_indices
//         .iter()
//         .map(|node| outputs[node.index()].clone())
//         .collect();

//     Ok(())
// }

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
        tracing::info!(
            "Processing recipe at path: {}",
            recipe_path.canonicalize().unwrap().display()
        );
        let output = get_build_output(&build_data, recipe_path, &tool_config).await?;
        outputs.extend(output);
    }

    if build_data.render_only {
        // Sort outputs topologically even in render-only mode to show expected build order
        // TODO(refactor): figure out if this is still needed
        // sort_build_outputs_topologically(&mut outputs, build_data.up_to.as_deref())?;

        let outputs = if build_data.with_solve {
            let mut updated_outputs = Vec::new();
            for output in outputs {
                updated_outputs.push(
                    output
                        .resolve_dependencies(&tool_config, RunExportsDownload::SkipDownload)
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

    // sort_build_outputs_topologically(&mut outputs, build_data.up_to.as_deref())?;
    run_build_from_args(outputs, tool_config).await?;

    Ok(())
}

/// Build all outputs and collect the package paths
async fn build_and_collect_packages(
    build_output: Vec<Output>,
    tool_configuration: &Configuration,
) -> miette::Result<Vec<PathBuf>> {
    let mut package_paths = Vec::new();
    let outputs_to_build = skip_existing(build_output, tool_configuration).await?;

    for output in outputs_to_build.iter() {
        let (_output, archive) = match run_build(
            output.clone(),
            tool_configuration,
            WorkingDirectoryBehavior::Cleanup,
        )
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
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        package_paths.push(archive);
    }

    Ok(package_paths)
}

/// Publish packages to a channel.
///
/// This function builds packages from recipes, uploads them to a specified channel,
/// and runs indexing on the channel.
pub async fn publish_packages(
    publish_data: PublishData,
    log_handler: &Option<console_utils::LoggingOutputHandler>,
) -> Result<(), miette::Error> {
    // Create tool configuration for cache clearing and building
    let tool_config = get_tool_config(&publish_data.build, log_handler)?;

    // Convert target to a channel URL
    let target_url = publish_data.to.clone();
    let channel_url = target_url
        .clone()
        .into_base_url(&tool_config.channel_config)
        .into_diagnostic()?;

    // Ensure the channel is initialized based on its type
    match channel_url.url().scheme() {
        "file" => {
            let dir = channel_url
                .url()
                .to_file_path()
                .map_err(|()| miette::miette!("Invalid file URL: {}", channel_url.url()))?;
            if !dir.exists() {
                tracing::info!(
                    "Creating initial index for local channel at {}",
                    dir.display()
                );

                fs::create_dir_all(&dir).into_diagnostic()?;

                ensure_channel_initialized_fs(&dir).await.map_err(|e| {
                    miette::miette!(
                        "Failed to initialize local channel at {}: {}",
                        dir.display(),
                        e
                    )
                })?;
            } else {
                // check if it is a valid channel by looking for `noarch/repodata.json` file
                let noarch_repodata = dir.join("noarch").join("repodata.json");
                if !noarch_repodata.exists() {
                    return Err(miette::miette!(
                        "The specified local channel at {} is not initialized (missing {}). Please initialize the channel first.",
                        dir.display(),
                        noarch_repodata.display()
                    ));
                }
            }
        }
        #[cfg(feature = "s3")]
        "s3" => {
            // Resolve S3 credentials and ensure the channel is initialized
            let resolved_s3_credentials = tool_configuration::resolve_s3_credentials(
                &publish_data.build.common.s3_config,
                publish_data.build.common.auth_file.clone(),
                channel_url.url(),
            )
            .await
            .into_diagnostic()?;

            ensure_channel_initialized_s3(channel_url.as_ref(), &resolved_s3_credentials)
                .await
                .map_err(|e| miette::miette!("Failed to initialize S3 channel: {}", e))?;
        }
        // Remote channels (http/https, quetz, prefix, etc.) handle initialization on the server side
        _ => {}
    }

    // Check if we're publishing pre-built packages or building from recipes
    let built_packages = if !publish_data.package_files.is_empty() {
        // Publish pre-built packages directly
        tracing::info!(
            "Publishing {} pre-built package(s)",
            publish_data.package_files.len()
        );

        // Validate that all package files exist
        for package_file in &publish_data.package_files {
            if !package_file.exists() {
                return Err(miette::miette!(
                    "Package file does not exist: {}",
                    package_file.display()
                ));
            }
        }

        publish_data.package_files.clone()
    } else {
        // Build packages from recipes
        let mut outputs = Vec::new();

        // Expand recipe paths (handles directories by finding all recipes within them)
        let mut expanded_recipe_paths = Vec::new();
        for recipe_path in &publish_data.recipe_paths {
            if recipe_path.is_dir() {
                // For directories, scan for all recipes
                for entry in ignore::Walk::new(recipe_path) {
                    let entry = entry.into_diagnostic()?;
                    if entry.path().is_dir()
                        && let Ok(resolved_path) = get_recipe_path(entry.path())
                    {
                        expanded_recipe_paths.push(resolved_path);
                    }
                }
            } else {
                // For files, resolve directly (handles recipe.yaml in directory or direct yaml files)
                let resolved_path = get_recipe_path(recipe_path)?;
                expanded_recipe_paths.push(resolved_path);
            }
        }
        // Sort to ensure deterministic ordering across platforms/filesystems
        expanded_recipe_paths.sort();

        for recipe_path in &expanded_recipe_paths {
            let output = get_build_output(&publish_data.build, recipe_path, &tool_config).await?;
            outputs.extend(output);
        }

        // Apply build number override if specified
        if let Some(ref build_number_arg) = publish_data.build_number {
            let build_number_override = BuildNumberOverride::parse(build_number_arg)?;

            // For relative bumps, we need to fetch the highest build numbers from the target channel
            let highest_build_numbers = match &build_number_override {
                BuildNumberOverride::Relative(_) => {
                    fetch_highest_build_numbers(
                        &target_url,
                        &outputs,
                        publish_data.build.target_platform,
                        &tool_config,
                    )
                    .await?
                }
                BuildNumberOverride::Absolute(num) => {
                    tracing::info!("Setting build number to {} for all outputs", num);
                    HashMap::new()
                }
            };

            apply_build_number_override(
                &mut outputs,
                &build_number_override,
                &highest_build_numbers,
            );
        }

        if publish_data.build.render_only {
            let outputs = if publish_data.build.with_solve {
                let mut updated_outputs = Vec::new();
                for output in outputs {
                    updated_outputs.push(
                        output
                            .resolve_dependencies(&tool_config, RunExportsDownload::SkipDownload)
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

        // sort_build_outputs_topologically(&mut outputs, publish_data.build.up_to.as_deref())?;

        // Build all packages and collect the paths
        let built_packages = build_and_collect_packages(outputs, &tool_config).await?;

        if built_packages.is_empty() {
            tracing::info!("No packages were built");
            return Ok(());
        }

        built_packages
    };

    upload_and_index_channel(
        &target_url,
        &built_packages,
        &publish_data,
        &tool_config.repodata_gateway,
    )
    .await?;

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
        variant_overrides: HashMap::new(),
        ignore_recipe_variants: false,
        render_only: false,
        with_solve: true,
        no_build_id: false,
        package_format: PackageFormatAndCompression {
            archive_type: CondaArchiveType::Conda,
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
        allow_absolute_license_paths: false,
        exclude_newer: None,
        build_num_override: None,
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
            .resolve_dependencies(&tool_config, RunExportsDownload::DownloadMissing)
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

/// Display information about a built package
pub fn show_package_info(args: InspectOpts) -> miette::Result<()> {
    package_info::package_info(args)
}

/// Extract a conda package to a directory
pub async fn extract_package(args: opt::ExtractOpts) -> miette::Result<()> {
    package_info::extract_package(args).await
}
