use std::str::FromStr;

use rattler_build::metadata::Output;
use pyo3::{pyclass, pymethods};

#[pyclass]
#[derive(Clone)]
pub struct PyOutput {
    pub inner: Output,
}

#[pymethods]
impl PyOutput {
    pub fn from_yaml(yaml: &str) -> Self {
        Self {
            inner: Output::from_str(yaml).unwrap(),
        }
    }
}