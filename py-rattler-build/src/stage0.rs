// Python bindings for rattler-build stage0 types (parsed recipe)

use crate::error::RattlerBuildError;
use pyo3::types::PyDict;
use pyo3::{IntoPyObjectExt, prelude::*};
use rattler_build_recipe::stage0;

/// Stage0 Recipe - The parsed recipe before evaluation
///
/// This is a union type that wraps either SingleOutputRecipe or MultiOutputRecipe.
/// Most users should use `from_yaml()` which returns the specific type directly.
#[pyclass(name = "Stage0Recipe")]
#[derive(Clone)]
pub struct PyStage0Recipe {
    pub(crate) inner: stage0::Recipe,
}

#[pymethods]
impl PyStage0Recipe {
    /// Parse a recipe from YAML string
    #[staticmethod]
    fn from_yaml(yaml: &str) -> PyResult<Self> {
        let recipe = stage0::parse_recipe_or_multi_from_source(yaml)
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{:?}", e)))?;
        Ok(PyStage0Recipe { inner: recipe })
    }

    /// Create a recipe from a Python dictionary
    #[staticmethod]
    fn from_dict(dict: &Bound<'_, PyAny>) -> PyResult<Self> {
        // Convert Python dict to JSON value via pythonize
        let json_value: serde_json::Value = pythonize::depythonize(dict).map_err(|e| {
            RattlerBuildError::RecipeParse(format!("Failed to convert Python dict to JSON: {}", e))
        })?;

        // Convert to YAML string using the JSON value's serde repr
        // This is a simple approach: serialize to JSON and parse as YAML
        let json_string = serde_json::to_string(&json_value).map_err(|e| {
            RattlerBuildError::RecipeParse(format!("Failed to serialize to JSON: {}", e))
        })?;

        // Parse as YAML (YAML is a superset of JSON, so this works)
        let recipe = stage0::parse_recipe_or_multi_from_source(&json_string)
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{:?}", e)))?;

        Ok(PyStage0Recipe { inner: recipe })
    }

    /// Check if this is a single output recipe
    fn is_single_output(&self) -> bool {
        matches!(self.inner, stage0::Recipe::SingleOutput(_))
    }

    /// Check if this is a multi output recipe
    fn is_multi_output(&self) -> bool {
        matches!(self.inner, stage0::Recipe::MultiOutput(_))
    }

    /// Get the recipe as a single output (returns None if multi-output)
    fn as_single_output(&self) -> Option<PySingleOutputRecipe> {
        match &self.inner {
            stage0::Recipe::SingleOutput(single) => Some(PySingleOutputRecipe {
                inner: single.as_ref().clone(),
            }),
            _ => None,
        }
    }

    /// Get the recipe as a multi output (returns None if single-output)
    fn as_multi_output(&self) -> Option<PyMultiOutputRecipe> {
        match &self.inner {
            stage0::Recipe::MultiOutput(multi) => Some(PyMultiOutputRecipe {
                inner: multi.as_ref().clone(),
            }),
            _ => None,
        }
    }

    /// Get the package information
    /// For single-output recipes, returns the package directly.
    /// For multi-output recipes, returns None (use .recipe.package on the multi-output recipe instead).
    #[getter]
    fn package(&self) -> Option<PyStage0Package> {
        match &self.inner {
            stage0::Recipe::SingleOutput(single) => Some(PyStage0Package {
                inner: single.package.clone(),
            }),
            _ => None,
        }
    }

    /// Get the build configuration
    #[getter]
    fn build(&self) -> PyStage0Build {
        match &self.inner {
            stage0::Recipe::SingleOutput(single) => PyStage0Build {
                inner: single.build.clone(),
            },
            stage0::Recipe::MultiOutput(multi) => PyStage0Build {
                inner: multi.build.clone(),
            },
        }
    }

    /// Get the requirements
    /// For single-output recipes, returns the requirements directly.
    /// For multi-output recipes, returns None (requirements are per-output).
    #[getter]
    fn requirements(&self) -> Option<PyStage0Requirements> {
        match &self.inner {
            stage0::Recipe::SingleOutput(single) => Some(PyStage0Requirements {
                inner: single.requirements.clone(),
            }),
            _ => None,
        }
    }

    /// Get the about metadata
    #[getter]
    fn about(&self) -> PyStage0About {
        match &self.inner {
            stage0::Recipe::SingleOutput(single) => PyStage0About {
                inner: single.about.clone(),
            },
            stage0::Recipe::MultiOutput(multi) => PyStage0About {
                inner: multi.about.clone(),
            },
        }
    }

    /// Get the context dictionary
    #[getter]
    fn context(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let context_map = match &self.inner {
            stage0::Recipe::SingleOutput(single) => &single.context,
            stage0::Recipe::MultiOutput(multi) => &multi.context,
        };

        let dict = PyDict::new(py);
        for (key, value) in context_map {
            let json_value = serde_json::to_value(value).map_err(RattlerBuildError::from)?;
            let py_value = pythonize::pythonize(py, &json_value)
                .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
            dict.set_item(key, py_value)?;
        }
        Ok(dict.into())
    }

    /// Convert to Python dictionary representation
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| {
                RattlerBuildError::RecipeParse(format!("Failed to convert to Python: {}", e)).into()
            })
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            stage0::Recipe::SingleOutput(_) => "Stage0Recipe(type='single-output')".to_string(),
            stage0::Recipe::MultiOutput(_) => "Stage0Recipe(type='multi-output')".to_string(),
        }
    }
}

/// Stage0 Single Output Recipe
#[pyclass(name = "SingleOutputRecipe")]
#[derive(Clone)]
pub struct PySingleOutputRecipe {
    pub(crate) inner: stage0::SingleOutputRecipe,
}

#[pymethods]
impl PySingleOutputRecipe {
    #[getter]
    fn schema_version(&self) -> Option<u32> {
        self.inner.schema_version
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
    fn package(&self) -> PyStage0Package {
        PyStage0Package {
            inner: self.inner.package.clone(),
        }
    }

    #[getter]
    fn build(&self) -> PyStage0Build {
        PyStage0Build {
            inner: self.inner.build.clone(),
        }
    }

    #[getter]
    fn requirements(&self) -> PyStage0Requirements {
        PyStage0Requirements {
            inner: self.inner.requirements.clone(),
        }
    }

    #[getter]
    fn about(&self) -> PyStage0About {
        PyStage0About {
            inner: self.inner.about.clone(),
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
            "SingleOutputRecipe(name='{}', version='{}')",
            serde_json::to_value(&self.inner.package.name)
                .and_then(serde_json::from_value::<String>)
                .unwrap_or_default(),
            serde_json::to_value(&self.inner.package.version)
                .and_then(serde_json::from_value::<String>)
                .unwrap_or_default()
        )
    }
}

/// Stage0 Multi Output Recipe
#[pyclass(name = "MultiOutputRecipe")]
#[derive(Clone)]
pub struct PyMultiOutputRecipe {
    pub(crate) inner: stage0::MultiOutputRecipe,
}

#[pymethods]
impl PyMultiOutputRecipe {
    #[getter]
    fn schema_version(&self) -> Option<u32> {
        self.inner.schema_version
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
    fn recipe(&self) -> PyStage0RecipeMetadata {
        PyStage0RecipeMetadata {
            inner: self.inner.recipe.clone(),
        }
    }

    #[getter]
    fn build(&self) -> PyStage0Build {
        PyStage0Build {
            inner: self.inner.build.clone(),
        }
    }

    #[getter]
    fn about(&self) -> PyStage0About {
        PyStage0About {
            inner: self.inner.about.clone(),
        }
    }

    #[getter]
    fn outputs(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        let mut result = Vec::new();
        for output in &self.inner.outputs {
            match output {
                stage0::Output::Package(pkg) => {
                    let py_output = PyStage0PackageOutput {
                        inner: pkg.as_ref().clone(),
                    };
                    result.push(py_output.into_py_any(py)?);
                }
                stage0::Output::Staging(staging) => {
                    let py_staging = PyStage0StagingOutput {
                        inner: *staging.clone(),
                    };
                    result.push(py_staging.into_py_any(py)?);
                }
            }
        }
        Ok(result)
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("MultiOutputRecipe(outputs={})", self.inner.outputs.len())
    }
}

/// Stage0 Package (full package with name and version)
#[pyclass(name = "Stage0Package")]
#[derive(Clone)]
pub struct PyStage0Package {
    pub(crate) inner: stage0::Package,
}

#[pymethods]
impl PyStage0Package {
    /// Get the package name (may be a template string like "${{ name }}")
    #[getter]
    fn name(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner.name).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get the package version (may be a template string like "${{ version }}")
    #[getter]
    fn version(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.version).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 Package metadata (package with optional version for multi-output recipes)
#[pyclass(name = "Stage0PackageMetadata")]
#[derive(Clone)]
pub struct PyStage0PackageMetadata {
    pub(crate) inner: stage0::PackageMetadata,
}

#[pymethods]
impl PyStage0PackageMetadata {
    /// Get the package name (may be a template string like "${{ name }}")
    #[getter]
    fn name(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner.name).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get the package version (may be a template string like "${{ version }}", or None if inherited)
    #[getter]
    fn version(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.version {
            Some(version) => {
                let json_value = serde_json::to_value(version).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 Recipe metadata (for multi-output recipes)
#[pyclass(name = "Stage0RecipeMetadata")]
#[derive(Clone)]
pub struct PyStage0RecipeMetadata {
    pub(crate) inner: stage0::RecipeMetadata,
}

#[pymethods]
impl PyStage0RecipeMetadata {
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 Build configuration
#[pyclass(name = "Stage0Build")]
#[derive(Clone)]
pub struct PyStage0Build {
    pub(crate) inner: stage0::Build,
}

#[pymethods]
impl PyStage0Build {
    /// Get the build number (may be a template)
    #[getter]
    fn number(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.number).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get the build string (may be a template or None for auto-generated)
    #[getter]
    fn string(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.string {
            Some(string) => {
                let json_value = serde_json::to_value(string).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the build script configuration
    #[getter]
    fn script(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.script).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get the noarch type (may be a template or None)
    #[getter]
    fn noarch(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.noarch {
            Some(noarch) => {
                let json_value = serde_json::to_value(noarch).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 Requirements
#[pyclass(name = "Stage0Requirements")]
#[derive(Clone)]
pub struct PyStage0Requirements {
    pub(crate) inner: stage0::Requirements,
}

#[pymethods]
impl PyStage0Requirements {
    /// Get build-time requirements (list of matchspecs or templates)
    #[getter]
    fn build(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.build).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get host-time requirements (list of matchspecs or templates)
    #[getter]
    fn host(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner.host).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get run-time requirements (list of matchspecs or templates)
    #[getter]
    fn run(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner.run).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Get run-time constraints (list of matchspecs or templates)
    #[getter]
    fn run_constraints(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.run_constraints).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 About metadata
#[pyclass(name = "Stage0About")]
#[derive(Clone)]
pub struct PyStage0About {
    pub(crate) inner: stage0::About,
}

#[pymethods]
impl PyStage0About {
    /// Get the homepage URL (may be a template or None)
    #[getter]
    fn homepage(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.homepage {
            Some(homepage) => {
                let json_value = serde_json::to_value(homepage).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the license (may be a template or None)
    #[getter]
    fn license(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.license {
            Some(license) => {
                let json_value = serde_json::to_value(license).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the license family (deprecated, may be a template or None)
    #[getter]
    fn license_family(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.license_family {
            Some(license_family) => {
                let json_value =
                    serde_json::to_value(license_family).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the summary (may be a template or None)
    #[getter]
    fn summary(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.summary {
            Some(summary) => {
                let json_value = serde_json::to_value(summary).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the description (may be a template or None)
    #[getter]
    fn description(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.description {
            Some(description) => {
                let json_value =
                    serde_json::to_value(description).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the documentation URL (may be a template or None)
    #[getter]
    fn documentation(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.documentation {
            Some(documentation) => {
                let json_value =
                    serde_json::to_value(documentation).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    /// Get the repository URL (may be a template or None)
    #[getter]
    fn repository(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.inner.repository {
            Some(repository) => {
                let json_value =
                    serde_json::to_value(repository).map_err(RattlerBuildError::from)?;
                let py_value = pythonize::pythonize(py, &json_value)
                    .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)))?;
                Ok(Some(py_value.into()))
            }
            None => Ok(None),
        }
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 Package Output
#[pyclass(name = "Stage0PackageOutput")]
#[derive(Clone)]
pub struct PyStage0PackageOutput {
    pub(crate) inner: stage0::PackageOutput,
}

#[pymethods]
impl PyStage0PackageOutput {
    #[getter]
    fn package(&self) -> PyStage0PackageMetadata {
        PyStage0PackageMetadata {
            inner: self.inner.package.clone(),
        }
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

/// Stage0 Staging Output
#[pyclass(name = "Stage0StagingOutput")]
#[derive(Clone)]
pub struct PyStage0StagingOutput {
    pub(crate) inner: stage0::StagingOutput,
}

#[pymethods]
impl PyStage0StagingOutput {
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }
}

pub fn register_stage0_module(py: Python<'_>, parent_module: &Bound<'_, PyModule>) -> PyResult<()> {
    let stage0_module = PyModule::new(py, "stage0")?;

    stage0_module.add_class::<PyStage0Recipe>()?;
    stage0_module.add_class::<PySingleOutputRecipe>()?;
    stage0_module.add_class::<PyMultiOutputRecipe>()?;
    stage0_module.add_class::<PyStage0Package>()?;
    stage0_module.add_class::<PyStage0PackageMetadata>()?;
    stage0_module.add_class::<PyStage0RecipeMetadata>()?;
    stage0_module.add_class::<PyStage0Build>()?;
    stage0_module.add_class::<PyStage0Requirements>()?;
    stage0_module.add_class::<PyStage0About>()?;
    stage0_module.add_class::<PyStage0PackageOutput>()?;
    stage0_module.add_class::<PyStage0StagingOutput>()?;

    parent_module.add_submodule(&stage0_module)?;
    Ok(())
}
