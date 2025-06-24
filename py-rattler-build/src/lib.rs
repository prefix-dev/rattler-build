use std::{path::PathBuf, str::FromStr};

use ::rattler_build::{
    build_recipes, get_rattler_build_version,
    metadata::Debug,
    opt::{
        AnacondaData, ArtifactoryData, BuildData, ChannelPriorityWrapper, CommonData,
        CondaForgeData, PrefixData, QuetzData, TestData,
    },
    run_test,
    tool_configuration::{self, SkipExisting, TestStrategy},
    upload,
};
use clap::ValueEnum;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::{ConfigBase, build::PackageFormatAndCompression};
use url::Url;

// Bind the get version function to the Python module
#[pyfunction]
fn get_rattler_build_version_py() -> PyResult<String> {
    Ok(get_rattler_build_version().to_string())
}

#[pyfunction]
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, ignore_recipe_variants, render_only, with_solve, keep_build, no_build_id, package_format, compression_threads, io_concurrency_limit, no_include_recipe, test, output_dir, auth_file, channel_priority, skip_existing, noarch_build_platform, allow_insecure_host=None, continue_on_failure=false, debug=false))]
#[allow(clippy::too_many_arguments)]
fn build_recipes_py(
    recipes: Vec<PathBuf>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<PathBuf>>,
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
        false,
        skip_existing,
        noarch_build_platform,
        None,
        None,
        Debug::new(debug),
        continue_on_failure.into(),
        false,
        false,
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = build_recipes(recipes, build_data, &None).await {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (package_file, channel, compression_threads, auth_file, channel_priority, allow_insecure_host=None, debug=false, test_index=None))]
fn test_package_py(
    package_file: PathBuf,
    channel: Option<Vec<String>>,
    compression_threads: Option<u32>,
    auth_file: Option<PathBuf>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    debug: bool,
    test_index: Option<usize>,
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

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = run_test(test_data, None).await {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
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

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = upload::upload_package_to_quetz(&store, &package_files, quetz_data).await {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
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

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) =
            upload::upload_package_to_artifactory(&store, &package_files, artifactory_data).await
        {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
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

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = upload::upload_package_to_prefix(&store, &package_files, prefix_data).await
        {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
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

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) =
            upload::upload_package_to_anaconda(&store, &package_files, anaconda_data).await
        {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
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

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) =
            upload::conda_forge::upload_packages_to_conda_forge(&package_files, conda_forge_data)
                .await
        {
            return Err(PyRuntimeError::new_err(e.to_string()));
        }
        Ok(())
    })
}

#[pymodule]
fn rattler_build<'py>(_py: Python<'py>, m: Bound<'py, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_rattler_build_version_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(build_recipes_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(test_package_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_quetz_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_artifactory_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_prefix_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_package_to_anaconda_py, &m).unwrap())?;
    m.add_function(wrap_pyfunction!(upload_packages_to_conda_forge_py, &m).unwrap())?;

    Ok(())
}
