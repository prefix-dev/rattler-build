use std::{collections::HashMap, path::PathBuf, str::FromStr};

use ::rattler_build::{
    metadata::{BuildConfiguration, Output, PlatformWithVirtualPackages},
    run_build_from_args,
    system_tools::SystemTools,
    types::{BuildSummary, Directories, PackageIdentifier, PackagingSettings},
};
use pyo3::prelude::*;
use rattler_build_recipe::stage1::HashInfo;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::build::PackageFormatAndCompression;
use rattler_solve::SolveStrategy;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::error::RattlerBuildError;
use crate::render;
use crate::run_async_task;
use crate::tool_config;
use crate::tracing_subscriber;

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

/// Build from a single rendered variant (Stage1 recipe)
///
/// This function takes a RenderedVariant object (from recipe.render()) and builds it
/// directly without needing to write temporary files.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn build_rendered_variant_py(
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
    let tool_config = tool_config.inner;

    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;

    let channels: Vec<NamedChannelOrUrl> = channels
        .iter()
        .map(|c| {
            NamedChannelOrUrl::from_str(c).map_err(|e| RattlerBuildError::Channel(e.to_string()))
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
            directories: Directories::builder(
                &build_name,
                &safe_recipe_path,
                &output_dir,
                &timestamp,
            )
            .no_build_id(no_build_id)
            .build()
            .map_err(|e| RattlerBuildError::Other(format!("Directory setup error: {}", e)))?,
            channels: channels_urls.clone(),
            channel_priority: tool_config.channel_priority,
            solve_strategy: SolveStrategy::Highest,
            timestamp,
            subpackages: subpackages.clone(),
            packaging_settings: {
                // If no explicit package_format was passed, check the recipe
                let effective_format = package_format.clone().or_else(|| {
                    recipe.build.package_format.as_ref().and_then(|fmt| {
                        PackageFormatAndCompression::from_str(fmt).ok()
                    })
                });
                PackagingSettings::from_args(
                    effective_format
                        .as_ref()
                        .map(|p| p.archive_type)
                        .unwrap_or(rattler_conda_types::package::CondaArchiveType::Conda),
                    effective_format
                        .as_ref()
                        .map(|p| p.compression_level)
                        .unwrap_or(rattler_conda_types::compression_level::CompressionLevel::Default),
                )
            },
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
            run_async_task(async {
                run_build_from_args(vec![output.clone()], tool_config, None).await
            })
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
        rattler_conda_types::package::CondaArchiveType::Conda => "conda",
        rattler_conda_types::package::CondaArchiveType::TarBz2 => "tar.bz2",
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
