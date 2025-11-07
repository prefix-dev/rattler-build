use std::{collections::HashMap, future::Future, path::PathBuf, str::FromStr};

use ::rattler_build::{
    build_recipes, get_rattler_build_version,
    metadata::Debug,
    opt::{BuildData, ChannelPriorityWrapper, CommonData, TestData},
    run_test,
    tool_configuration::{ContinueOnFailure, SkipExisting, TestStrategy},
};
use clap::ValueEnum;
use pyo3::prelude::*;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::{ConfigBase, build::PackageFormatAndCompression};

mod error;
mod jinja_config;
mod platform_types;
mod progress_callback;
mod recipe_generation;
mod render;
mod stage0;
mod stage1;
mod tool_config;
mod tracing_subscriber;
mod upload;
mod variant_config;

use error::RattlerBuildError;
use jinja_config::PyJinjaConfig;

/// Execute async tasks in Python bindings with proper error handling
fn run_async_task<F, R>(future: F) -> PyResult<R>
where
    F: Future<Output = miette::Result<R>>,
{
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| RattlerBuildError::Other(format!("Failed to create async runtime: {}", e)))?;

    Ok(rt.block_on(async { future.await.map_err(RattlerBuildError::from) })?)
}

// Bind the get version function to the Python module
#[pyfunction]
fn get_rattler_build_version_py() -> PyResult<String> {
    Ok(get_rattler_build_version().to_string())
}

#[pyfunction]
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, variant_overrides=None, ignore_recipe_variants=false, render_only=false, with_solve=false, keep_build=false, no_build_id=false, package_format=None, compression_threads=None, io_concurrency_limit=None, no_include_recipe=false, test=None, output_dir=None, auth_file=None, channel_priority=None, skip_existing=None, noarch_build_platform=None, allow_insecure_host=None, continue_on_failure=false, debug=false, error_prefix_in_binary=false, allow_symlinks_on_windows=false, exclude_newer=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
#[allow(clippy::too_many_arguments)]
fn build_recipes_py(
    recipes: Vec<PathBuf>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<PathBuf>>,
    variant_overrides: Option<HashMap<String, Vec<String>>>,
    ignore_recipe_variants: bool,
    render_only: bool,
    with_solve: bool,
    keep_build: bool,
    no_build_id: bool,
    package_format: Option<String>,
    compression_threads: Option<u32>,
    io_concurrency_limit: Option<usize>,
    no_include_recipe: bool,
    test: Option<String>,
    output_dir: Option<PathBuf>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    skip_existing: Option<String>,
    noarch_build_platform: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    continue_on_failure: bool,
    debug: bool,
    error_prefix_in_binary: bool,
    allow_symlinks_on_windows: bool,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    // todo: allow custom config here
    let config = ConfigBase::<()>::default();
    let common = CommonData::new(
        output_dir,
        false,
        auth_file.map(|a| a.into()),
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_jlap,
        use_sharded,
    );
    let build_platform = build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;
    let test = test.map(|t| TestStrategy::from_str(&t, false).unwrap());
    let skip_existing = skip_existing.map(|s| SkipExisting::from_str(&s, false).unwrap());
    let noarch_build_platform = noarch_build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                        .map_err(|e| e.into())
                })
                .collect::<PyResult<_>>()?,
        ),
    };

    let build_data = BuildData::new(
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        variant_overrides.unwrap_or_default(),
        ignore_recipe_variants,
        render_only,
        with_solve,
        keep_build,
        no_build_id,
        package_format,
        compression_threads,
        io_concurrency_limit,
        no_include_recipe,
        test,
        common,
        false, // TUI disabled
        skip_existing,
        noarch_build_platform,
        None, // extra meta
        None, // sandbox configuration
        Debug::new(debug),
        ContinueOnFailure::from(continue_on_failure),
        error_prefix_in_binary,
        allow_symlinks_on_windows,
        exclude_newer,
    );

    run_async_task(async {
        build_recipes(recipes, build_data, &None).await?;
        Ok(())
    })
}

/// Build from already-rendered variants (Stage1 recipes)
///
/// This function takes RenderedVariant objects (from recipe.render()) and builds them
/// directly without needing to write temporary files.
///
/// If tool_config is provided, it will be used instead of the individual parameters.
#[pyfunction]
#[pyo3(signature = (rendered_variants, tool_config=None, output_dir=None, channel=None, progress_callback=None, recipe_path=None, keep_build=false, no_build_id=false, package_format=None, compression_threads=None, io_concurrency_limit=None, no_include_recipe=false, test=None, auth_file=None, channel_priority=None, skip_existing=None, allow_insecure_host=None, continue_on_failure=false, debug=false, _error_prefix_in_binary=false, _allow_symlinks_on_windows=false, exclude_newer=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
#[allow(clippy::too_many_arguments)]
fn build_from_rendered_variants_py(
    rendered_variants: Vec<render::PyRenderedVariant>,
    tool_config: Option<tool_config::PyToolConfiguration>,
    output_dir: Option<PathBuf>,
    channel: Option<Vec<String>>,
    progress_callback: Option<Py<PyAny>>,
    recipe_path: Option<PathBuf>,
    keep_build: bool,
    no_build_id: bool,
    package_format: Option<String>,
    compression_threads: Option<u32>,
    io_concurrency_limit: Option<usize>,
    no_include_recipe: bool,
    test: Option<String>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    skip_existing: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    continue_on_failure: bool,
    debug: bool,
    _error_prefix_in_binary: bool,
    _allow_symlinks_on_windows: bool,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    use ::rattler_build::{
        console_utils::LoggingOutputHandler,
        metadata::{BuildConfiguration, Output, PlatformWithVirtualPackages},
        run_build_from_args,
        system_tools::SystemTools,
        tool_configuration::Configuration,
        types::{BuildSummary, Directories, PackageIdentifier, PackagingSettings},
    };
    use rattler_build_recipe::stage1::HashInfo;
    use rattler_build_types::NormalizedKey;
    use rattler_solve::SolveStrategy;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    // Use provided tool_config or build one from parameters
    let tool_config = if let Some(config) = tool_config {
        config.inner
    } else {
        let channel_priority = channel_priority
            .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
            .transpose()
            .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;

        let config = ConfigBase::<()>::default();
        let channel_config = config.channel_config.clone();
        let _common = CommonData::new(
            output_dir.clone(),
            false,
            auth_file.map(|a| a.into()),
            config,
            channel_priority,
            allow_insecure_host.clone(),
            use_bz2,
            use_zstd,
            use_jlap,
            use_sharded,
        );

        let test_strategy = test.map(|t| TestStrategy::from_str(&t, false).unwrap());
        let skip_existing = skip_existing.map(|s| SkipExisting::from_str(&s, false).unwrap());

        // Use a hidden multi-progress if Python callback is provided to suppress Rust progress bars
        let log_handler = if progress_callback.is_some() {
            use indicatif::MultiProgress;
            // Create a hidden MultiProgress that doesn't render to terminal
            let mp = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
            LoggingOutputHandler::default().with_multi_progress(mp)
        } else {
            LoggingOutputHandler::default()
        };

        Configuration::builder()
            .with_logging_output_handler(log_handler)
            .with_channel_config(channel_config.clone())
            .with_compression_threads(compression_threads)
            .with_io_concurrency_limit(io_concurrency_limit)
            .with_keep_build(keep_build)
            .with_test_strategy(test_strategy.unwrap_or(TestStrategy::Skip))
            .with_zstd_repodata_enabled(use_zstd)
            .with_bz2_repodata_enabled(use_bz2)
            .with_sharded_repodata_enabled(use_sharded)
            .with_jlap_enabled(use_jlap)
            .with_skip_existing(skip_existing.unwrap_or(SkipExisting::None))
            .with_channel_priority(channel_priority.unwrap_or_default())
            .with_continue_on_failure(ContinueOnFailure::from(continue_on_failure))
            .with_allow_insecure_host(allow_insecure_host)
            .finish()
    };

    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;

    let channels = match channel {
        None => vec![NamedChannelOrUrl::Name("conda-forge".to_string())],
        Some(channel) => channel
            .iter()
            .map(|c| {
                NamedChannelOrUrl::from_str(c)
                    .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                    .map_err(|e| e.into())
            })
            .collect::<PyResult<_>>()?,
    };

    // Convert rendered variants to Output objects
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));
    let timestamp = chrono::Utc::now();
    let virtual_package_override = rattler_virtual_packages::VirtualPackageOverrides::from_env();

    let mut outputs = Vec::new();
    let mut subpackages = BTreeMap::new();

    // First pass: collect all subpackage identifiers
    for rendered_variant in &rendered_variants {
        let recipe = &rendered_variant.inner.recipe;
        subpackages.insert(
            recipe.package.name.clone(),
            PackageIdentifier {
                name: recipe.package.name.clone(),
                version: recipe.package.version.clone(),
                build_string: recipe
                    .build
                    .string
                    .as_resolved()
                    .ok_or_else(|| {
                        RattlerBuildError::Other("Build string not resolved".to_string())
                    })?
                    .to_string(),
            },
        );
    }

    // If recipe_path is None, we should not include the recipe in the package
    let effective_no_include_recipe = no_include_recipe || recipe_path.is_none();

    // Create a safe fallback recipe path when None is provided
    // We use a subdirectory in output_dir to avoid copying unrelated files
    let safe_recipe_path = recipe_path
        .clone()
        .unwrap_or_else(|| output_dir.join("_no_recipe").join("recipe.yaml"));

    // Second pass: create Output objects
    for rendered_variant in rendered_variants {
        let recipe = rendered_variant.inner.recipe;
        let variant = rendered_variant.inner.variant;

        // Extract platforms from variant or use current platform
        let target_platform = variant
            .get(&NormalizedKey("target_platform".to_string()))
            .and_then(|v| v.to_string().parse::<Platform>().ok())
            .unwrap_or_else(Platform::current);

        let build_platform = variant
            .get(&NormalizedKey("build_platform".to_string()))
            .and_then(|v| v.to_string().parse::<Platform>().ok())
            .unwrap_or_else(Platform::current);

        let host_platform = variant
            .get(&NormalizedKey("host_platform".to_string()))
            .and_then(|v| v.to_string().parse::<Platform>().ok())
            .unwrap_or_else(Platform::current);

        // Convert channels to base URLs
        let channels_urls = channels
            .iter()
            .map(|c| c.clone().into_base_url(&tool_config.channel_config))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| RattlerBuildError::Other(format!("Channel error: {}", e)))?;

        // Create hash info - use default if not available
        let hash_info = rendered_variant
            .inner
            .hash_info
            .clone()
            .unwrap_or_else(|| HashInfo {
                hash: String::new(),
                prefix: String::new(),
            });

        let build_name = recipe.package.name.as_normalized().to_string();

        let output = Output {
            recipe,
            build_configuration: BuildConfiguration {
                target_platform,
                host_platform: PlatformWithVirtualPackages::detect_for_platform(
                    host_platform,
                    &virtual_package_override,
                )
                .map_err(|e| {
                    RattlerBuildError::Other(format!("Platform detection error: {}", e))
                })?,
                build_platform: PlatformWithVirtualPackages::detect_for_platform(
                    build_platform,
                    &virtual_package_override,
                )
                .map_err(|e| {
                    RattlerBuildError::Other(format!("Platform detection error: {}", e))
                })?,
                hash: hash_info,
                variant,
                directories: Directories::setup(
                    &build_name,
                    &safe_recipe_path,
                    &output_dir,
                    no_build_id,
                    &timestamp,
                    false, // merge_build_and_host_envs - we can infer from recipe if needed
                )
                .map_err(|e| RattlerBuildError::Other(format!("Directory setup error: {}", e)))?,
                channels: channels_urls.clone(),
                channel_priority: tool_config.channel_priority,
                solve_strategy: SolveStrategy::Highest,
                timestamp,
                subpackages: subpackages.clone(),
                packaging_settings: PackagingSettings::from_args(
                    package_format
                        .as_ref()
                        .map(|p| p.archive_type)
                        .unwrap_or(rattler_conda_types::package::ArchiveType::Conda),
                    package_format
                        .as_ref()
                        .map(|p| p.compression_level)
                        .unwrap_or(
                            rattler_conda_types::compression_level::CompressionLevel::Default,
                        ),
                ),
                store_recipe: !effective_no_include_recipe,
                force_colors: false, // Set to false for Python API
                sandbox_config: None,
                debug: ::rattler_build::metadata::Debug::new(debug),
                exclude_newer,
            },
            finalized_dependencies: None,
            finalized_sources: None,
            finalized_cache_dependencies: None,
            finalized_cache_sources: None,
            build_summary: Arc::new(Mutex::new(BuildSummary::default())),
            system_tools: SystemTools::default(),
            extra_meta: None,
        };

        outputs.push(output);
    }

    // Run the build with optional tracing subscriber
    tracing_subscriber::with_python_tracing(progress_callback, || {
        run_async_task(async { run_build_from_args(outputs, tool_config).await })
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (package_file, channel, compression_threads, auth_file, channel_priority, allow_insecure_host=None, debug=false, test_index=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
fn test_package_py(
    package_file: PathBuf,
    channel: Option<Vec<String>>,
    compression_threads: Option<u32>,
    auth_file: Option<PathBuf>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    debug: bool,
    test_index: Option<usize>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    // todo: allow custom config here
    let config = ConfigBase::<()>::default();
    let common = CommonData::new(
        None,
        false,
        auth_file,
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_jlap,
        use_sharded,
    );
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                        .map_err(|e| e.into())
                })
                .collect::<PyResult<_>>()?,
        ),
    };
    let test_data = TestData::new(
        package_file,
        channel,
        compression_threads,
        Debug::new(debug),
        test_index,
        common,
    );

    run_async_task(async {
        run_test(test_data, None).await?;
        Ok(())
    })
}

#[pymodule]
fn rattler_build<'py>(_py: Python<'py>, m: Bound<'py, PyModule>) -> PyResult<()> {
    error::register_exceptions(_py, &m)?;
    m.add_function(wrap_pyfunction!(get_rattler_build_version_py, &m).unwrap())?;
    m.add_function(
        wrap_pyfunction!(recipe_generation::generate_pypi_recipe_string_py, &m).unwrap(),
    )?;
    m.add_function(wrap_pyfunction!(recipe_generation::generate_r_recipe_string_py, &m).unwrap())?;
    m.add_function(
        wrap_pyfunction!(recipe_generation::generate_cpan_recipe_string_py, &m).unwrap(),
    )?;
    m.add_function(
        wrap_pyfunction!(recipe_generation::generate_luarocks_recipe_string_py, &m).unwrap(),
    )?;
    m.add_function(wrap_pyfunction!(build_recipes_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build_from_rendered_variants_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(test_package_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_quetz_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_artifactory_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_prefix_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_anaconda_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_packages_to_conda_forge_py, &m).unwrap())?;
    m.add_class::<PyJinjaConfig>()?;

    // Register all submodules
    stage0::register_stage0_module(_py, &m)?;
    stage1::register_stage1_module(_py, &m)?;
    variant_config::register_variant_config_module(_py, &m)?;
    render::register_render_module(_py, &m)?;
    tool_config::register_tool_config_module(_py, &m)?;
    platform_types::register_platform_types_module(_py, &m)?;

    Ok(())
}
