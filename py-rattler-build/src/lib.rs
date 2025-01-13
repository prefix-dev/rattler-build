use std::path::PathBuf;

use ::rattler_build::{build_recipes, get_rattler_build_version, opt::BuildData};
use pyo3::prelude::*;

// Bind the get version function to the Python module
#[pyfunction]
fn get_rattler_build_version_py() -> PyResult<String> {
    Ok(get_rattler_build_version().to_string())
}

#[pyfunction]
fn build_recipes_py(recipes: Vec<String>) -> PyResult<()> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let recipes = recipes.into_iter().map(PathBuf::from).collect();
    rt.block_on(async {
        if let Err(e) = build_recipes(recipes, BuildData::default(), &None).await {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                e.to_string(),
            ));
        }
        Ok(())
    })
}

#[pymodule]
fn rattler_build<'py>(_py: Python<'py>, m: Bound<'py, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_rattler_build_version_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build_recipes_py, &m).unwrap())?;
    Ok(())
}
