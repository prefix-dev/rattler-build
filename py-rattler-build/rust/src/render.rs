use std::path::PathBuf;

use indexmap::IndexMap;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_build_jinja::{Jinja, UndefinedBehavior, Variable};
use rattler_build_recipe::variant_render::{
    RenderConfig as RustRenderConfig, RenderedVariant as RustRenderedVariant,
    render_recipe_with_variant_config,
};
use rattler_conda_types::Platform;

use rattler_build_script::EnvironmentIsolation;

use crate::error::RattlerBuildError;
use crate::jinja_config::PyJinjaConfig;
use crate::repodata_revision::PyRepodataRevision;
use crate::stage0::PyStage0Recipe;
use crate::stage1::PyStage1Recipe;
use crate::variant_config::PyVariantConfig;

/// Configuration for rendering recipes with variants
#[pyclass(name = "RenderConfig", from_py_object)]
#[derive(Clone)]
pub struct PyRenderConfig {
    pub(crate) inner: RustRenderConfig,
}

#[pymethods]
impl PyRenderConfig {
    /// Create a new render configuration with default settings
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (target_platform=None, build_platform=None, host_platform=None, experimental=false, repodata_revision=None, recipe_path=None, extra_context=None, build_string_prefix=None, build_number_override=None))]
    fn new(
        target_platform: Option<String>,
        build_platform: Option<String>,
        host_platform: Option<String>,
        experimental: bool,
        repodata_revision: Option<PyRepodataRevision>,
        recipe_path: Option<PathBuf>,
        extra_context: Option<Bound<'_, PyDict>>,
        build_string_prefix: Option<String>,
        build_number_override: Option<u64>,
    ) -> PyResult<Self> {
        let target_platform = target_platform
            .map(|p| p.parse::<Platform>())
            .transpose()
            .map_err(RattlerBuildError::from)?
            .unwrap_or_else(Platform::current);

        let build_platform = build_platform
            .map(|p| p.parse::<Platform>())
            .transpose()
            .map_err(RattlerBuildError::from)?
            .unwrap_or_else(Platform::current);

        let host_platform = host_platform
            .map(|p| p.parse::<Platform>())
            .transpose()
            .map_err(RattlerBuildError::from)?
            .unwrap_or(target_platform);

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

        // Get OS environment variable keys that can be overridden by variant config
        // We use an empty prefix path since we just need the keys, not the values
        let os_env_var_keys = rattler_build::env_vars::os_vars(
            &std::path::PathBuf::new(),
            &target_platform,
            &host_platform,
            EnvironmentIsolation::default(),
            &std::path::PathBuf::new(),
        )
        .keys()
        .cloned()
        .collect();

        Ok(Self {
            inner: RustRenderConfig {
                extra_context,
                experimental,
                repodata_revision: repodata_revision.unwrap_or_default().into(),
                recipe_path,
                target_platform,
                build_platform,
                host_platform,
                os_env_var_keys,
                build_string_prefix,
                build_number_override,
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

    /// Get the repodata revision controlling which recipe fields and
    /// MatchSpec syntax are accepted.
    fn repodata_revision(&self) -> PyRepodataRevision {
        self.inner.repodata_revision.into()
    }

    /// Get the recipe path
    fn recipe_path(&self) -> Option<PathBuf> {
        self.inner.recipe_path.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "RenderConfig(target_platform='{}', build_platform='{}', host_platform='{}', experimental={}, repodata_revision={:?})",
            self.inner.target_platform,
            self.inner.build_platform,
            self.inner.host_platform,
            self.inner.experimental,
            PyRepodataRevision::from(self.inner.repodata_revision),
        )
    }
}

/// Hash information for a rendered variant
#[pyclass(name = "HashInfo", from_py_object)]
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
#[pyclass(name = "PinSubpackageInfo", from_py_object)]
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
#[pyclass(name = "RenderedVariant", from_py_object)]
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

/// Render the `context` section of a recipe and substitute the resulting
/// variables into the rest of the recipe, *without* resolving variants or
/// lowering to Stage1. `recipe` may be a `Stage0Recipe` or a plain recipe
/// dictionary; a dictionary is rendered as-is, preserving its exact structure.
///
/// Semantics (a lenient, lint-oriented render):
/// * the `context` section is evaluated in order, so later entries may use
///   earlier ones;
/// * plain variables and filters (`version_to_buildstring`, `split`, ...) are
///   resolved with rattler-build's own Jinja engine;
/// * undefined variables and recipe helper functions (`compiler`,
///   `pin_subpackage`, `cdt`, ...) are left **verbatim** as `${{ ... }}`;
/// * `if` / `then` / `else` conditionals are preserved as-is.
///
/// A fully resolved scalar is re-parsed as a YAML scalar so that its type
/// (int, bool, ...) is recovered, matching how the recipe would be read back.
#[pyfunction]
#[pyo3(signature = (recipe, jinja_config=None))]
pub fn render_context(
    py: Python<'_>,
    recipe: &Bound<'_, PyAny>,
    jinja_config: Option<PyJinjaConfig>,
) -> PyResult<Py<PyAny>> {
    // Accept either a parsed Stage0 recipe or a raw recipe dictionary. The
    // dictionary path preserves the recipe's exact structure, which is what a
    // linter wants.
    let mut tree = if let Ok(stage0) = recipe.extract::<PyRef<PyStage0Recipe>>() {
        serde_json::to_value(&stage0.inner).map_err(RattlerBuildError::from)?
    } else {
        pythonize::depythonize(recipe).map_err(|e| {
            RattlerBuildError::Other(format!("Expected a Stage0 recipe or a dict: {e}"))
        })?
    };

    let mut config = jinja_config.map(|c| c.inner).unwrap_or_default();
    // Context rendering must keep undefined variables verbatim, so force a
    // strict engine: an undefined variable then errors and the original
    // `${{ ... }}` source is preserved instead of rendering to an empty string.
    config.undefined_behavior = UndefinedBehavior::Strict;

    // Build the engine and disable recipe helper functions so they survive.
    let mut jinja = Jinja::new(config);
    jinja.preserve_recipe_functions();

    // Evaluate the `context` section in order and feed it forward.
    if let Some(context) = tree.get("context").cloned() {
        if let Some(map) = context.as_object() {
            for (key, value) in map {
                let resolved = render_json_scalar(&jinja, value);
                jinja
                    .context_mut()
                    .insert(key.clone(), minijinja::Value::from_serialize(&resolved));
            }
        }
    }

    render_tree(&mut tree, &jinja);

    pythonize::pythonize(py, &tree)
        .map(|obj| obj.into())
        .map_err(|e| {
            RattlerBuildError::Other(format!("Failed to convert rendered recipe: {e}")).into()
        })
}

/// Render a single JSON scalar with the current Jinja context.
///
/// Non-strings and strings without a `${{` marker pass through unchanged.
/// Each `${{ ... }}` expression is rendered on its own so that a scalar mixing
/// resolvable and unresolvable expressions (e.g.
/// `${{ name }}-${{ unknown }}`) keeps only the unresolved part verbatim.
fn render_json_scalar(jinja: &Jinja, value: &serde_json::Value) -> serde_json::Value {
    let Some(source) = value.as_str() else {
        return value.clone();
    };
    if !source.contains("${{") {
        return value.clone();
    }
    let rendered = substitute_expressions(jinja, source);
    if rendered.contains("${{") {
        // Something was left unresolved, keep it as a string.
        serde_json::Value::String(rendered)
    } else {
        // Fully resolved: recover the scalar type (int/bool/...) the way YAML
        // would read it back.
        serde_yaml::from_str::<serde_json::Value>(&rendered)
            .unwrap_or(serde_json::Value::String(rendered))
    }
}

/// Replace every `${{ ... }}` expression in `source` by rendering it on its
/// own. An expression that fails to render (undefined variable or a preserved
/// helper function) is kept verbatim.
fn substitute_expressions(jinja: &Jinja, source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after.find("}}") {
            Some(end) => {
                let expr = &after[..end + 2];
                match jinja.render_str(expr) {
                    Ok(rendered) => out.push_str(&rendered),
                    Err(_) => out.push_str(expr),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Recursively render every templated string in the tree in place.
fn render_tree(value: &mut serde_json::Value, jinja: &Jinja) {
    match value {
        serde_json::Value::String(_) => *value = render_json_scalar(jinja, value),
        serde_json::Value::Array(items) => {
            for item in items {
                render_tree(item, jinja);
            }
        }
        serde_json::Value::Object(map) => {
            for (_key, v) in map.iter_mut() {
                render_tree(v, jinja);
            }
        }
        _ => {}
    }
}

/// Helper function to convert Python values to Variable
fn python_to_variable(value: Bound<'_, PyAny>) -> PyResult<Variable> {
    if let Ok(b) = value.extract::<bool>() {
        Ok(Variable::from(b))
    } else if let Ok(i) = value.extract::<i64>() {
        Ok(Variable::from(i))
    } else if let Ok(s) = value.extract::<String>() {
        Ok(Variable::from(s))
    } else if let Ok(list) = value.cast::<pyo3::types::PyList>() {
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
    m.add_function(wrap_pyfunction!(render_context, &m)?)?;
    parent.add_submodule(&m)?;
    Ok(())
}
