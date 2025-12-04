use std::future::Future;

use ::rattler_build::get_rattler_build_version;
use pyo3::prelude::*;

mod build;
mod cli_api;
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

use build::BuildResultPy;
use error::RattlerBuildError;
use jinja_config::PyJinjaConfig;

/// Execute async tasks in Python bindings with proper error handling
pub(crate) fn run_async_task<F, R>(future: F) -> PyResult<R>
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
    m.add_function(wrap_pyfunction!(cli_api::build_recipes_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build::build_rendered_variant_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(cli_api::test_package_py, &m).unwrap())?;
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
