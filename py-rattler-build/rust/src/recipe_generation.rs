use ::rattler_build::recipe_generator::{
    CpanOpts, PyPIOpts, generate_cpan_recipe_string, generate_luarocks_recipe_string,
    generate_pypi_recipe_string, generate_r_recipe_string,
};
use pyo3::prelude::*;

use crate::run_async_task;

/// Generate a PyPI recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (package, version=None, use_mapping=true))]
pub fn generate_pypi_recipe_string_py(
    package: String,
    version: Option<String>,
    use_mapping: bool,
) -> PyResult<String> {
    let opts = PyPIOpts {
        package,
        version,
        write: false,
        use_mapping,
        tree: false,
    };

    run_async_task(generate_pypi_recipe_string(&opts))
}

/// Generate a CRAN (R) recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (package, universe=None))]
pub fn generate_r_recipe_string_py(package: String, universe: Option<String>) -> PyResult<String> {
    run_async_task(generate_r_recipe_string(&package, universe.as_deref()))
}

/// Generate a CPAN (Perl) recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (package, version=None))]
pub fn generate_cpan_recipe_string_py(
    package: String,
    version: Option<String>,
) -> PyResult<String> {
    let opts = CpanOpts {
        package,
        version,
        write: false,
        tree: false,
    };

    run_async_task(generate_cpan_recipe_string(&opts))
}

/// Generate a LuaRocks recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (rock))]
pub fn generate_luarocks_recipe_string_py(rock: String) -> PyResult<String> {
    run_async_task(generate_luarocks_recipe_string(&rock))
}
