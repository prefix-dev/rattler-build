use miette::IntoDiagnostic;
use rattler_conda_types::NamedChannelOrUrl;
use rattler_index::{IndexFsConfig, index_fs};
use std::fs;
use std::path::{Path, PathBuf};

use crate::opt::BuildIntoData;
use crate::tool_configuration;

/// Helper function to determine the package subdirectory (platform)
fn determine_package_subdir(package_path: &Path) -> miette::Result<String> {
    use rattler_conda_types::package::IndexJson;
    use rattler_package_streaming::seek::read_package_file;

    let index_json: IndexJson = read_package_file(package_path)
        .map_err(|e| miette::miette!("Failed to read package file: {}", e))?;

    Ok(index_json.subdir.unwrap_or_else(|| "noarch".to_string()))
}

/// Upload packages to a remote channel and run indexing
pub(crate) async fn upload_to_remote_channel(
    target_url: &NamedChannelOrUrl,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    match target_url {
        NamedChannelOrUrl::Url(url) => {
            let scheme = url.scheme();

            match scheme {
                "s3" => {
                    #[cfg(not(feature = "s3"))]
                    {
                        return Err(miette::miette!(
                            "S3 support is not enabled. Please recompile with the 's3' feature."
                        ));
                    }

                    #[cfg(feature = "s3")]
                    {
                        upload_to_s3(url, package_paths, build_into_data).await
                    }
                }
                "quetz" => upload_to_quetz(url, package_paths, build_into_data).await,
                "artifactory" => upload_to_artifactory(url, package_paths, build_into_data).await,
                "prefix" => upload_to_prefix(url, package_paths, build_into_data).await,
                "file" => {
                    let path = PathBuf::from(url.path());
                    upload_to_local_filesystem(&path, package_paths, build_into_data).await
                }
                "http" | "https" => {
                    // Detect backend from hostname
                    let host = url.host_str().unwrap_or("");

                    if host.contains("prefix.dev") {
                        upload_to_prefix(url, package_paths, build_into_data).await
                    } else if host.contains("anaconda.org") {
                        upload_to_anaconda(url, package_paths, build_into_data).await
                    } else if host.contains("quetz") {
                        upload_to_quetz(url, package_paths, build_into_data).await
                    } else {
                        Err(miette::miette!(
                            "Cannot determine upload backend from URL '{}'. \n\
                            Supported hosts: prefix.dev, anaconda.org, or use explicit schemes: s3://, quetz://, artifactory://, prefix://",
                            url
                        ))
                    }
                }
                _ => Err(miette::miette!(
                    "Unsupported URL scheme '{}'. Supported schemes: file://, s3://, quetz://, artifactory://, prefix://, http://, https://",
                    scheme
                )),
            }
        }
        NamedChannelOrUrl::Path(path) => {
            let path_buf = PathBuf::from(path.as_str());
            upload_to_local_filesystem(&path_buf, package_paths, build_into_data).await
        }
        NamedChannelOrUrl::Name(name) => Err(miette::miette!(
            "Cannot upload to named channel '{}'. Please use a direct URL instead.",
            name
        )),
    }
}

#[cfg(feature = "s3")]
/// Upload packages to S3 and run indexing
async fn upload_to_s3(
    url: &url::Url,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    use rattler_index::{IndexS3Config, index_s3};
    use rattler_upload::upload::upload_package_to_s3;

    tracing::info!("Uploading packages to S3 channel: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(build_into_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Upload packages to S3 (credentials come from AWS SDK default chain)
    upload_package_to_s3(
        &auth_storage,
        url.clone(),
        None, // Use default AWS credential chain
        &package_paths.to_vec(),
        false, // force
    )
    .await
    .map_err(|e| miette::miette!("Failed to upload packages to S3: {}", e))?;

    tracing::info!("Successfully uploaded packages to S3");

    // Run S3 indexing
    tracing::info!("Indexing S3 channel at {}", url);

    // Use default AWS credential chain
    let resolved_credentials = rattler_s3::ResolvedS3Credentials::from_sdk()
        .await
        .into_diagnostic()?;

    let index_config = IndexS3Config {
        channel: url.clone(),
        credentials: resolved_credentials,
        target_platform: Some(build_into_data.build.target_platform),
        repodata_patch: None,
        write_zst: false,
        write_shards: false,
        force: false,
        max_parallel: num_cpus::get_physical(),
        multi_progress: None,
        precondition_checks: Default::default(),
    };

    index_s3(index_config)
        .await
        .map_err(|e| miette::miette!("Failed to index S3 channel: {}", e))?;

    tracing::info!("Successfully indexed S3 channel");
    Ok(())
}

/// Upload packages to Quetz server
async fn upload_to_quetz(
    url: &url::Url,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::QuetzData;
    use rattler_upload::upload::upload_package_to_quetz;

    tracing::info!("Uploading packages to Quetz server: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(build_into_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Extract channel name from URL path
    let channel = url
        .path_segments()
        .and_then(|segments| segments.last())
        .ok_or_else(|| miette::miette!("Invalid Quetz URL: missing channel name"))?
        .to_string();

    // Convert quetz:// to https:// if needed, otherwise use as-is
    let server_url = if url.scheme() == "quetz" {
        let mut converted = url.clone();
        converted
            .set_scheme("https")
            .map_err(|_| miette::miette!("Failed to convert quetz:// URL to https://"))?;
        converted
    } else {
        url.clone()
    };

    // Create QuetzData with server URL, channel, and optional API key
    let quetz_data = QuetzData::new(server_url, channel, None);

    // Upload packages
    upload_package_to_quetz(&auth_storage, &package_paths.to_vec(), quetz_data)
        .await
        .map_err(|e| miette::miette!("Failed to upload packages to Quetz: {}", e))?;

    tracing::info!("Successfully uploaded packages to Quetz");
    tracing::info!("Note: Quetz handles indexing automatically on the server side");
    Ok(())
}

/// Upload packages to Artifactory server
async fn upload_to_artifactory(
    url: &url::Url,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::ArtifactoryData;
    use rattler_upload::upload::upload_package_to_artifactory;

    tracing::info!("Uploading packages to Artifactory server: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(build_into_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Extract channel name from URL path
    let channel = url
        .path_segments()
        .and_then(|segments| segments.last())
        .ok_or_else(|| miette::miette!("Invalid Artifactory URL: missing repository name"))?
        .to_string();

    // Convert artifactory:// to https:// if needed, otherwise use as-is
    let server_url = if url.scheme() == "artifactory" {
        let mut converted = url.clone();
        converted
            .set_scheme("https")
            .map_err(|_| miette::miette!("Failed to convert artifactory:// URL to https://"))?;
        converted
    } else {
        url.clone()
    };

    // Create ArtifactoryData with server URL, channel, and optional token
    let artifactory_data = ArtifactoryData::new(server_url, channel, None);

    // Upload packages
    upload_package_to_artifactory(&auth_storage, &package_paths.to_vec(), artifactory_data)
        .await
        .map_err(|e| miette::miette!("Failed to upload packages to Artifactory: {}", e))?;

    tracing::info!("Successfully uploaded packages to Artifactory");
    tracing::info!("Note: Artifactory handles indexing automatically on the server side");
    Ok(())
}

/// Upload packages to Prefix.dev server
async fn upload_to_prefix(
    url: &url::Url,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::PrefixData;
    use rattler_upload::upload::upload_package_to_prefix;

    tracing::info!("Uploading packages to Prefix.dev server: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(build_into_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Extract channel name from URL path
    let channel = url
        .path_segments()
        .and_then(|segments| segments.last())
        .ok_or_else(|| miette::miette!("Invalid Prefix URL: missing channel name"))?
        .to_string();

    // Convert prefix:// to https:// if needed, otherwise use as-is
    let server_url = if url.scheme() == "prefix" {
        let mut converted = url.clone();
        converted
            .set_scheme("https")
            .map_err(|_| miette::miette!("Failed to convert prefix:// URL to https://"))?;
        converted
    } else {
        url.clone()
    };

    // Create PrefixData with server URL, channel, optional API key, no attestation, and skip_existing=false
    let prefix_data = PrefixData::new(server_url, channel, None, None, false);

    // Upload packages
    upload_package_to_prefix(&auth_storage, &package_paths.to_vec(), prefix_data)
        .await
        .map_err(|e| miette::miette!("Failed to upload packages to Prefix: {}", e))?;

    tracing::info!("Successfully uploaded packages to Prefix.dev");
    tracing::info!("Note: Prefix.dev handles indexing automatically on the server side");
    Ok(())
}

/// Upload packages to Anaconda.org
async fn upload_to_anaconda(
    url: &url::Url,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::AnacondaData;
    use rattler_upload::upload::upload_package_to_anaconda;

    tracing::info!("Uploading packages to Anaconda.org: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(build_into_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Parse URL path to extract owner and optional channel
    // Expected format: https://anaconda.org/owner/channel or https://anaconda.org/owner
    let path_segments: Vec<&str> = url
        .path_segments()
        .ok_or_else(|| miette::miette!("Invalid Anaconda.org URL: missing path"))?
        .collect();

    let (owner, channel) = match path_segments.len() {
        1 => (path_segments[0].to_string(), None),
        2 => (
            path_segments[0].to_string(),
            Some(path_segments[1].to_string()),
        ),
        _ => {
            return Err(miette::miette!(
                "Invalid Anaconda.org URL format. Expected: https://anaconda.org/owner or https://anaconda.org/owner/channel"
            ));
        }
    };

    // Create AnacondaData with owner, optional channel, API key, URL, and force flag
    let anaconda_data = AnacondaData::new(
        owner,
        channel,
        None, // API key from auth storage
        Some(url.clone()),
        false, // force
    );

    // Upload packages
    upload_package_to_anaconda(&auth_storage, &package_paths.to_vec(), anaconda_data)
        .await
        .map_err(|e| miette::miette!("Failed to upload packages to Anaconda.org: {}", e))?;

    tracing::info!("Successfully uploaded packages to Anaconda.org");
    tracing::info!("Note: Anaconda.org handles indexing automatically on the server side");
    Ok(())
}

/// Upload packages to local filesystem and run indexing
async fn upload_to_local_filesystem(
    target_dir: &Path,
    package_paths: &[PathBuf],
    build_into_data: &BuildIntoData,
) -> miette::Result<()> {
    tracing::info!(
        "Copying packages to local channel: {}",
        target_dir.display()
    );

    // Copy packages to the target directory organized by platform
    for package_path in package_paths {
        // Extract platform from package filename or metadata
        let package_name = package_path
            .file_name()
            .ok_or_else(|| miette::miette!("Invalid package path"))?;

        // Determine subdir from package
        let subdir = determine_package_subdir(package_path)?;
        let target_subdir = target_dir.join(&subdir);

        fs::create_dir_all(&target_subdir).into_diagnostic()?;
        let target_path = target_subdir.join(package_name);

        tracing::info!(
            "Copying {} to {}",
            package_path.display(),
            target_path.display()
        );
        fs::copy(package_path, &target_path).into_diagnostic()?;
    }

    // Run rattler-index on the local directory
    tracing::info!("Indexing local channel at {}", target_dir.display());
    let index_config = IndexFsConfig {
        channel: target_dir.to_path_buf(),
        target_platform: Some(build_into_data.build.target_platform),
        repodata_patch: None,
        write_zst: false,
        write_shards: false,
        force: false,
        max_parallel: num_cpus::get_physical(),
        multi_progress: None,
    };

    index_fs(index_config)
        .await
        .map_err(|e| miette::miette!("Failed to index channel: {}", e))?;
    tracing::info!("Successfully indexed local channel");
    Ok(())
}
