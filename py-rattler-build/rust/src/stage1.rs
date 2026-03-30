// Python bindings for rattler-build stage1 types (evaluated recipe)

use crate::error::RattlerBuildError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_build_recipe::stage1;

/// Stage1 Recipe - The fully evaluated recipe ready for building
#[pyclass(name = "Stage1Recipe", from_py_object)]
#[derive(Clone)]
pub struct PyStage1Recipe {
    pub(crate) inner: stage1::Recipe,
}

#[pymethods]
impl PyStage1Recipe {
    #[getter]
    fn package(&self) -> PyStage1Package {
        PyStage1Package {
            inner: self.inner.package.clone(),
        }
    }

    #[getter]
    fn build(&self) -> PyStage1Build {
        PyStage1Build {
            inner: self.inner.build.clone(),
        }
    }

    #[getter]
    fn requirements(&self) -> PyStage1Requirements {
        PyStage1Requirements {
            inner: self.inner.requirements.clone(),
        }
    }

    #[getter]
    fn about(&self) -> PyStage1About {
        PyStage1About {
            inner: self.inner.about.clone(),
        }
    }

    #[getter]
    fn context(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.inner.context {
            let json_value = serde_json::to_value(value).map_err(RattlerBuildError::from)?;
            let py_value = pythonize::pythonize(py, &json_value)
                .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
            dict.set_item(key, py_value)?;
        }
        Ok(dict.into())
    }

    #[getter]
    fn used_variant(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.inner.used_variant {
            let json_value = serde_json::to_value(value).map_err(RattlerBuildError::from)?;
            let py_value = pythonize::pythonize(py, &json_value)
                .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
            dict.set_item(key.normalize(), py_value)?;
        }
        Ok(dict.into())
    }

    #[getter]
    fn sources(&self) -> Vec<PyStage1Source> {
        self.inner
            .source
            .iter()
            .map(|s| PyStage1Source { inner: s.clone() })
            .collect()
    }

    #[getter]
    fn staging_caches(&self) -> Vec<PyStage1StagingCache> {
        self.inner
            .staging_caches
            .iter()
            .map(|s| PyStage1StagingCache { inner: s.clone() })
            .collect()
    }

    #[getter]
    fn inherits_from(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        if let Some(ref inherits) = self.inner.inherits_from {
            let json_value = serde_json::to_value(inherits).map_err(RattlerBuildError::from)?;
            Ok(Some(
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?,
            ))
        } else {
            Ok(None)
        }
    }

    /// Convert to Python dictionary
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!(
            "Stage1Recipe(package='{}', version='{}')",
            self.inner.package.name.as_normalized(),
            self.inner.package.version
        )
    }
}

/// Stage1 Package metadata
#[pyclass(name = "Stage1Package", from_py_object)]
#[derive(Clone)]
pub struct PyStage1Package {
    pub(crate) inner: stage1::Package,
}

#[pymethods]
impl PyStage1Package {
    #[getter]
    fn name(&self) -> String {
        self.inner.name.as_normalized().to_string()
    }

    #[getter]
    fn version(&self) -> String {
        self.inner.version.to_string()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!(
            "Stage1Package(name='{}', version='{}')",
            self.inner.name.as_normalized(),
            self.inner.version
        )
    }
}

/// Stage1 Build configuration
#[pyclass(name = "Stage1Build", from_py_object)]
#[derive(Clone)]
pub struct PyStage1Build {
    pub(crate) inner: stage1::Build,
}

#[pymethods]
impl PyStage1Build {
    #[getter]
    fn number(&self) -> Option<u64> {
        self.inner.number
    }

    #[getter]
    fn string(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.string).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    #[getter]
    fn script(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.script).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    #[getter]
    fn noarch(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        if let Some(ref noarch) = self.inner.noarch {
            let json_value = serde_json::to_value(noarch).map_err(RattlerBuildError::from)?;
            Ok(Some(
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?,
            ))
        } else {
            Ok(None)
        }
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("Stage1Build(number={:?})", self.inner.number)
    }
}

/// Stage1 Requirements
#[pyclass(name = "Stage1Requirements", from_py_object)]
#[derive(Clone)]
pub struct PyStage1Requirements {
    pub(crate) inner: stage1::Requirements,
}

#[pymethods]
impl PyStage1Requirements {
    #[getter]
    fn build(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.inner
            .build
            .iter()
            .map(|dep| {
                let json_value = serde_json::to_value(dep).map_err(RattlerBuildError::from)?;
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
            })
            .collect()
    }

    #[getter]
    fn host(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.inner
            .host
            .iter()
            .map(|dep| {
                let json_value = serde_json::to_value(dep).map_err(RattlerBuildError::from)?;
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
            })
            .collect()
    }

    #[getter]
    fn run(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.inner
            .run
            .iter()
            .map(|dep| {
                let json_value = serde_json::to_value(dep).map_err(RattlerBuildError::from)?;
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
            })
            .collect()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!(
            "Stage1Requirements(build={}, host={}, run={})",
            self.inner.build.len(),
            self.inner.host.len(),
            self.inner.run.len()
        )
    }
}

/// Stage1 About metadata
#[pyclass(name = "Stage1About", from_py_object)]
#[derive(Clone)]
pub struct PyStage1About {
    pub(crate) inner: stage1::About,
}

#[pymethods]
impl PyStage1About {
    #[getter]
    fn homepage(&self) -> Option<String> {
        self.inner.homepage.as_ref().map(|u| u.to_string())
    }

    #[getter]
    fn repository(&self) -> Option<String> {
        self.inner.repository.as_ref().map(|u| u.to_string())
    }

    #[getter]
    fn documentation(&self) -> Option<String> {
        self.inner.documentation.as_ref().map(|u| u.to_string())
    }

    #[getter]
    fn license(&self) -> Option<String> {
        self.inner
            .license
            .as_ref()
            .map(|l| l.0.as_ref().to_string())
    }

    #[getter]
    fn summary(&self) -> Option<String> {
        self.inner.summary.clone()
    }

    #[getter]
    fn description(&self) -> Option<String> {
        self.inner.description.clone()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!(
            "Stage1About(license={:?}, summary={:?})",
            self.license(),
            self.summary()
        )
    }
}

/// Stage1 Source
#[pyclass(name = "Stage1Source", from_py_object)]
#[derive(Clone)]
pub struct PyStage1Source {
    pub(crate) inner: stage1::Source,
}

#[pymethods]
impl PyStage1Source {
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage1 Staging Cache
#[pyclass(name = "Stage1StagingCache", from_py_object)]
#[derive(Clone)]
pub struct PyStage1StagingCache {
    pub(crate) inner: stage1::StagingCache,
}

#[pymethods]
impl PyStage1StagingCache {
    #[getter]
    fn name(&self) -> String {
        self.inner.name.clone()
    }

    #[getter]
    fn build(&self) -> PyStage1Build {
        PyStage1Build {
            inner: self.inner.build.clone(),
        }
    }

    #[getter]
    fn requirements(&self) -> PyStage1Requirements {
        PyStage1Requirements {
            inner: self.inner.requirements.clone(),
        }
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("Stage1StagingCache(name='{}')", self.inner.name)
    }
}

pub fn register_stage1_module(py: Python<'_>, parent_module: &Bound<'_, PyModule>) -> PyResult<()> {
    let stage1_module = PyModule::new(py, "stage1")?;

    stage1_module.add_class::<PyStage1Recipe>()?;
    stage1_module.add_class::<PyStage1Package>()?;
    stage1_module.add_class::<PyStage1Build>()?;
    stage1_module.add_class::<PyStage1Requirements>()?;
    stage1_module.add_class::<PyStage1About>()?;
    stage1_module.add_class::<PyStage1Source>()?;
    stage1_module.add_class::<PyStage1StagingCache>()?;

    parent_module.add_submodule(&stage1_module)?;
    Ok(())
}
