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
mod render;
mod stage0;
mod stage1;
mod upload;
mod variant_config;
mod recipe_generation;

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

// Legacy parse_recipe_py function - now use the stage0/stage1 Python bindings instead
// This function is commented out as the Recipe::from_yaml API has changed
// Use the new Stage0Recipe.from_yaml() in the stage0 module
/*
/// Parse a recipe YAML string and return the parsed recipe as a Python dictionary.
#[pyfunction]
#[pyo3(signature = (yaml_content, selector_config))]
fn parse_recipe_py(
    yaml_content: String,
    selector_config: &PySelectorConfig,
) -> PyResult<Py<PyAny>> {
    // This function needs to be updated to work with the new stage0/stage1 API
    // For now, use the stage0.Recipe.from_yaml() method instead
    unimplemented!("Use stage0.Recipe.from_yaml() instead")
}
*/

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
    m.add_function(wrap_pyfunction!(recipe_generation::generate_pypi_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(recipe_generation::generate_r_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(recipe_generation::generate_cpan_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(recipe_generation::generate_luarocks_recipe_string_py, &m).unwrap())?;
    // parse_recipe_py is deprecated - use stage0.Recipe.from_yaml() instead
    // m.add_function(wrap_pyfunction!(parse_recipe_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build_recipes_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(test_package_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_quetz_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_artifactory_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_prefix_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_package_to_anaconda_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload::upload_packages_to_conda_forge_py, &m).unwrap())?;
    m.add_class::<PyJinjaConfig>()?;

    // Register stage0, stage1, variant_config, and render submodules
    stage0::register_stage0_module(_py, &m)?;
    stage1::register_stage1_module(_py, &m)?;
    variant_config::register_variant_config_module(_py, &m)?;
    render::register_render_module(_py, &m)?;

    Ok(())
}
