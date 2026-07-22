use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use ::rattler_build::config::Config;
use ::rattler_build::opt::{BuildData, ChannelPriorityWrapper, CommonData};
use ::rattler_build::render_recipes as render_recipes_rs;
use ::rattler_build::tool_configuration::ContinueOnFailure;
use indexmap::IndexMap;
use minijinja::ErrorKind;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_build_jinja::{Jinja, UndefinedBehavior, Variable};
use rattler_build_recipe::variant_render::{
    RenderConfig as RustRenderConfig, RenderedVariant as RustRenderedVariant,
    render_recipe_with_variant_config,
};
use rattler_conda_types::{NamedChannelOrUrl, Platform};

use rattler_build_script::EnvironmentIsolation;

use crate::error::RattlerBuildError;
use crate::jinja_config::PyJinjaConfig;
use crate::repodata_revision::PyRepodataRevision;
use crate::run_async_task;
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

/// Render recipes without building them.
///
/// Returns the same JSON string that `rattler-build build --render-only`
/// prints: a list of outputs with their rendered recipe and build
/// configuration, skip-filtered and sorted topologically.
#[pyfunction]
#[pyo3(signature = (recipes, up_to=None, build_platform=None, target_platform=None, host_platform=None, channel=None, variant_config=None, variant_overrides=None, ignore_recipe_variants=false, with_solve=false, no_build_id=false, output_dir=None, auth_file=None, channel_priority=None, allow_insecure_host=None, exclude_newer=None, build_num=None, build_string_prefix=None, use_bz2=true, use_zstd=true, use_sharded=true, repodata_revision=None))]
#[allow(clippy::too_many_arguments)]
pub fn render_recipes(
    recipes: Vec<PathBuf>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<PathBuf>>,
    variant_overrides: Option<HashMap<String, Vec<String>>>,
    ignore_recipe_variants: bool,
    with_solve: bool,
    no_build_id: bool,
    output_dir: Option<PathBuf>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    exclude_newer: Option<jiff::Timestamp>,
    build_num: Option<u64>,
    build_string_prefix: Option<String>,
    use_bz2: bool,
    use_zstd: bool,
    use_sharded: bool,
    repodata_revision: Option<PyRepodataRevision>,
) -> PyResult<String> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    let config = Config::default();
    let v3 = matches!(
        repodata_revision.unwrap_or_default(),
        PyRepodataRevision::V3
    );
    let common = CommonData::new(
        output_dir,
        false,
        v3,
        auth_file.map(|a| a.into()),
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_sharded,
    );
    let build_platform = build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                        .map_err(|e| e.into())
                })
                .collect::<PyResult<_>>()?,
        ),
    };

    let build_data = BuildData::new(
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        variant_overrides.unwrap_or_default(),
        ignore_recipe_variants,
        true, // render_only
        with_solve,
        false, // keep_build
        no_build_id,
        None,  // package_format
        None,  // compression_threads
        None,  // io_concurrency_limit
        false, // no_include_recipe
        None,  // test
        common,
        None, // skip_existing
        None, // noarch_build_platform
        None, // extra meta
        None, // sandbox configuration
        EnvironmentIsolation::default(),
        ContinueOnFailure::from(false),
        false, // error_prefix_in_binary
        false, // allow_symlinks_on_windows
        false, // allow_absolute_license_paths
        exclude_newer,
        build_num,
        build_string_prefix,
        None, // markdown_summary
    );

    run_async_task(async {
        let outputs = render_recipes_rs(recipes, &build_data, &None).await?;
        serde_json::to_string(&outputs)
            .map_err(|e| miette::miette!("failed to serialize rendered outputs: {e}"))
    })
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
/// `functions` may map helper names to Python callables; a mapped helper is
/// evaluated with the callable's return value instead of being preserved. A
/// callable that raises falls back to preserving the expression verbatim.
///
/// Every substituted scalar stays a string. Recovering the YAML type of a
/// rendered scalar needs the original document's quoting to tell
/// `"${{ python_min }}"` from `${{ build_number }}`, and that information is
/// gone by the time the recipe reaches this function, so the caller does it.
#[pyfunction]
#[pyo3(signature = (recipe, jinja_config=None, functions=None))]
pub fn render_context(
    py: Python<'_>,
    recipe: &Bound<'_, PyAny>,
    jinja_config: Option<PyJinjaConfig>,
    functions: Option<Bound<'_, PyDict>>,
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

    // Re-enable helpers the caller provided an implementation for. The global
    // replaces the undefined shadow installed by `preserve_recipe_functions`.
    if let Some(functions) = functions {
        for (name, func) in functions.iter() {
            let name = name.extract::<String>()?;
            let func = func.unbind();
            jinja.env_mut().add_global(
                name,
                minijinja::Value::from_function(python_jinja_function(func)),
            );
        }
    }

    // Evaluate the `context` section in order and feed it forward. Rendered
    // entries are fed forward as *strings* - the same value a Jinja engine
    // produces - so that comparisons in later entries (e.g.
    // `${{ x if major == '0' else y }}`) keep working. Entries that do not
    // fully resolve are not fed forward at all, so a reference to them stays
    // verbatim instead of having the unresolved template expanded into it.
    if let Some(context) = tree.get("context").cloned()
        && let Some(map) = context.as_object()
    {
        for (key, value) in map {
            let context_value = match value.as_str() {
                Some(source) if is_templated(source) => {
                    let rendered = substitute_expressions(&jinja, source);
                    if is_templated(&rendered) {
                        continue;
                    }
                    minijinja::Value::from(rendered)
                }
                _ => minijinja::Value::from_serialize(value),
            };
            jinja.context_mut().insert(key.clone(), context_value);
        }
    }

    render_tree(&mut tree, &jinja);

    pythonize::pythonize(py, &tree)
        .map(|obj| obj.into())
        .map_err(|e| {
            RattlerBuildError::Other(format!("Failed to convert rendered recipe: {e}")).into()
        })
}

/// Wrap a Python callable as a minijinja function.
///
/// Positional arguments and keyword arguments are converted to Python values,
/// the callable's result is converted back into a Jinja value. Any Python
/// exception surfaces as a minijinja error, which `render_context` treats as
/// "keep the expression verbatim".
fn python_jinja_function(
    func: Py<PyAny>,
) -> impl Fn(minijinja::value::Rest<minijinja::Value>) -> Result<minijinja::Value, minijinja::Error>
+ Send
+ Sync
+ 'static {
    fn invalid_op(message: String) -> minijinja::Error {
        minijinja::Error::new(ErrorKind::InvalidOperation, message)
    }

    move |args: minijinja::value::Rest<minijinja::Value>| {
        Python::attach(|py| {
            // Keyword arguments arrive as a trailing kwargs-marked value.
            let mut positional = args.0.as_slice();
            let kwargs = positional
                .last()
                .filter(|last| last.is_kwargs())
                .and_then(|last| minijinja::value::Kwargs::try_from(last.clone()).ok());
            if kwargs.is_some() {
                positional = &positional[..positional.len() - 1];
            }

            let py_args = positional
                .iter()
                .map(|value| pythonize::pythonize(py, value))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| invalid_op(format!("failed to convert argument: {e}")))?;
            let py_args = pyo3::types::PyTuple::new(py, py_args)
                .map_err(|e| invalid_op(format!("failed to build arguments: {e}")))?;

            let py_kwargs = kwargs
                .map(|kwargs| {
                    let dict = PyDict::new(py);
                    for key in kwargs.args() {
                        let value: minijinja::Value = kwargs.get(key)?;
                        let value = pythonize::pythonize(py, &value).map_err(|e| {
                            invalid_op(format!("failed to convert keyword argument: {e}"))
                        })?;
                        dict.set_item(key, value)
                            .map_err(|e| invalid_op(format!("failed to set keyword: {e}")))?;
                    }
                    Ok::<_, minijinja::Error>(dict)
                })
                .transpose()?;

            let result = func
                .call(py, py_args, py_kwargs.as_ref())
                .map_err(|e| invalid_op(format!("python function raised: {e}")))?;

            let json: serde_json::Value = pythonize::depythonize(result.bind(py))
                .map_err(|e| invalid_op(format!("failed to convert return value: {e}")))?;
            Ok(minijinja::Value::from_serialize(&json))
        })
    }
}

/// Render a single JSON scalar with the current Jinja context.
///
/// Non-strings and strings without a `${{` or `{%` marker pass through
/// unchanged. Each `${{ ... }}` expression is rendered on its own so that a
/// scalar mixing resolvable and unresolvable expressions (e.g.
/// `${{ name }}-${{ unknown }}`) keeps only the unresolved part verbatim.
fn render_json_scalar(jinja: &Jinja, value: &serde_json::Value) -> serde_json::Value {
    let Some(source) = value.as_str() else {
        return value.clone();
    };
    if !is_templated(source) {
        return value.clone();
    }
    serde_json::Value::String(substitute_expressions(jinja, source))
}

/// Whether a scalar carries Jinja syntax: a `${{ ... }}` expression or a
/// `{% ... %}` statement block.
fn is_templated(source: &str) -> bool {
    source.contains("${{") || source.contains("{%")
}

/// Replace every `${{ ... }}` expression in `source` by rendering it on its
/// own. An expression that fails to render (undefined variable or a preserved
/// helper function) is kept verbatim.
///
/// A scalar containing a statement block (`{% if %}`, `{% for %}`, `{% raw %}`)
/// cannot be split this way, because the block spans the surrounding text and
/// its body may depend on the loop or branch it sits in. Such a scalar is
/// rendered as a whole instead, and kept verbatim if that fails.
fn substitute_expressions(jinja: &Jinja, source: &str) -> String {
    if source.contains("{%") {
        return jinja
            .render_str(source)
            .unwrap_or_else(|_| source.to_string());
    }
    substitute_inline_expressions(jinja, source)
}

/// Replace every `${{ ... }}` expression in a source without statement blocks.
fn substitute_inline_expressions(jinja: &Jinja, source: &str) -> String {
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
    m.add_function(wrap_pyfunction!(render_recipes, &m)?)?;
    m.add_function(wrap_pyfunction!(render_context, &m)?)?;
    parent.add_submodule(&m)?;
    Ok(())
}
