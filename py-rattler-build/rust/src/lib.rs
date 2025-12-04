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
mod package;
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

/// Result of a successful package build
#[pyclass(name = "BuildResult")]
#[derive(Clone)]
pub struct BuildResultPy {
    /// List of paths to built package files
    #[pyo3(get)]
    pub packages: Vec<PathBuf>,
    /// Package name
    #[pyo3(get)]
    pub name: String,
    /// Package version
    #[pyo3(get)]
    pub version: String,
    /// Build string (hash and variant identifier)
    #[pyo3(get)]
    pub build_string: String,
    /// Target platform (e.g., "linux-64", "noarch")
    #[pyo3(get)]
    pub platform: String,
    /// Dictionary of variant values used for this build
    #[pyo3(get)]
    pub variant: HashMap<String, String>,
    /// Build duration in seconds
    #[pyo3(get)]
    pub build_time: f64,
    /// Captured build log messages (info level and above)
    #[pyo3(get)]
    pub log: Vec<String>,
}

#[pymethods]
impl BuildResultPy {
    fn __repr__(&self) -> String {
        let pkg_count = self.packages.len();
        let pkg_str = if pkg_count == 1 {
            "package"
        } else {
            "packages"
        };
        format!(
            "BuildResult({}={}={}, {} {}, platform={}, time={:.2}s)",
            self.name,
            self.version,
            self.build_string,
            pkg_count,
            pkg_str,
            self.platform,
            self.build_time
        )
    }
}

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
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, variant_overrides=None, ignore_recipe_variants=false, render_only=false, with_solve=false, keep_build=false, no_build_id=false, package_format=None, compression_threads=None, io_concurrency_limit=None, no_include_recipe=false, test=None, output_dir=None, auth_file=None, channel_priority=None, skip_existing=None, noarch_build_platform=None, allow_insecure_host=None, continue_on_failure=false, debug=false, error_prefix_in_binary=false, allow_symlinks_on_windows=false, exclude_newer=None, build_num=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
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
    build_num: Option<u64>,
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
        build_num,
    );

    run_async_task(async {
        build_recipes(recipes, build_data, &None).await?;
        Ok(())
    })
}

/// Build from a single rendered variant (Stage1 recipe)
///
/// This function takes a RenderedVariant object (from recipe.render()) and builds it
/// directly without needing to write temporary files.
#[pyfunction]
fn build_rendered_variant_py(
    rendered_variant: render::PyRenderedVariant,
    tool_config: tool_config::PyToolConfiguration,
    output_dir: PathBuf,
    channels: Vec<String>,
    progress_callback: Option<Py<PyAny>>,
    recipe_path: Option<PathBuf>,
    no_build_id: bool,
    package_format: Option<String>,
    no_include_recipe: bool,
    debug: bool,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
) -> PyResult<BuildResultPy> {
    use ::rattler_build::{
        metadata::{BuildConfiguration, Output, PlatformWithVirtualPackages},
        run_build_from_args,
        system_tools::SystemTools,
        types::{BuildSummary, Directories, PackageIdentifier, PackagingSettings},
    };
    use rattler_build_recipe::stage1::HashInfo;
    use rattler_build_types::NormalizedKey;
    use rattler_solve::SolveStrategy;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    let tool_config = tool_config.inner;

    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;

    let channels: Vec<NamedChannelOrUrl> = channels
        .iter()
        .map(|c| {
            NamedChannelOrUrl::from_str(c)
                .map_err(|e| RattlerBuildError::Channel(e.to_string()))
        })
        .collect::<Result<_, _>>()?;

    // Convert rendered variant to Output object
    let timestamp = chrono::Utc::now();
    let virtual_package_override = rattler_virtual_packages::VirtualPackageOverrides::from_env();

    let mut subpackages = BTreeMap::new();

    // Collect subpackage identifier
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
                .ok_or_else(|| RattlerBuildError::Other("Build string not resolved".to_string()))?
                .to_string(),
        },
    );

    // If recipe_path is None, we should not include the recipe in the package
    let effective_no_include_recipe = no_include_recipe || recipe_path.is_none();

    // Create a safe fallback recipe path when None is provided
    // We use a subdirectory in output_dir to avoid copying unrelated files
    let safe_recipe_path = recipe_path
        .clone()
        .unwrap_or_else(|| output_dir.join("_no_recipe").join("recipe.yaml"));

    // Create Output object
    let recipe = rendered_variant.inner.recipe;
    let variant = rendered_variant.inner.variant;
    let hash_info = rendered_variant.inner.hash_info;

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

    // Use hash info or default
    let hash_info = hash_info.unwrap_or_else(|| HashInfo {
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
            .map_err(|e| RattlerBuildError::Other(format!("Platform detection error: {}", e)))?,
            build_platform: PlatformWithVirtualPackages::detect_for_platform(
                build_platform,
                &virtual_package_override,
            )
            .map_err(|e| RattlerBuildError::Other(format!("Platform detection error: {}", e)))?,
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
                    .unwrap_or(rattler_conda_types::compression_level::CompressionLevel::Default),
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

    // Capture start time for build duration calculation
    let start_time = std::time::Instant::now();

    // Run the build with log capture and optional tracing subscriber
    let (build_result, log_buffer) =
        tracing_subscriber::with_log_capture(progress_callback, || {
            run_async_task(async { run_build_from_args(vec![output.clone()], tool_config).await })
        });

    // Extract captured logs
    let captured_logs = log_buffer
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();

    // Check if build succeeded, if not enrich error with logs
    if let Err(err) = build_result {
        // Include logs in error message
        let log_text = if captured_logs.is_empty() {
            String::new()
        } else {
            format!("\n\nBuild log:\n{}", captured_logs.join("\n"))
        };

        return Err(RattlerBuildError::Other(format!("{}{}", err, log_text)).into());
    }

    // Calculate build time
    let build_time = start_time.elapsed().as_secs_f64();

    // Collect build result from output

    let recipe = &output.recipe;
    let build_config = &output.build_configuration;

    // Get build string
    let build_string = recipe
        .build
        .string
        .as_resolved()
        .ok_or_else(|| RattlerBuildError::Other("Build string not resolved".to_string()))?
        .to_string();

    // Construct package filename
    let archive_type = build_config.packaging_settings.archive_type;
    let extension = match archive_type {
        rattler_conda_types::package::ArchiveType::Conda => "conda",
        rattler_conda_types::package::ArchiveType::TarBz2 => "tar.bz2",
    };

    let package_filename = format!(
        "{}-{}-{}.{}",
        recipe.package.name.as_normalized(),
        recipe.package.version,
        build_string,
        extension
    );

    // Determine platform subdirectory
    let platform_str = if recipe.build.noarch.is_some() {
        "noarch"
    } else {
        build_config.target_platform.as_str()
    };

    // Construct full package path
    let package_path = build_config
        .directories
        .output_dir
        .join(platform_str)
        .join(&package_filename);

    // Convert variant to HashMap<String, String>
    let variant_map: HashMap<String, String> = build_config
        .variant
        .iter()
        .map(|(k, v)| (k.0.as_str().to_string(), v.to_string()))
        .collect();

    Ok(BuildResultPy {
        packages: vec![package_path],
        name: recipe.package.name.as_normalized().to_string(),
        version: recipe.package.version.to_string(),
        build_string,
        platform: platform_str.to_string(),
        variant: variant_map,
        build_time,
        log: captured_logs,
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
#[pyo3(name = "_rattler_build")]
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
    m.add_function(wrap_pyfunction!(build_rendered_variant_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(test_package_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_quetz_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_artifactory_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_prefix_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_anaconda_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_packages_to_conda_forge_py, &m).unwrap())?;
    m.add_class::<PyJinjaConfig>()?;
    m.add_class::<BuildResultPy>()?;

    // Register all submodules
    stage0::register_stage0_module(_py, &m)?;
    stage1::register_stage1_module(_py, &m)?;
    variant_config::register_variant_config_module(_py, &m)?;
    render::register_render_module(_py, &m)?;
    tool_config::register_tool_config_module(_py, &m)?;
    package::register_package_module(_py, &m)?;

    Ok(())
}
