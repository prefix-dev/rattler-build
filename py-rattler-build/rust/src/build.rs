use std::{collections::HashMap, path::PathBuf, str::FromStr};

use ::rattler_build::{
    metadata::{BuildConfiguration, Output, PlatformWithVirtualPackages},
    run_build_from_args,
    system_tools::SystemTools,
    types::{BuildSummary, Directories, PackageIdentifier, PackagingSettings},
};
use pyo3::prelude::*;
use rattler_build_recipe::stage1::HashInfo;
use rattler_build_script::EnvironmentIsolation;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{ChannelUrl, NamedChannelOrUrl, Platform};
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

/// Controls how the build subprocess environment is constructed.
///
/// - ``STRICT`` (default): Clean environment with only explicitly set build
///   variables and a minimal passthrough whitelist (SSL certs, SSH agent,
///   proxies). Maximum reproducibility.
/// - ``CONDA_BUILD``: Match conda-build behavior — forward CFLAGS, CXXFLAGS,
///   LDFLAGS, MAKEFLAGS, LANG, LC_ALL, and HOME from the host.
/// - ``NONE``: Inherit the entire host environment. Least reproducible but
///   useful for debugging.
#[pyclass(name = "EnvironmentIsolation", eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PyEnvironmentIsolation {
    #[pyo3(name = "STRICT")]
    Strict,
    #[pyo3(name = "CONDA_BUILD")]
    CondaBuild,
    #[pyo3(name = "NONE")]
    None,
}

impl From<PyEnvironmentIsolation> for EnvironmentIsolation {
    fn from(value: PyEnvironmentIsolation) -> Self {
        match value {
            PyEnvironmentIsolation::Strict => EnvironmentIsolation::Strict,
            PyEnvironmentIsolation::CondaBuild => EnvironmentIsolation::CondaBuild,
            PyEnvironmentIsolation::None => EnvironmentIsolation::None,
        }
    }
}

/// Result of a successful package build
#[pyclass(name = "BuildResult", from_py_object)]
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

/// Construct an `Output` object from a rendered variant and configuration.
///
/// This is shared between `build_rendered_variant_py` and `create_debug_session_py`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn output_from_rendered_variant(
    rendered_variant: &render::PyRenderedVariant,
    tool_config: &::rattler_build::tool_configuration::Configuration,
    output_dir: &Path,
    channels: &[NamedChannelOrUrl],
    no_build_id: bool,
    package_format: Option<&PackageFormatAndCompression>,
    no_include_recipe: bool,
    recipe_path: Option<&Path>,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    env_isolation: EnvironmentIsolation,
    extra_subpackages: BTreeMap<rattler_conda_types::PackageName, PackageIdentifier>,
) -> Result<Output, RattlerBuildError> {
    let timestamp = chrono::Utc::now();
    let virtual_package_override = rattler_virtual_packages::VirtualPackageOverrides::from_env();

    let mut subpackages = extra_subpackages;

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
                .ok_or_else(|| RattlerBuildError::Other("build string not resolved".to_string()))?
                .to_string(),
        },
    );

    let effective_no_include_recipe = no_include_recipe || recipe_path.is_none();

    let safe_recipe_path = recipe_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| output_dir.join("_no_recipe").join("recipe.yaml"));

    let recipe = rendered_variant.inner.recipe.clone();
    let variant = rendered_variant.inner.variant.clone();
    let hash_info = rendered_variant.inner.hash_info.clone();

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

    let channels_urls: Vec<ChannelUrl> = channels
        .iter()
        .map(|c| c.clone().into_base_url(&tool_config.channel_config))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| RattlerBuildError::Other(format!("channel error: {}", e)))?;

    let hash_info = hash_info.unwrap_or_else(|| HashInfo {
        hash: String::new(),
        prefix: String::new(),
    });

    let build_name = recipe.package.name.as_normalized().to_string();

    Ok(Output {
        recipe,
        build_configuration: BuildConfiguration {
            target_platform,
            host_platform: PlatformWithVirtualPackages::detect_for_platform(
                host_platform,
                &virtual_package_override,
            )
            .map_err(|e| RattlerBuildError::Other(format!("platform detection error: {}", e)))?,
            build_platform: PlatformWithVirtualPackages::detect_for_platform(
                build_platform,
                &virtual_package_override,
            )
            .map_err(|e| RattlerBuildError::Other(format!("platform detection error: {}", e)))?,
            hash: hash_info,
            variant,
            directories: Directories::builder(
                &build_name,
                &safe_recipe_path,
                output_dir,
                &timestamp,
            )
            .no_build_id(no_build_id)
            .build()
            .map_err(|e| RattlerBuildError::Other(format!("directory setup error: {}", e)))?,
            channels: channels_urls,
            channel_priority: tool_config.channel_priority,
            solve_strategy: SolveStrategy::Highest,
            timestamp,
            subpackages,
            packaging_settings: PackagingSettings::from_args(
                package_format
                    .map(|p| p.archive_type)
                    .unwrap_or(rattler_conda_types::package::CondaArchiveType::Conda),
                package_format
                    .map(|p| p.compression_level)
                    .unwrap_or(rattler_conda_types::compression_level::CompressionLevel::Default),
            ),
            store_recipe: !effective_no_include_recipe,
            force_colors: false,
            env_isolation,
            sandbox_config: None,
            exclude_newer,
        },
        finalized_dependencies: None,
        finalized_sources: None,
        finalized_cache_dependencies: None,
        finalized_cache_sources: None,
        staging_library_name_map: None,
        staging_build_system_libs: Vec::new(),
        build_summary: Arc::new(Mutex::new(BuildSummary::default())),
        system_tools: SystemTools::new("rattler-build", env!("CARGO_PKG_VERSION")),
        extra_meta: None,
    })
}

use std::path::Path;

/// Build from a single rendered variant (Stage1 recipe)
///
/// This function takes a RenderedVariant object (from recipe.render()) and builds it
/// directly without needing to write temporary files.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub fn build_rendered_variant_py(
    py: Python<'_>,
    rendered_variant: render::PyRenderedVariant,
    tool_config: tool_config::PyToolConfiguration,
    output_dir: PathBuf,
    channels: Vec<String>,
    progress_callback: Option<Py<PyAny>>,
    recipe_path: Option<PathBuf>,
    no_build_id: bool,
    package_format: Option<String>,
    no_include_recipe: bool,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    env_isolation: PyEnvironmentIsolation,
    sibling_variants: Vec<render::PyRenderedVariant>,
) -> PyResult<BuildResultPy> {
    let tool_config = tool_config.inner;

    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;

    let env_isolation: EnvironmentIsolation = env_isolation.into();

    let channels: Vec<NamedChannelOrUrl> = channels
        .iter()
        .map(|c| {
            NamedChannelOrUrl::from_str(c).map_err(|e| RattlerBuildError::Channel(e.to_string()))
        })
        .collect::<Result<_, _>>()?;

    // Build subpackages map from sibling variants (for multi-output pin_subpackage support)
    let mut extra_subpackages = BTreeMap::new();
    for sibling in &sibling_variants {
        let sibling_recipe = &sibling.inner.recipe;
        if let Some(build_string) = sibling_recipe.build.string.as_resolved() {
            extra_subpackages.insert(
                sibling_recipe.package.name.clone(),
                PackageIdentifier {
                    name: sibling_recipe.package.name.clone(),
                    version: sibling_recipe.package.version.clone(),
                    build_string: build_string.to_string(),
                },
            );
        }
    }

    let output = output_from_rendered_variant(
        &rendered_variant,
        &tool_config,
        &output_dir,
        &channels,
        no_build_id,
        package_format.as_ref(),
        no_include_recipe,
        recipe_path.as_deref(),
        exclude_newer,
        env_isolation,
        extra_subpackages,
    )?;

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

    // Check if build succeeded, if not raise BuildError with log as separate attribute
    if let Err(err) = build_result {
        return Err(crate::error::build_error_with_log(
            py,
            err.to_string(),
            captured_logs,
        ));
    }

    // Calculate build time
    let build_time = start_time.elapsed().as_secs_f64();

    let recipe = &output.recipe;
    let build_config = &output.build_configuration;

    let build_string = recipe
        .build
        .string
        .as_resolved()
        .ok_or_else(|| RattlerBuildError::Other("build string not resolved".to_string()))?
        .to_string();

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

    let platform_str = if recipe.build.noarch.is_some() {
        "noarch"
    } else {
        build_config.target_platform.as_str()
    };

    let package_path = build_config
        .directories
        .output_dir
        .join(platform_str)
        .join(&package_filename);

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
