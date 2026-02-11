use std::path::PathBuf;

use pyo3::prelude::*;
use rattler_build::tool_configuration;
use rattler_upload::upload;
use rattler_upload::upload::opt::{
    AnacondaData, ArtifactoryData, AttestationSource, CondaForgeData, ForceOverwrite, PrefixData,
    QuetzData, SkipExisting as UploadSkipExisting,
};
use url::Url;

use crate::{error::RattlerBuildError, run_async_task};

#[pyfunction]
#[pyo3(signature = (package_files, url, channels, api_key, auth_file))]
pub fn upload_package_to_quetz_py(
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
pub fn upload_package_to_artifactory_py(
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

#[pyfunction]
#[pyo3(signature = (package_files, url, channel, api_key, auth_file, skip_existing, force, generate_attestation, attestation_file, store_github_attestation=false))]
#[allow(clippy::too_many_arguments)]
pub fn upload_package_to_prefix_py(
    package_files: Vec<PathBuf>,
    url: String,
    channel: String,
    api_key: Option<String>,
    auth_file: Option<PathBuf>,
    skip_existing: bool,
    force: bool,
    generate_attestation: bool,
    attestation_file: Option<PathBuf>,
    store_github_attestation: bool,
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
        store_github_attestation,
    );

    run_async_task(async {
        upload::upload_package_to_prefix(&store, &package_files, prefix_data).await?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (package_files, owner, channel, api_key, url, force, auth_file))]
pub fn upload_package_to_anaconda_py(
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
pub fn upload_packages_to_conda_forge_py(
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
