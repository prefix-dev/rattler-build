use serde_json::Value as JsonValue;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    str::FromStr,
};

use crate::error::RattlerBuildError;
use pyo3::prelude::*;

use rattler_build_jinja::{JinjaConfig, NormalizedKey, UndefinedBehavior, Variable};
use rattler_conda_types::Platform;

/// Python wrapper for JinjaConfig
#[pyclass(name = "PyJinjaConfig")]
#[derive(Clone)]
pub struct PyJinjaConfig {
    pub(crate) inner: JinjaConfig,
}

#[pymethods]
impl PyJinjaConfig {
    #[new]
    #[pyo3(signature = (target_platform=None, host_platform=None, build_platform=None, variant=None, experimental=None, allow_undefined=None, recipe_path=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        target_platform: Option<String>,
        host_platform: Option<String>,
        build_platform: Option<String>,
        variant: Option<HashMap<String, Py<PyAny>>>,
        experimental: Option<bool>,
        allow_undefined: Option<bool>,
        recipe_path: Option<PathBuf>,
    ) -> PyResult<Self> {
        let target_platform = target_platform
            .map(|p| Platform::from_str(&p))
            .transpose()
            .map_err(RattlerBuildError::from)?
            .unwrap_or_else(Platform::current);

        let host_platform = host_platform
            .map(|p| Platform::from_str(&p))
            .transpose()
            .map_err(RattlerBuildError::from)?
            .unwrap_or(target_platform);

        let build_platform = build_platform
            .map(|p| Platform::from_str(&p))
            .transpose()
            .map_err(RattlerBuildError::from)?
            .unwrap_or_else(Platform::current);

        // Convert variant from Python dict to BTreeMap<NormalizedKey, Variable>
        let variant_map = if let Some(variant_dict) = variant {
            let mut map = BTreeMap::new();
            for (key, value) in variant_dict {
                let normalized_key = NormalizedKey::from(key);
                // Convert Python object to JSON Value then to Variable
                let json_val: serde_json::Value =
                    pythonize::depythonize(value.bind(py)).map_err(|e| {
                        RattlerBuildError::Variant(format!(
                            "Failed to convert variant value: {}",
                            e
                        ))
                    })?;
                let variable = match &json_val {
                    JsonValue::String(s) => Variable::from_string(s),
                    JsonValue::Bool(b) => Variable::from(*b),
                    JsonValue::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Variable::from(i)
                        } else {
                            Variable::from_string(&n.to_string())
                        }
                    }

                    JsonValue::Array(arr) => {
                        let vars: Result<Vec<Variable>, RattlerBuildError> = arr
                            .iter()
                            .map(|v| match v {
                                JsonValue::String(s) => Ok(Variable::from_string(s)),
                                JsonValue::Bool(b) => Ok(Variable::from(*b)),
                                JsonValue::Number(n) => Ok(if let Some(i) = n.as_i64() {
                                    Variable::from(i)
                                } else {
                                    Variable::from_string(&n.to_string())
                                }),
                                _ => Err(RattlerBuildError::Variant(
                                    "Complex array elements not supported".to_string(),
                                )),
                            })
                            .collect();
                        Variable::from(vars?)
                    }
                    _ => {
                        return Err(RattlerBuildError::Variant(
                            "Object and null variants not supported".to_string(),
                        )
                        .into());
                    }
                };
                map.insert(normalized_key, variable);
            }
            Ok::<BTreeMap<NormalizedKey, Variable>, RattlerBuildError>(map)?
        } else {
            BTreeMap::new()
        };

        // Convert allow_undefined to undefined_behavior
        let undefined_behavior = if allow_undefined.unwrap_or(false) {
            UndefinedBehavior::Lenient
        } else {
            UndefinedBehavior::SemiStrict
        };

        let jinja_config = JinjaConfig {
            target_platform,
            host_platform,
            build_platform,
            variant: variant_map,
            experimental: experimental.unwrap_or(false),
            recipe_path,
            undefined_behavior,
        };

        Ok(PyJinjaConfig {
            inner: jinja_config,
        })
    }

    #[getter]
    fn target_platform(&self) -> String {
        self.inner.target_platform.to_string()
    }

    #[getter]
    fn host_platform(&self) -> String {
        self.inner.host_platform.to_string()
    }

    #[getter]
    fn build_platform(&self) -> String {
        self.inner.build_platform.to_string()
    }

    #[getter]
    fn experimental(&self) -> bool {
        self.inner.experimental
    }

    #[getter]
    fn allow_undefined(&self) -> bool {
        matches!(self.inner.undefined_behavior, UndefinedBehavior::Lenient)
    }

    #[getter]
    fn recipe_path(&self) -> Option<PathBuf> {
        self.inner.recipe_path.clone()
    }

    #[getter]
    fn variant(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut dict = HashMap::new();
        for (key, value) in &self.inner.variant {
            let json_value = serde_json::to_value(value).map_err(RattlerBuildError::from)?;
            dict.insert(key.normalize(), json_value);
        }
        Ok(pythonize::pythonize(py, &dict)
            .map(|obj| obj.into())
            .map_err(|e| {
                RattlerBuildError::Variant(format!("Failed to convert variant to Python: {}", e))
            })?)
    }
}
