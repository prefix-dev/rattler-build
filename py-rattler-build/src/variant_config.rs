use std::collections::BTreeMap;
use std::path::PathBuf;
use pyo3::prelude::*;
use ::rattler_build::{
    NormalizedKey,
    variant_config::{Pin as RustPin, VariantConfig as RustVariantConfig},
    recipe::variable::Variable,
};
use crate::error::RattlerBuildError;
use crate::PySelectorConfig;

/// Python wrapper for Pin struct
#[pyclass(name = "PyPin")]
#[derive(Clone, Debug)]
pub struct PyPin {
    pub(crate) inner: RustPin,
}

#[pymethods]
impl PyPin {
    #[new]
    #[pyo3(signature = (max_pin=None, min_pin=None))]
    fn new(max_pin: Option<String>, min_pin: Option<String>) -> Self {
        PyPin {
            inner: RustPin {
                max_pin,
                min_pin,
            },
        }
    }

    #[getter]
    fn max_pin(&self) -> Option<String> {
        self.inner.max_pin.clone()
    }

    #[setter]
    fn set_max_pin(&mut self, value: Option<String>) {
        self.inner.max_pin = value;
    }

    #[getter]
    fn min_pin(&self) -> Option<String> {
        self.inner.min_pin.clone()
    }

    #[setter]
    fn set_min_pin(&mut self, value: Option<String>) {
        self.inner.min_pin = value;
    }

    fn __repr__(&self) -> String {
        format!("Pin(max_pin={:?}, min_pin={:?})", self.inner.max_pin, self.inner.min_pin)
    }
}

/// Python wrapper for VariantConfig struct
#[pyclass(name = "PyVariantConfig")]
#[derive(Clone, Debug)]
pub struct PyVariantConfig {
    pub(crate) inner: RustVariantConfig,
}

#[pymethods]
impl PyVariantConfig {
    #[new]
    #[pyo3(signature = (pin_run_as_build=None, zip_keys=None, variants=None))]
    fn new(
        py: Python<'_>,
        pin_run_as_build: Option<BTreeMap<String, PyRef<PyPin>>>,
        zip_keys: Option<Vec<Vec<String>>>,
        variants: Option<BTreeMap<String, Vec<Py<PyAny>>>>,
    ) -> PyResult<Self> {
        // Convert pin_run_as_build from Python wrapper to Rust
        let pin_run_as_build = pin_run_as_build.map(|map| {
            map.into_iter()
                .map(|(k, v)| (k, v.inner.clone()))
                .collect()
        });

        // Convert zip_keys from String to NormalizedKey
        let zip_keys = zip_keys.map(|keys| {
            keys.into_iter()
                .map(|inner_vec| {
                    inner_vec.into_iter()
                        .map(NormalizedKey::from)
                        .collect()
                })
                .collect()
        });

        // Convert variants from Python to Rust
        let variants = if let Some(variant_dict) = variants {
            let mut map = BTreeMap::new();
            for (key, value_list) in variant_dict {
                let normalized_key = NormalizedKey::from(key);
                let mut variables = Vec::new();

                for py_value in value_list {
                    let json_val: serde_json::Value =
                        pythonize::depythonize(py_value.bind(py)).map_err(|e| {
                            RattlerBuildError::Variant(format!(
                                "Failed to convert variant value: {}",
                                e
                            ))
                        })?;

                    let variable = match &json_val {
                        serde_json::Value::String(s) => Variable::from_string(s),
                        serde_json::Value::Bool(b) => Variable::from(*b),
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                Variable::from(i)
                            } else {
                                Variable::from_string(&n.to_string())
                            }
                        }
                        _ => {
                            return Err(RattlerBuildError::Variant(
                                "Variant values must be string, bool, or number".to_string(),
                            )
                            .into());
                        }
                    };
                    variables.push(variable);
                }
                map.insert(normalized_key, variables);
            }
            map
        } else {
            BTreeMap::new()
        };

        Ok(PyVariantConfig {
            inner: RustVariantConfig {
                pin_run_as_build,
                zip_keys,
                variants,
            },
        })
    }

    #[getter]
    fn pin_run_as_build(&self) -> Option<BTreeMap<String, PyPin>> {
        self.inner.pin_run_as_build.as_ref().map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), PyPin { inner: v.clone() }))
                .collect()
        })
    }

    #[setter]
    fn set_pin_run_as_build(&mut self, value: Option<BTreeMap<String, PyRef<PyPin>>>) {
        self.inner.pin_run_as_build = value.map(|map| {
            map.into_iter()
                .map(|(k, v)| (k, v.inner.clone()))
                .collect()
        });
    }

    #[getter]
    fn zip_keys(&self) -> Option<Vec<Vec<String>>> {
        self.inner.zip_keys.as_ref().map(|keys| {
            keys.iter()
                .map(|inner_vec| {
                    inner_vec.iter()
                        .map(|nk| nk.normalize())
                        .collect()
                })
                .collect()
        })
    }

    #[setter]
    fn set_zip_keys(&mut self, value: Option<Vec<Vec<String>>>) {
        self.inner.zip_keys = value.map(|keys| {
            keys.into_iter()
                .map(|inner_vec| {
                    inner_vec.into_iter()
                        .map(NormalizedKey::from)
                        .collect()
                })
                .collect()
        });
    }

    #[getter]
    fn variants(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_variants: BTreeMap<String, Vec<serde_json::Value>> = self.inner.variants
            .iter()
            .map(|(k, v)| {
                let values: Result<Vec<serde_json::Value>, RattlerBuildError> = v
                    .iter()
                    .map(|var| serde_json::to_value(var).map_err(RattlerBuildError::from))
                    .collect();
                values.map(|vals| (k.normalize(), vals))
            })
            .collect::<Result<BTreeMap<_, _>, RattlerBuildError>>()?;

        Ok(pythonize::pythonize(py, &json_variants)
            .map(|obj| obj.into())
            .map_err(|e| {
                RattlerBuildError::Variant(format!("Failed to convert variants to Python: {}", e))
            })?)
    }

    #[setter]
    fn set_variants(&mut self, py: Python<'_>, value: BTreeMap<String, Vec<Py<PyAny>>>) -> PyResult<()> {
        let mut map = BTreeMap::new();
        for (key, value_list) in value {
            let normalized_key = NormalizedKey::from(key);
            let mut variables = Vec::new();

            for py_value in value_list {
                let json_val: serde_json::Value =
                    pythonize::depythonize(py_value.bind(py)).map_err(|e| {
                        RattlerBuildError::Variant(format!(
                            "Failed to convert variant value: {}",
                            e
                        ))
                    })?;

                let variable = match &json_val {
                    serde_json::Value::String(s) => Variable::from_string(s),
                    serde_json::Value::Bool(b) => Variable::from(*b),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Variable::from(i)
                        } else {
                            Variable::from_string(&n.to_string())
                        }
                    }
                    _ => {
                        return Err(RattlerBuildError::Variant(
                            "Variant values must be string, bool, or number".to_string(),
                        )
                        .into());
                    }
                };
                variables.push(variable);
            }
            map.insert(normalized_key, variables);
        }
        self.inner.variants = map;
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!(
            "VariantConfig(pin_run_as_build={:?}, zip_keys={:?}, variants_keys={:?})",
            self.inner.pin_run_as_build.as_ref().map(|m| m.keys().collect::<Vec<_>>()),
            self.inner.zip_keys,
            self.inner.variants.keys().collect::<Vec<_>>()
        )
    }

    /// Load a VariantConfig from a single file.
    ///
    /// This function loads a single variant configuration file. The file can be
    /// either a variant config file (variants.yaml) or a conda-build config file
    /// (conda_build_config.yaml). The target_platform and build_platform are
    /// automatically inserted into the variants.
    ///
    /// Args:
    ///     path: Path to variant configuration file
    ///     selector_config: SelectorConfig to use for rendering
    ///
    /// Returns:
    ///     A new PyVariantConfig with the configuration from the file
    #[staticmethod]
    fn from_file(path: PathBuf, selector_config: &PySelectorConfig) -> PyResult<Self> {
        let config = RustVariantConfig::from_file(&path, &selector_config.inner)
            .map_err(|e| RattlerBuildError::Variant(format!("Failed to load variant config: {:?}", e)))?;

        Ok(PyVariantConfig { inner: config })
    }

    /// Load a VariantConfig from a list of files.
    ///
    /// This function loads multiple variant configuration files and merges them
    /// into a single configuration. Files can be either variant config files
    /// (variants.yaml) or conda-build config files (conda_build_config.yaml).
    ///
    /// Args:
    ///     files: List of paths to variant configuration files
    ///     selector_config: SelectorConfig to use for rendering
    ///
    /// Returns:
    ///     A new PyVariantConfig with the merged configuration
    #[staticmethod]
    fn from_files(files: Vec<PathBuf>, selector_config: &PySelectorConfig) -> PyResult<Self> {
        let config = RustVariantConfig::from_files(&files, &selector_config.inner)
            .map_err(|e| RattlerBuildError::Variant(format!("Failed to load variant config: {:?}", e)))?;

        Ok(PyVariantConfig { inner: config })
    }

    /// Merge another VariantConfig into this one.
    ///
    /// - Variants are extended (keys from `other` replace keys in `self`)
    /// - pin_run_as_build entries are extended
    /// - zip_keys are replaced (not merged)
    ///
    /// Args:
    ///     other: Another PyVariantConfig to merge into this one
    fn merge(&mut self, other: &PyVariantConfig) {
        self.inner.merge(other.inner.clone());
    }
}
