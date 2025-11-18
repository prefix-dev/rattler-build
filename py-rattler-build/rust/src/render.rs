use std::path::PathBuf;

use indexmap::IndexMap;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_build_jinja::Variable;
use rattler_build_recipe::variant_render::{
    RenderConfig as RustRenderConfig, RenderedVariant as RustRenderedVariant,
    render_recipe_with_variant_config,
};
use rattler_conda_types::Platform;

use crate::error::RattlerBuildError;
use crate::stage0::PyStage0Recipe;
use crate::stage1::PyStage1Recipe;
use crate::variant_config::PyVariantConfig;

/// Configuration for rendering recipes with variants
#[pyclass(name = "RenderConfig")]
#[derive(Clone)]
pub struct PyRenderConfig {
    pub(crate) inner: RustRenderConfig,
}

#[pymethods]
impl PyRenderConfig {
    /// Create a new render configuration with default settings
    #[new]
    #[pyo3(signature = (target_platform=None, build_platform=None, host_platform=None, experimental=false, recipe_path=None, extra_context=None))]
    fn new(
        target_platform: Option<String>,
        build_platform: Option<String>,
        host_platform: Option<String>,
        experimental: bool,
        recipe_path: Option<PathBuf>,
        extra_context: Option<Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let target_platform = target_platform
            .map(|p| {
                p.parse::<Platform>().map_err(|e| {
                    RattlerBuildError::Other(format!("Invalid target_platform: {}", e))
                })
            })
            .transpose()?
            .unwrap_or_else(Platform::current);

        let build_platform = build_platform
            .map(|p| {
                p.parse::<Platform>()
                    .map_err(|e| RattlerBuildError::Other(format!("Invalid build_platform: {}", e)))
            })
            .transpose()?
            .unwrap_or_else(Platform::current);

        let host_platform = host_platform
            .map(|p| {
                p.parse::<Platform>()
                    .map_err(|e| RattlerBuildError::Other(format!("Invalid host_platform: {}", e)))
            })
            .transpose()?
            .unwrap_or_else(Platform::current);

        let extra_context = extra_context
            .map(|dict| {
                dict.iter()
                    .map(|(key, value)| {
                        let key_str = key.extract::<String>()?;
                        let variable = python_to_variable(value)?;
                        Ok((key_str, variable))
                    })
                    .collect::<PyResult<IndexMap<String, Variable>>>()
            })
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            inner: RustRenderConfig {
                extra_context,
                experimental,
                recipe_path,
                target_platform,
                build_platform,
                host_platform,
            },
        })
    }

    /// Get an extra context variable
    fn get_context(&self, py: Python<'_>, key: &str) -> PyResult<Option<Py<PyAny>>> {
        if let Some(var) = self.inner.extra_context.get(key) {
            Ok(Some(variable_to_python(py, var)?))
        } else {
            Ok(None)
        }
    }

    /// Get all extra context variables as a dictionary
    fn get_all_context(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.inner.extra_context {
            dict.set_item(key, variable_to_python(py, value)?)?;
        }
        Ok(dict.into())
    }

    /// Get the target platform as a string
    fn target_platform(&self) -> String {
        self.inner.target_platform.to_string()
    }

    /// Get the build platform as a string
    fn build_platform(&self) -> String {
        self.inner.build_platform.to_string()
    }

    /// Get the host platform as a string
    fn host_platform(&self) -> String {
        self.inner.host_platform.to_string()
    }

    /// Get whether experimental features are enabled
    fn experimental(&self) -> bool {
        self.inner.experimental
    }

    /// Get the recipe path
    fn recipe_path(&self) -> Option<PathBuf> {
        self.inner.recipe_path.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "RenderConfig(target_platform='{}', build_platform='{}', host_platform='{}', experimental={})",
            self.inner.target_platform,
            self.inner.build_platform,
            self.inner.host_platform,
            self.inner.experimental
        )
    }
}

/// Hash information for a rendered variant
#[pyclass(name = "HashInfo")]
#[derive(Clone, Debug)]
pub struct PyHashInfo {
    #[pyo3(get)]
    pub hash: String,
    #[pyo3(get)]
    pub prefix: String,
}

#[pymethods]
impl PyHashInfo {
    fn __repr__(&self) -> String {
        format!("HashInfo(hash='{}', prefix='{}')", self.hash, self.prefix)
    }
}

/// Information about a pin_subpackage dependency
#[pyclass(name = "PinSubpackageInfo")]
#[derive(Clone, Debug)]
pub struct PyPinSubpackageInfo {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub version: String,
    #[pyo3(get)]
    pub build_string: Option<String>,
    #[pyo3(get)]
    pub exact: bool,
}

#[pymethods]
impl PyPinSubpackageInfo {
    fn __repr__(&self) -> String {
        format!(
            "PinSubpackageInfo(name='{}', version='{}', build_string={:?}, exact={})",
            self.name, self.version, self.build_string, self.exact
        )
    }
}

/// Result of rendering a recipe with a specific variant combination
#[pyclass(name = "RenderedVariant")]
#[derive(Clone)]
pub struct PyRenderedVariant {
    pub(crate) inner: RustRenderedVariant,
}

#[pymethods]
impl PyRenderedVariant {
    /// Get the variant combination used (variable name -> value)
    fn variant(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.inner.variant {
            dict.set_item(key.0.as_str(), variable_to_python(py, value)?)?;
        }
        Ok(dict.into())
    }

    /// Get the rendered stage1 recipe
    fn recipe(&self) -> PyStage1Recipe {
        PyStage1Recipe {
            inner: self.inner.recipe.clone(),
        }
    }

    /// Get hash info if available
    fn hash_info(&self) -> Option<PyHashInfo> {
        self.inner.hash_info.as_ref().map(|hash_info| PyHashInfo {
            hash: hash_info.hash.clone(),
            prefix: hash_info.prefix.clone(),
        })
    }

    /// Get pin_subpackage information
    fn pin_subpackages(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, pin_info) in &self.inner.pin_subpackages {
            let py_pin_info = PyPinSubpackageInfo {
                name: pin_info.name.as_normalized().to_string(),
                version: pin_info.version.to_string(),
                build_string: pin_info.build_string.clone(),
                exact: pin_info.exact,
            };
            dict.set_item(key.0.as_str(), py_pin_info)?;
        }
        Ok(dict.into())
    }

    fn __repr__(&self) -> String {
        format!(
            "RenderedVariant(package='{}', version='{}', build_string='{}')",
            self.inner.recipe.package.name.as_normalized(),
            self.inner.recipe.package.version,
            self.inner
                .recipe
                .build
                .string
                .as_resolved()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "None".to_string())
        )
    }
}

/// Render a Stage0 recipe with a variant configuration into Stage1 recipes
///
/// # Arguments
/// * `recipe` - The Stage0 recipe to render (Recipe, SingleOutputRecipe, or MultiOutputRecipe)
/// * `variant_config` - The variant configuration
/// * `render_config` - Optional render configuration (defaults to current platform)
///
/// # Returns
/// A list of RenderedVariant objects, one for each variant combination
#[pyfunction]
#[pyo3(signature = (recipe, variant_config, render_config=None))]
pub fn render_recipe(
    recipe: &Bound<'_, PyAny>,
    variant_config: &PyVariantConfig,
    render_config: Option<PyRenderConfig>,
) -> PyResult<Vec<PyRenderedVariant>> {
    let config = render_config.unwrap_or_else(|| PyRenderConfig {
        inner: RustRenderConfig::default(),
    });

    // Try to extract the inner stage0 recipe
    let stage0_recipe = if let Ok(r) = recipe.extract::<PyRef<PyStage0Recipe>>() {
        r.inner.clone()
    } else {
        return Err(RattlerBuildError::Other("Expected a Stage0 Recipe".to_string()).into());
    };

    // Call the Rust render function
    let rendered =
        render_recipe_with_variant_config(&stage0_recipe, &variant_config.inner, config.inner)
            .map_err(|e| RattlerBuildError::Other(format!("Render error: {:?}", e)))?;

    // Convert to Python objects
    Ok(rendered
        .into_iter()
        .map(|r| PyRenderedVariant { inner: r })
        .collect())
}

/// Helper function to convert Python values to Variable
fn python_to_variable(value: Bound<'_, PyAny>) -> PyResult<Variable> {
    if let Ok(b) = value.extract::<bool>() {
        Ok(Variable::from(b))
    } else if let Ok(i) = value.extract::<i64>() {
        Ok(Variable::from(i))
    } else if let Ok(s) = value.extract::<String>() {
        Ok(Variable::from(s))
    } else if let Ok(list) = value.downcast::<pyo3::types::PyList>() {
        let items: PyResult<Vec<Variable>> =
            list.iter().map(|item| python_to_variable(item)).collect();
        Ok(Variable::from(items?))
    } else {
        Ok(Variable::from(value.to_string()))
    }
}

/// Helper function to convert Variable to Python values
fn variable_to_python(py: Python<'_>, var: &Variable) -> PyResult<Py<PyAny>> {
    // Try to extract as bool first (must be before number check)
    if let Some(b) = var.as_bool() {
        let json_val = serde_json::Value::Bool(b);
        return pythonize::pythonize(py, &json_val)
            .map(|obj| obj.into())
            .map_err(|e| {
                RattlerBuildError::Other(format!("Failed to convert bool: {}", e)).into()
            });
    }

    // Try to extract as integer
    if let Some(i) = var.as_i64() {
        let json_val = serde_json::Value::Number(i.into());
        return pythonize::pythonize(py, &json_val)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::Other(format!("Failed to convert int: {}", e)).into());
    }

    // Try to extract as string
    if let Some(s) = var.as_str() {
        let json_val = serde_json::Value::String(s.to_string());
        return pythonize::pythonize(py, &json_val)
            .map(|obj| obj.into())
            .map_err(|e| {
                RattlerBuildError::Other(format!("Failed to convert string: {}", e)).into()
            });
    }

    // Try to extract as list/sequence
    if var.is_sequence() {
        let mut vec = Vec::new();
        if let Ok(iter) = var.try_iter() {
            for item in iter {
                let item_var = Variable::from(item);
                let py_item = variable_to_python(py, &item_var)?;
                vec.push(py_item);
            }
            let list = pyo3::types::PyList::new(py, &vec)?;
            return Ok(list.unbind().into());
        }
    }

    // Fallback to string representation
    let s = var.to_string();
    let json_val = serde_json::Value::String(s);
    pythonize::pythonize(py, &json_val)
        .map(|obj| obj.into())
        .map_err(|e| RattlerBuildError::Other(format!("Failed to convert value: {}", e)).into())
}

/// Register the render module with Python
pub fn register_render_module(py: Python<'_>, parent: &Bound<'_, PyModule>) -> PyResult<()> {
    let m = PyModule::new(py, "render")?;
    m.add_class::<PyRenderConfig>()?;
    m.add_class::<PyRenderedVariant>()?;
    m.add_class::<PyHashInfo>()?;
    m.add_class::<PyPinSubpackageInfo>()?;
    m.add_function(wrap_pyfunction!(render_recipe, &m)?)?;
    parent.add_submodule(&m)?;
    Ok(())
}
