mod output;
use output::PyOutput;

use pyo3::prelude::*;

#[pymodule]
fn rattler_build(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<PyOutput>().unwrap();
    Ok(())
}
