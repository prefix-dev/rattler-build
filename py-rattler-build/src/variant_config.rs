// Python bindings for VariantConfig

use crate::error::RattlerBuildError;
use crate::jinja_config::PyJinjaConfig;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;
use rattler_build_variant_config::config::VariantConfig;
use std::path::PathBuf;

/// Python wrapper for VariantConfig
#[pyclass(name = "VariantConfig")]
#[derive(Clone)]
pub struct PyVariantConfig {
    pub(crate) inner: VariantConfig,
}

#[pymethods]
impl PyVariantConfig {
    /// Create a new VariantConfig with optional variants and zip_keys
    #[new]
    #[pyo3(signature = (variants=None, zip_keys=None))]
    fn new(variants: Option<Bound<'_, PyDict>>, zip_keys: Option<Vec<Vec<String>>>, py: Python<'_>) -> PyResult<Self> {
        let mut inner = VariantConfig {
            zip_keys: zip_keys.map(|zk| {
                zk.into_iter()
                    .map(|group| group.into_iter().map(NormalizedKey::from).collect())
                    .collect()
            }),
            ..Default::default()
        };

        // Process variants if provided
        if let Some(variants_dict) = variants {
            for (key, values) in variants_dict.iter() {
                let key_str: String = key.extract()?;
                let normalized_key = NormalizedKey::from(key_str.as_str());

                // Extract the list of values
                let values_list: Vec<Bound<'_, PyAny>> = values.extract()?;
                let variables: Vec<Variable> = values_list
                    .iter()
                    .map(|v| {
                        let json_value: serde_json::Value = pythonize::depythonize(v)
                            .map_err(|e| RattlerBuildError::Variant(format!("{}", e)))?;

                        match &json_value {
                            serde_json::Value::String(s) => Ok(Variable::from_string(s)),
                            serde_json::Value::Bool(b) => Ok(Variable::from(*b)),
                            serde_json::Value::Number(n) => {
                                if let Some(i) = n.as_i64() {
                                    Ok(Variable::from(i))
                                } else {
                                    Ok(Variable::from_string(&n.to_string()))
                                }
                            }
                            _ => {
                                Err(RattlerBuildError::Variant("Unsupported value type".to_string()).into())
                            }
                        }
                    })
                    .collect::<PyResult<_>>()?;

                inner.insert(normalized_key, variables);
            }
        }

        Ok(PyVariantConfig { inner })
    }

    /// Load VariantConfig from a YAML file (variants.yaml format)
    #[staticmethod]
    fn from_file(path: PathBuf) -> PyResult<Self> {
        let config = VariantConfig::from_file(&path)
            .map_err(|e| RattlerBuildError::Variant(format!("{:?}", e)))?;
        Ok(PyVariantConfig { inner: config })
    }

    /// Load VariantConfig from a YAML file with a JinjaConfig context (variants.yaml format)
    ///
    /// This allows evaluation of conditionals and templates in the variant file.
    /// The `jinja_config` provides platform information and other context needed for evaluation.
    #[staticmethod]
    fn from_file_with_context(path: PathBuf, jinja_config: &PyJinjaConfig) -> PyResult<Self> {
        let config = VariantConfig::from_file_with_context(&path, &jinja_config.inner)
            .map_err(|e| RattlerBuildError::Variant(format!("{:?}", e)))?;
        Ok(PyVariantConfig { inner: config })
    }

    /// Load VariantConfig from a conda_build_config.yaml file
    ///
    /// This supports the legacy conda-build format with `# [selector]` syntax.
    /// Selectors are evaluated using the provided JinjaConfig.
    #[staticmethod]
    fn from_conda_build_config(path: PathBuf, jinja_config: &PyJinjaConfig) -> PyResult<Self> {
        let config = rattler_build_variant_config::conda_build_config::load_conda_build_config(
            &path,
            &jinja_config.inner,
        )
        .map_err(|e| RattlerBuildError::Variant(format!("{:?}", e)))?;
        Ok(PyVariantConfig { inner: config })
    }

    /// Load VariantConfig from a YAML string (variants.yaml format)
    #[staticmethod]
    fn from_yaml(yaml: &str) -> PyResult<Self> {
        let config = VariantConfig::from_yaml_str(yaml)
            .map_err(|e| RattlerBuildError::Variant(format!("{:?}", e)))?;
        Ok(PyVariantConfig { inner: config })
    }

    /// Load VariantConfig from a YAML string with a JinjaConfig context (variants.yaml format)
    ///
    /// This allows evaluation of conditionals and templates in the variant YAML.
    /// The `jinja_config` provides platform information and other context needed for evaluation.
    #[staticmethod]
    fn from_yaml_with_context(yaml: &str, jinja_config: &PyJinjaConfig) -> PyResult<Self> {
        let config = VariantConfig::from_yaml_str_with_context(yaml, &jinja_config.inner)
            .map_err(|e| RattlerBuildError::Variant(format!("{:?}", e)))?;
        Ok(PyVariantConfig { inner: config })
    }

    /// Get all variant keys
    fn keys(&self) -> Vec<String> {
        self.inner.keys().map(|k| k.normalize()).collect()
    }

    /// Get zip_keys - groups of keys that should be zipped together
    #[getter]
    fn zip_keys(&self) -> Option<Vec<Vec<String>>> {
        self.inner.zip_keys.as_ref().map(|zip_keys| {
            zip_keys
                .iter()
                .map(|group| group.iter().map(|k| k.normalize()).collect())
                .collect()
        })
    }


    /// Get values for a specific variant key
    fn get_values(&self, key: &str, py: Python<'_>) -> PyResult<Option<Vec<Py<PyAny>>>> {
        let normalized_key = NormalizedKey::from(key);
        if let Some(values) = self.inner.get(&normalized_key) {
            let py_values = values
                .iter()
                .map(|v| {
                    let json_value = serde_json::to_value(v).map_err(RattlerBuildError::from)?;
                    pythonize::pythonize(py, &json_value)
                        .map(|obj| obj.into())
                        .map_err(|e| RattlerBuildError::Variant(format!("{}", e)).into())
                })
                .collect::<PyResult<Vec<_>>>()?;
            Ok(Some(py_values))
        } else {
            Ok(None)
        }
    }

    /// Get all variants as a dictionary
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for key in self.inner.keys() {
            if let Some(values) = self.inner.get(key) {
                let py_values: Vec<Py<PyAny>> = values
                    .iter()
                    .map(|v| {
                        let json_value =
                            serde_json::to_value(v).map_err(RattlerBuildError::from)?;
                        pythonize::pythonize(py, &json_value)
                            .map(|obj| obj.into())
                            .map_err(|e| RattlerBuildError::Variant(format!("{}", e)).into())
                    })
                    .collect::<PyResult<Vec<_>>>()?;
                dict.set_item(key.normalize(), py_values)?;
            }
        }
        Ok(dict.into())
    }

    /// Generate combinations of variants
    fn combinations(&self, py: Python<'_>) -> PyResult<Vec<Py<PyDict>>> {
        // Use all keys for combinations
        let used_vars = self.inner.keys().cloned().collect();
        let combos = self
            .inner
            .combinations(&used_vars)
            .map_err(|e| RattlerBuildError::Variant(format!("{:?}", e)))?;
        combos
            .into_iter()
            .map(|combo| {
                let dict = PyDict::new(py);
                for (key, value) in combo {
                    let json_value =
                        serde_json::to_value(&value).map_err(RattlerBuildError::from)?;
                    let py_value = pythonize::pythonize(py, &json_value)
                        .map_err(|e| RattlerBuildError::Variant(format!("{}", e)))?;
                    dict.set_item(key.normalize(), py_value)?;
                }
                Ok(dict.into())
            })
            .collect()
    }

    fn __len__(&self) -> usize {
        self.inner.keys().count()
    }

    fn __repr__(&self) -> String {
        format!("VariantConfig(keys={})", self.inner.keys().count())
    }
}

pub fn register_variant_config_module(
    _py: Python<'_>,
    parent_module: &Bound<'_, PyModule>,
) -> PyResult<()> {
    parent_module.add_class::<PyVariantConfig>()?;
    Ok(())
}
