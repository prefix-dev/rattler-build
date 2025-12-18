use serde_json::Value as JsonValue;
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    path::PathBuf,
    str::FromStr,
};

use ::rattler_build::{
    NormalizedKey, build_recipes, get_rattler_build_version,
    hash::HashInfo,
    metadata::Debug,
    opt::{BuildData, ChannelPriorityWrapper, CommonData, TestData},
    recipe::{parser::Recipe, variable::Variable},
    recipe_generator::{
        CpanOpts, PyPIOpts, generate_cpan_recipe_string, generate_luarocks_recipe_string,
        generate_pypi_recipe_string, generate_r_recipe_string,
    },
    run_test,
    selectors::SelectorConfig,
    tool_configuration::{self, ContinueOnFailure, SkipExisting, TestStrategy},
};
use clap::ValueEnum;
use pyo3::prelude::*;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::{ConfigBase, build::PackageFormatAndCompression};
use rattler_upload::upload;
use rattler_upload::upload::opt::{
    AnacondaData, ArtifactoryData, AttestationSource, CondaForgeData, ForceOverwrite, PrefixData,
    QuetzData, SkipExisting as UploadSkipExisting,
};
use url::Url;

mod error;
use error::RattlerBuildError;

/// Execute async tasks in Python bindings with proper error handling
fn run_async_task<F, R>(future: F) -> PyResult<R>
where
    F: Future<Output = miette::Result<R>>,
{
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| RattlerBuildError::Other(format!("Failed to create async runtime: {}", e)))?;

    Ok(rt.block_on(async { future.await.map_err(RattlerBuildError::from) })?)
}

/// Python wrapper for SelectorConfig
#[pyclass]
#[derive(Clone)]
pub struct PySelectorConfig {
    pub(crate) inner: SelectorConfig,
}

#[pymethods]
impl PySelectorConfig {
    #[new]
    #[pyo3(signature = (target_platform=None, host_platform=None, build_platform=None, variant=None, experimental=None, allow_undefined=None, recipe_path=None, hash=None))]
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
        hash: Option<String>,
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
            .unwrap_or_else(Platform::current);

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

        let selector_config = SelectorConfig {
            target_platform,
            host_platform,
            build_platform,
            hash: hash.map(|h| HashInfo {
                hash: h,
                prefix: String::new(),
            }),
            variant: variant_map,
            experimental: experimental.unwrap_or(false),
            allow_undefined: allow_undefined.unwrap_or(false),
            recipe_path,
        };

        Ok(PySelectorConfig {
            inner: selector_config,
        })
    }

    #[getter]
    fn target_platform(&self) -> String {
        self.inner.target_platform.to_string()
    }

    #[setter]
    fn set_target_platform(&mut self, value: String) -> PyResult<()> {
        let platform = Platform::from_str(&value).map_err(RattlerBuildError::from)?;
        self.inner.target_platform = platform;
        Ok(())
    }

    #[getter]
    fn host_platform(&self) -> String {
        self.inner.host_platform.to_string()
    }

    #[setter]
    fn set_host_platform(&mut self, value: String) -> PyResult<()> {
        let platform = Platform::from_str(&value).map_err(RattlerBuildError::from)?;
        self.inner.host_platform = platform;
        Ok(())
    }

    #[getter]
    fn build_platform(&self) -> String {
        self.inner.build_platform.to_string()
    }

    #[setter]
    fn set_build_platform(&mut self, value: String) -> PyResult<()> {
        let platform = Platform::from_str(&value).map_err(RattlerBuildError::from)?;
        self.inner.build_platform = platform;
        Ok(())
    }

    #[getter]
    fn experimental(&self) -> bool {
        self.inner.experimental
    }

    #[setter]
    fn set_experimental(&mut self, value: bool) {
        self.inner.experimental = value;
    }

    #[getter]
    fn allow_undefined(&self) -> bool {
        self.inner.allow_undefined
    }

    #[setter]
    fn set_allow_undefined(&mut self, value: bool) {
        self.inner.allow_undefined = value;
    }

    #[getter]
    fn recipe_path(&self) -> Option<PathBuf> {
        self.inner.recipe_path.clone()
    }

    #[setter]
    fn set_recipe_path(&mut self, value: Option<PathBuf>) {
        self.inner.recipe_path = value;
    }

    #[getter]
    fn hash(&self) -> Option<String> {
        self.inner.hash.as_ref().map(|h| h.hash.clone())
    }

    #[setter]
    fn set_hash(&mut self, value: Option<String>) {
        self.inner.hash = value.map(|h| HashInfo {
            hash: h,
            prefix: String::new(),
        });
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

    #[setter]
    fn set_variant(&mut self, py: Python<'_>, value: HashMap<String, Py<PyAny>>) -> PyResult<()> {
        let mut map = BTreeMap::new();
        for (key, py_value) in value {
            let normalized_key = NormalizedKey::from(key);
            let json_val: serde_json::Value =
                pythonize::depythonize(py_value.bind(py)).map_err(|e| {
                    RattlerBuildError::Variant(format!("Failed to convert variant value: {}", e))
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
        self.inner.variant = map;
        Ok(())
    }
}

// Bind the get version function to the Python module
#[pyfunction]
fn get_rattler_build_version_py() -> PyResult<String> {
    Ok(get_rattler_build_version().to_string())
}

/// Generate a PyPI recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (package, version=None, use_mapping=true))]
fn generate_pypi_recipe_string_py(
    package: String,
    version: Option<String>,
    use_mapping: bool,
) -> PyResult<String> {
    let opts = PyPIOpts {
        package,
        version,
        write: false,
        use_mapping,
        tree: false,
    };

    run_async_task(generate_pypi_recipe_string(&opts))
}

/// Generate a CRAN (R) recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (package, universe=None))]
fn generate_r_recipe_string_py(package: String, universe: Option<String>) -> PyResult<String> {
    run_async_task(generate_r_recipe_string(&package, universe.as_deref()))
}

/// Generate a CPAN (Perl) recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (package, version=None))]
fn generate_cpan_recipe_string_py(package: String, version: Option<String>) -> PyResult<String> {
    let opts = CpanOpts {
        package,
        version,
        write: false,
        tree: false,
    };

    run_async_task(generate_cpan_recipe_string(&opts))
}

/// Generate a LuaRocks recipe and return the YAML as a string.
#[pyfunction]
#[pyo3(signature = (rock))]
fn generate_luarocks_recipe_string_py(rock: String) -> PyResult<String> {
    run_async_task(generate_luarocks_recipe_string(&rock))
}

/// Parse a recipe YAML string and return the parsed recipe as a Python dictionary.
#[pyfunction]
#[pyo3(signature = (yaml_content, selector_config))]
fn parse_recipe_py(
    yaml_content: String,
    selector_config: &PySelectorConfig,
) -> PyResult<Py<PyAny>> {
    match Recipe::from_yaml(yaml_content.as_str(), selector_config.inner.clone()) {
        Ok(recipe) => {
            let json_value = serde_json::to_value(recipe).map_err(RattlerBuildError::from)?;

            Python::attach(|py| {
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| {
                        RattlerBuildError::RecipeParse(format!(
                            "Failed to convert to Python: {}",
                            e
                        ))
                        .into()
                    })
            })
        }
        Err(errors) => Err(RattlerBuildError::RecipeParse(format!(
            "Recipe parsing failed: {:?}",
            errors
        ))
        .into()),
    }
}

#[pyfunction]
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, variant_overrides=None, ignore_recipe_variants=false, render_only=false, with_solve=false, keep_build=false, no_build_id=false, package_format=None, compression_threads=None, io_concurrency_limit=None, no_include_recipe=false, test=None, output_dir=None, auth_file=None, channel_priority=None, skip_existing=None, noarch_build_platform=None, allow_insecure_host=None, continue_on_failure=false, debug=false, error_prefix_in_binary=false, allow_symlinks_on_windows=false, exclude_newer=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
#[allow(clippy::too_many_arguments)]
fn build_recipes_py(
    recipes: Vec<PathBuf>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<PathBuf>>,
    variant_overrides: Option<HashMap<String, Vec<String>>>,
    ignore_recipe_variants: bool,
    render_only: bool,
    with_solve: bool,
    keep_build: bool,
    no_build_id: bool,
    package_format: Option<String>,
    compression_threads: Option<u32>,
    io_concurrency_limit: Option<usize>,
    no_include_recipe: bool,
    test: Option<String>,
    output_dir: Option<PathBuf>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    skip_existing: Option<String>,
    noarch_build_platform: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    continue_on_failure: bool,
    debug: bool,
    error_prefix_in_binary: bool,
    allow_symlinks_on_windows: bool,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    // todo: allow custom config here
    let config = ConfigBase::<()>::default();
    let common = CommonData::new(
        output_dir,
        false,
        auth_file.map(|a| a.into()),
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_jlap,
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
    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;
    let test = test.map(|t| TestStrategy::from_str(&t, false).unwrap());
    let skip_existing = skip_existing.map(|s| SkipExisting::from_str(&s, false).unwrap());
    let noarch_build_platform = noarch_build_platform
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
        render_only,
        with_solve,
        keep_build,
        no_build_id,
        package_format,
        compression_threads,
        io_concurrency_limit,
        no_include_recipe,
        test,
        common,
        false, // TUI disabled
        skip_existing,
        noarch_build_platform,
        None, // extra meta
        None, // sandbox configuration
        Debug::new(debug),
        ContinueOnFailure::from(continue_on_failure),
        error_prefix_in_binary,
        allow_symlinks_on_windows,
        exclude_newer,
        // TODO: implement build number override!
        None,
    );

    run_async_task(async {
        build_recipes(recipes, build_data, &None).await?;
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (package_file, channel, compression_threads, auth_file, channel_priority, allow_insecure_host=None, debug=false, test_index=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
fn test_package_py(
    package_file: PathBuf,
    channel: Option<Vec<String>>,
    compression_threads: Option<u32>,
    auth_file: Option<PathBuf>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    debug: bool,
    test_index: Option<usize>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    // todo: allow custom config here
    let config = ConfigBase::<()>::default();
    let common = CommonData::new(
        None,
        false,
        auth_file,
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_jlap,
        use_sharded,
    );
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
    let test_data = TestData::new(
        package_file,
        channel,
        compression_threads,
        Debug::new(debug),
        test_index,
        common,
    );

    run_async_task(async {
        run_test(test_data, None).await?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_files, url, channels, api_key, auth_file))]
fn upload_package_to_quetz_py(
    package_files: Vec<PathBuf>,
    url: String,
    channels: String,
    api_key: Option<String>,
    auth_file: Option<PathBuf>,
) -> PyResult<()> {
    let store = tool_configuration::get_auth_store(auth_file).map_err(RattlerBuildError::Auth)?;

    let url = Url::parse(&url).map_err(RattlerBuildError::from)?;
    let quetz_data = QuetzData::new(url, channels, api_key);

    run_async_task(async {
        upload::upload_package_to_quetz(&store, &package_files, quetz_data).await?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_files, url, channels, token, auth_file))]
fn upload_package_to_artifactory_py(
    package_files: Vec<PathBuf>,
    url: String,
    channels: String,
    token: Option<String>,
    auth_file: Option<PathBuf>,
) -> PyResult<()> {
    let store = tool_configuration::get_auth_store(auth_file).map_err(RattlerBuildError::Auth)?;
    let url = Url::parse(&url).map_err(RattlerBuildError::from)?;
    let artifactory_data = ArtifactoryData::new(url, channels, token);

    run_async_task(async {
        upload::upload_package_to_artifactory(&store, &package_files, artifactory_data).await?;
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (package_files, url, channel, api_key, auth_file, skip_existing, force=false, generate_attestation=false, attestation_file=None))]
fn upload_package_to_prefix_py(
    package_files: Vec<PathBuf>,
    url: String,
    channel: String,
    api_key: Option<String>,
    auth_file: Option<PathBuf>,
    skip_existing: bool,
    force: bool,
    generate_attestation: bool,
    attestation_file: Option<PathBuf>,
) -> PyResult<()> {
    let store = tool_configuration::get_auth_store(auth_file).map_err(RattlerBuildError::Auth)?;

    let url = Url::parse(&url).map_err(RattlerBuildError::from)?;

    // Convert attestation parameters to AttestationSource
    let attestation = match (attestation_file, generate_attestation) {
        (Some(path), false) => AttestationSource::Attestation(path),
        (None, true) => AttestationSource::GenerateAttestation,
        _ => AttestationSource::NoAttestation,
    };

    let prefix_data = PrefixData::new(
        url,
        channel,
        api_key,
        attestation,
        UploadSkipExisting(skip_existing),
        ForceOverwrite(force),
        false, // store_github_attestation
    );

    run_async_task(async {
        upload::upload_package_to_prefix(&store, &package_files, prefix_data).await?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_files, owner, channel, api_key, url, force, auth_file))]
fn upload_package_to_anaconda_py(
    package_files: Vec<PathBuf>,
    owner: String,
    channel: Option<Vec<String>>,
    api_key: Option<String>,
    url: Option<String>,
    force: bool,
    auth_file: Option<PathBuf>,
) -> PyResult<()> {
    let store = tool_configuration::get_auth_store(auth_file).map_err(RattlerBuildError::Auth)?;

    let url = url
        .map(|u| Url::parse(&u))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let anaconda_data = AnacondaData::new(owner, channel, api_key, url, ForceOverwrite(force));

    run_async_task(async {
        upload::upload_package_to_anaconda(&store, &package_files, anaconda_data).await?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_files, staging_token, feedstock, feedstock_token, staging_channel, anaconda_url, validation_endpoint, provider, dry_run))]
#[allow(clippy::too_many_arguments)]
fn upload_packages_to_conda_forge_py(
    package_files: Vec<PathBuf>,
    staging_token: String,
    feedstock: String,
    feedstock_token: String,
    staging_channel: Option<String>,
    anaconda_url: Option<String>,
    validation_endpoint: Option<String>,
    provider: Option<String>,
    dry_run: bool,
) -> PyResult<()> {
    let anaconda_url = anaconda_url
        .map(|u| Url::parse(&u))
        .transpose()
        .map_err(|e| RattlerBuildError::Other(format!("Error parsing anaconda_url: {e}")))?;

    let validation_endpoint = validation_endpoint
        .map(|u| Url::parse(&u))
        .transpose()
        .map_err(|e| {
            RattlerBuildError::Other(format!("Error parsing validation_endpoint: {e}",))
        })?;

    let conda_forge_data = CondaForgeData::new(
        staging_token,
        feedstock,
        feedstock_token,
        staging_channel,
        anaconda_url,
        validation_endpoint,
        provider,
        dry_run,
    );

    run_async_task(async {
        upload::conda_forge::upload_packages_to_conda_forge(&package_files, conda_forge_data)
            .await?;
        Ok(())
    })
}

#[pymodule]
fn rattler_build<'py>(_py: Python<'py>, m: Bound<'py, PyModule>) -> PyResult<()> {
    error::register_exceptions(_py, &m)?;
    m.add_function(wrap_pyfunction!(get_rattler_build_version_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(generate_pypi_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(generate_r_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(generate_cpan_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(generate_luarocks_recipe_string_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(parse_recipe_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build_recipes_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(test_package_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_quetz_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_artifactory_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_prefix_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_anaconda_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_packages_to_conda_forge_py, &m).unwrap())?;
    m.add_class::<PySelectorConfig>()?;

    Ok(())
}
