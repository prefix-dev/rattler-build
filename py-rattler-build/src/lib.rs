use std::{path::PathBuf, str::FromStr};

use ::rattler_build::{
    build_recipes, get_rattler_build_version,
    opt::{BuildData, ChannelPriorityWrapper, CommonData, PackageFormatAndCompression, TestData},
    run_test,
    tool_configuration::{SkipExisting, TestStrategy},
};
use clap::ValueEnum;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use rattler_conda_types::Platform;

// Bind the get version function to the Python module
#[pyfunction]
fn get_rattler_build_version_py() -> PyResult<String> {
    Ok(get_rattler_build_version().to_string())
}

#[pyfunction]
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, ignore_recipe_variants, render_only, with_solve, keep_build, no_build_id, package_format, compression_threads, no_include_recipe, test, output_dir, auth_file, channel_priority, skip_existing, noarch_build_platform))]
fn build_recipes_py(
    recipes: Vec<String>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<String>>,
    ignore_recipe_variants: bool,
    render_only: bool,
    with_solve: bool,
    keep_build: bool,
    no_build_id: bool,
    package_format: Option<String>,
    compression_threads: Option<u32>,
    no_include_recipe: bool,
    test: Option<String>,
    output_dir: Option<String>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    skip_existing: Option<String>,
    noarch_build_platform: Option<String>,
) -> PyResult<()> {
    let recipes = recipes.into_iter().map(PathBuf::from).collect();
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let common = CommonData::new(
        output_dir.map(PathBuf::from),
        false,
        auth_file.map(|a| a.into()),
        channel_priority,
    );
    let build_platform = build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let variant_config =
        variant_config.map(|configs| configs.into_iter().map(PathBuf::from).collect());
    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let test = test.map(|t| TestStrategy::from_str(&t, false).unwrap());
    let skip_existing = skip_existing.map(|s| SkipExisting::from_str(&s, false).unwrap());
    let noarch_build_platform = noarch_build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

    let build_data = BuildData::new(
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        ignore_recipe_variants,
        render_only,
        with_solve,
        keep_build,
        no_build_id,
        package_format,
        compression_threads,
        no_include_recipe,
        test,
        common,
        false,
        skip_existing,
        noarch_build_platform,
        None,
        None,
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = build_recipes(recipes, build_data, &None).await {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_file, channel, compression_threads, auth_file, channel_priority))]
fn test_py(
    package_file: String,
    channel: Option<Vec<String>>,
    compression_threads: Option<u32>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
) -> PyResult<()> {
    let package_file = PathBuf::from(package_file);
    let auth_file = auth_file.map(PathBuf::from);
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let common = CommonData::new(None, false, auth_file, channel_priority);
    let test_data = TestData::new(package_file, channel, compression_threads, common);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = run_test(test_data, None).await {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
        Ok(())
    })
}

#[pymodule]
fn rattler_build<'py>(_py: Python<'py>, m: Bound<'py, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_rattler_build_version_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build_recipes_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(test_py, &m).unwrap())?;
    Ok(())
}
