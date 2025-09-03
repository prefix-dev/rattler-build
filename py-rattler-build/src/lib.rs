use std::{collections::HashMap, future::Future, path::PathBuf, str::FromStr};

use ::rattler_build::{
    build_recipes, get_rattler_build_version,
    metadata::Debug,
    opt::{BuildData, ChannelPriorityWrapper, CommonData, TestData},
    recipe::parser::Recipe,
    recipe_generator::{
        CpanOpts, PyPIOpts, generate_cpan_recipe_string, generate_luarocks_recipe_string,
        generate_pypi_recipe_string, generate_r_recipe_string,
    },
    run_test,
    selectors::SelectorConfig,
    tool_configuration::{self, ContinueOnFailure, SkipExisting, TestStrategy},
};
use clap::ValueEnum;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::{ConfigBase, build::PackageFormatAndCompression};
use rattler_upload::upload;
use rattler_upload::upload::opt::{
    AnacondaData, ArtifactoryData, CondaForgeData, PrefixData, QuetzData,
};
use url::Url;

/// Execute async tasks in Python bindings with proper error handling
fn run_async_task<F, R>(future: F) -> PyResult<R>
where
    F: Future<Output = miette::Result<R>>,
{
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to create async runtime: {}", e)))?;

    rt.block_on(async {
        future
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    })
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
#[pyo3(signature = (yaml_content, target_platform=None, host_platform=None, build_platform=None, experimental=None, allow_undefined=None))]
fn parse_recipe_py(
    yaml_content: String,
    target_platform: Option<String>,
    host_platform: Option<String>,
    build_platform: Option<String>,
    experimental: Option<bool>,
    allow_undefined: Option<bool>,
) -> PyResult<Py<PyAny>> {
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
        .unwrap_or_else(Platform::current);

    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
        .unwrap_or_else(Platform::current);

    let build_platform = build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
        .unwrap_or_else(Platform::current);

    let selector_config = SelectorConfig {
        target_platform,
        host_platform,
        build_platform,
        experimental: experimental.unwrap_or(false),
        allow_undefined: allow_undefined.unwrap_or(false),
        ..Default::default()
    };

    match Recipe::from_yaml(yaml_content.as_str(), selector_config) {
        Ok(recipe) => {
            let json_value = serde_json::to_value(recipe).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to serialize recipe: {}", e))
            })?;

            Python::attach(|py| {
                pythonize::pythonize(py, &json_value)
                    .map(|obj| obj.into())
                    .map_err(|e| {
                        PyRuntimeError::new_err(format!("Failed to convert to Python: {}", e))
                    })
            })
        }
        Err(errors) => Err(PyRuntimeError::new_err(format!(
            "Recipe parsing failed: {:?}",
            errors
        ))),
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
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
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
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let test = test.map(|t| TestStrategy::from_str(&t, false).unwrap());
    let skip_existing = skip_existing.map(|s| SkipExisting::from_str(&s, false).unwrap());
    let noarch_build_platform = noarch_build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
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
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
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
                        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
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
    let store = tool_configuration::get_auth_store(auth_file)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

    let url = Url::parse(&url).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
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
    let store = tool_configuration::get_auth_store(auth_file)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

    let url = Url::parse(&url).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let artifactory_data = ArtifactoryData::new(url, channels, token);

    run_async_task(async {
        upload::upload_package_to_artifactory(&store, &package_files, artifactory_data).await?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_files, url, channel, api_key, auth_file, skip_existing, attestation_file=None,))]
fn upload_package_to_prefix_py(
    package_files: Vec<PathBuf>,
    url: String,
    channel: String,
    api_key: Option<String>,
    auth_file: Option<PathBuf>,
    skip_existing: bool,
    attestation_file: Option<PathBuf>,
) -> PyResult<()> {
    let store = tool_configuration::get_auth_store(auth_file)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

    let url = Url::parse(&url).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let prefix_data = PrefixData::new(url, channel, api_key, attestation_file, skip_existing);

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
    let store = tool_configuration::get_auth_store(auth_file)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

    let url = url
        .map(|u| Url::parse(&u))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let anaconda_data = AnacondaData::new(owner, channel, api_key, url, force);

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
        .map_err(|e| PyRuntimeError::new_err(format!("Error parsing anaconda_url: {e}")))?;

    let validation_endpoint = validation_endpoint
        .map(|u| Url::parse(&u))
        .transpose()
        .map_err(|e| PyRuntimeError::new_err(format!("Error parsing validation_endpoint: {e}",)))?;

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

    Ok(())
}
