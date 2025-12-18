//! Functions for publishing conda packages to various backends (local filesystem, S3, Quetz, etc.)

use miette::IntoDiagnostic;
use rattler_conda_types::{
    Channel, ChannelUrl, MatchSpec, NamedChannelOrUrl, PackageName, Platform,
};
use rattler_index::{IndexFsConfig, index_fs};
use rattler_repodata_gateway::{CacheClearMode, Gateway, SubdirSelection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::opt::PublishData;
use crate::recipe::parser::BuildString;
use crate::render::reporters::GatewayReporter;
use crate::tool_configuration::{self, Configuration};
use crate::types::Output;

/// Represents a parsed build number argument
#[derive(Debug, Clone)]
pub(crate) enum BuildNumberOverride {
    /// Absolute build number (e.g., "12")
    Absolute(u64),
    /// Relative bump (e.g., "+1")
    Relative(i64),
}

impl BuildNumberOverride {
    /// Parse a build number string into either absolute or relative form
    pub(crate) fn parse(s: &str) -> miette::Result<Self> {
        let s = s.trim();
        if let Some(stripped) = s.strip_prefix('+') {
            let bump: i64 = stripped
                .parse()
                .map_err(|e| miette::miette!("Invalid relative build number '{}': {}", s, e))?;
            Ok(BuildNumberOverride::Relative(bump))
        } else if let Some(stripped) = s.strip_prefix('-') {
            let bump: i64 = stripped
                .parse::<i64>()
                .map_err(|e| miette::miette!("Invalid relative build number '{}': {}", s, e))?;
            Ok(BuildNumberOverride::Relative(-bump))
        } else {
            let num: u64 = s
                .parse()
                .map_err(|e| miette::miette!("Invalid absolute build number '{}': {}", s, e))?;
            Ok(BuildNumberOverride::Absolute(num))
        }
    }
}

/// Fetch the highest build number for packages from the target channel
pub(crate) async fn fetch_highest_build_numbers(
    target_url: &NamedChannelOrUrl,
    outputs: &[Output],
    target_platform: Platform,
    tool_config: &Configuration,
) -> miette::Result<HashMap<(PackageName, String), u64>> {
    // Convert target URL to channel
    let channel = match target_url {
        NamedChannelOrUrl::Url(url) => Channel::from_url(ChannelUrl::from(url.clone())),
        NamedChannelOrUrl::Path(path) => {
            let url = url::Url::from_file_path(path.as_str())
                .map_err(|_| miette::miette!("Invalid path: {}", path))?;
            Channel::from_url(ChannelUrl::from(url))
        }
        NamedChannelOrUrl::Name(name) => {
            return Err(miette::miette!(
                "Cannot fetch repodata from named channel '{}'. Please use a URL.",
                name
            ));
        }
    };

    // Collect unique package names from outputs (we'll filter by version later)
    let mut package_specs: Vec<MatchSpec> = Vec::new();
    let mut versions_to_check: HashMap<PackageName, Vec<String>> = HashMap::new();

    for output in outputs {
        let name = output.name().clone();
        let version = output.recipe.package().version().to_string();

        // Track versions we're interested in
        versions_to_check
            .entry(name.clone())
            .or_default()
            .push(version);

        // Create a matchspec that matches the package name (any version)
        let spec = MatchSpec {
            name: Some(rattler_conda_types::PackageNameMatcher::Exact(name)),
            ..Default::default()
        };
        if !package_specs.iter().any(|s| s.name == spec.name) {
            package_specs.push(spec);
        }
    }

    if package_specs.is_empty() {
        return Ok(HashMap::new());
    }

    let span = tracing::info_span!("Fetching build numbers from target channel",);
    let _guard = span.enter();

    // Query the repodata
    let result = tool_config
        .repodata_gateway
        .query(
            vec![channel],
            [target_platform, Platform::NoArch],
            package_specs,
        )
        .with_reporter(
            GatewayReporter::builder()
                .with_multi_progress(tool_config.fancy_log_handler.multi_progress().clone())
                .with_progress_template(tool_config.fancy_log_handler.default_bytes_style())
                .with_finish_template(tool_config.fancy_log_handler.finished_progress_style())
                .finish(),
        )
        .recursive(false)
        .await;

    tool_config
        .fancy_log_handler
        .multi_progress()
        .clear()
        .unwrap();

    // Process results to find highest build numbers
    let mut highest_build_numbers: HashMap<(PackageName, String), u64> = HashMap::new();

    match result {
        Ok(repo_data) => {
            for repo in repo_data {
                for record in repo.iter() {
                    let name = &record.package_record.name;
                    let version = record.package_record.version.version().to_string();

                    // Only track versions we're actually building
                    if let Some(versions) = versions_to_check.get(name)
                        && versions.contains(&version)
                    {
                        let key = (name.clone(), version);
                        let build_number = record.package_record.build_number;
                        highest_build_numbers
                            .entry(key)
                            .and_modify(|e| *e = (*e).max(build_number))
                            .or_insert(build_number);
                    }
                }
            }
        }
        Err(e) => {
            // Log the error but don't fail - the channel might not exist yet or be empty
            tracing::debug!("Could not fetch repodata from target channel: {}", e);
        }
    }

    Ok(highest_build_numbers)
}

/// Apply build number override to outputs
pub(crate) fn apply_build_number_override(
    outputs: &mut [Output],
    build_number_override: &BuildNumberOverride,
    highest_build_numbers: &HashMap<(PackageName, String), u64>,
) {
    let span = tracing::info_span!("Applying build number overrides",);
    let _guard = span.enter();
    for output in outputs {
        let name = output.name().clone();
        let version = output.recipe.package().version().to_string();
        let key = (name.clone(), version.clone());

        let new_build_number = match build_number_override {
            BuildNumberOverride::Absolute(num) => *num,
            BuildNumberOverride::Relative(bump) => {
                let current_highest = highest_build_numbers.get(&key).copied().unwrap_or(0);
                let new_num = (current_highest as i64 + bump).max(0) as u64;
                tracing::info!(
                    "Packaging {} ({}): bumping build number from {} to {} ({}{})",
                    name.as_normalized(),
                    version,
                    current_highest,
                    new_num,
                    if *bump >= 0 { "+" } else { "" },
                    bump
                );
                new_num
            }
        };

        // Update the build number
        output.recipe.build.number = new_build_number;

        // Extract the hash from the current build string and recompute with new build number
        let current_build_string = output
            .recipe
            .build
            .string
            .as_resolved()
            .expect("Build string should be resolved at this point");

        // Split on last '_' to separate hash from build number
        if let Some(last_underscore) = current_build_string.rfind('_') {
            let hash_part = &current_build_string[..last_underscore];
            let new_build_string = format!("{}_{}", hash_part, new_build_number);
            output.recipe.build.string = BuildString::Resolved(new_build_string);
        }
    }
}

/// Helper function to determine the package subdirectory (platform)
pub fn determine_package_subdir(package_path: &Path) -> miette::Result<String> {
    use rattler_conda_types::package::IndexJson;
    use rattler_package_streaming::seek::read_package_file;

    let index_json: IndexJson = read_package_file(package_path)
        .map_err(|e| miette::miette!("Failed to read package file: {}", e))?;

    Ok(index_json.subdir.unwrap_or_else(|| "noarch".to_string()))
}

/// Upload packages to a channel and run indexing.
/// After the indexing, the repodata cache for the target channel is cleared.
pub(crate) async fn upload_and_index_channel(
    target_url: &NamedChannelOrUrl,
    package_paths: &[PathBuf],
    publish_data: &PublishData,
    repodata_gateway: &Gateway,
) -> miette::Result<()> {
    let span = tracing::info_span!("Publishing packages");
    let _guard = span.enter();

    // Collect subdirs from packages for cache clearing later
    let subdirs: std::collections::HashSet<String> = package_paths
        .iter()
        .filter_map(|p| determine_package_subdir(p).ok())
        .collect();

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
                        upload_to_s3(url, package_paths, publish_data).await
                    }
                }
                "quetz" => upload_to_quetz(url, package_paths, publish_data).await,
                "artifactory" => upload_to_artifactory(url, package_paths, publish_data).await,
                "prefix" => upload_to_prefix(url, package_paths, publish_data).await,
                "file" => {
                    let path = url
                        .to_file_path()
                        .map_err(|()| miette::miette!("Invalid file URL: {}", url))?;
                    upload_to_local_filesystem(&path, package_paths, publish_data.force).await
                }
                "http" | "https" => {
                    // Detect backend from hostname
                    let host = url.host_str().unwrap_or("");

                    if host.contains("prefix.dev") {
                        upload_to_prefix(url, package_paths, publish_data).await
                    } else if host.contains("anaconda.org") {
                        upload_to_anaconda(url, package_paths, publish_data).await
                    } else if host.contains("quetz") {
                        upload_to_quetz(url, package_paths, publish_data).await
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
            upload_to_local_filesystem(&path_buf, package_paths, publish_data.force).await
        }
        NamedChannelOrUrl::Name(name) => Err(miette::miette!(
            "Cannot upload to named channel '{}'. Please use a direct URL instead.",
            name
        )),
    }?;

    // Clear repodata cache for the target channel after publishing
    let channel = match target_url {
        NamedChannelOrUrl::Url(url) => Channel::from_url(ChannelUrl::from(url.clone())),
        NamedChannelOrUrl::Path(path) => {
            let url = url::Url::from_file_path(path.as_str())
                .map_err(|_| miette::miette!("Invalid path: {}", path))?;
            Channel::from_url(ChannelUrl::from(url))
        }
        NamedChannelOrUrl::Name(_) => {
            // Named channels are already rejected above, so this is unreachable
            unreachable!()
        }
    };

    repodata_gateway
        .clear_repodata_cache(
            &channel,
            SubdirSelection::Some(subdirs),
            CacheClearMode::InMemoryAndDisk,
        )
        .into_diagnostic()?;

    tracing::debug!("Cleared repodata cache for target channel");

    Ok(())
}

#[cfg(feature = "s3")]
/// Upload packages to S3 and run indexing
async fn upload_to_s3(
    url: &url::Url,
    package_paths: &[PathBuf],
    publish_data: &PublishData,
) -> miette::Result<()> {
    use rattler_index::{IndexS3Config, ensure_channel_initialized_s3, index_s3};
    use rattler_networking::s3_middleware;
    use rattler_upload::upload::upload_package_to_s3;
    use std::collections::HashSet;

    tracing::info!("Uploading packages to S3 channel: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(publish_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Resolve S3 credentials using config + auth storage, falling back to AWS SDK
    let resolved_credentials = tool_configuration::resolve_s3_credentials(
        &publish_data.build.common.s3_config,
        publish_data.build.common.auth_file.clone(),
        url,
    )
    .await
    .into_diagnostic()?;

    // Create S3Credentials from the config if available (for upload_package_to_s3)
    let bucket_name = url.host_str().unwrap_or_default();
    let s3_credentials = publish_data
        .build
        .common
        .s3_config
        .get(bucket_name)
        .and_then(|config| {
            if let s3_middleware::S3Config::Custom {
                endpoint_url,
                region,
                force_path_style,
            } = config
            {
                Some(rattler_s3::S3Credentials {
                    endpoint_url: endpoint_url.clone(),
                    region: region.clone(),
                    addressing_style: if *force_path_style {
                        rattler_s3::S3AddressingStyle::Path
                    } else {
                        rattler_s3::S3AddressingStyle::VirtualHost
                    },
                    access_key_id: None,
                    secret_access_key: None,
                    session_token: None,
                })
            } else {
                None
            }
        });

    // Ensure channel is initialized with noarch/repodata.json
    ensure_channel_initialized_s3(url, &resolved_credentials)
        .await
        .map_err(|e| miette::miette!("Failed to initialize S3 channel: {}", e))?;

    // Collect unique subdirs from all packages
    let mut subdirs = HashSet::new();
    for package_path in package_paths {
        let subdir = determine_package_subdir(package_path)?;
        subdirs.insert(subdir);
    }

    // Upload packages to S3
    upload_package_to_s3(
        &auth_storage,
        url.clone(),
        s3_credentials,
        &package_paths.to_vec(),
        publish_data.force,
    )
    .await
    .map_err(|e| miette::miette!("Failed to upload packages to S3: {}", e))?;

    tracing::info!("Successfully uploaded packages to S3");

    for subdir in subdirs {
        // Run S3 indexing for each subdir
        tracing::info!("Indexing S3 channel at {} / {}", url, subdir);

        let target_platform = subdir
            .parse::<Platform>()
            .map_err(|e| miette::miette!("Invalid platform subdir '{}': {}", subdir, e))?;

        let index_config = IndexS3Config {
            channel: url.clone(),
            credentials: resolved_credentials.clone(),
            target_platform: Some(target_platform),
            repodata_patch: None,
            write_zst: true,
            write_shards: true,
            force: false,
            max_parallel: num_cpus::get_physical(),
            multi_progress: None,
            precondition_checks: rattler_index::PreconditionChecks::Enabled,
        };

        index_s3(index_config)
            .await
            .map_err(|e| miette::miette!("Failed to index S3 channel: {}", e))?;
    }

    tracing::info!("Successfully indexed S3 channel");
    Ok(())
}

/// Upload packages to Quetz server
async fn upload_to_quetz(
    url: &url::Url,
    package_paths: &[PathBuf],
    publish_data: &PublishData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::QuetzData;
    use rattler_upload::upload::upload_package_to_quetz;

    tracing::info!("Uploading packages to Quetz server: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(publish_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Extract channel name from URL path
    let channel = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
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
    publish_data: &PublishData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::ArtifactoryData;
    use rattler_upload::upload::upload_package_to_artifactory;

    tracing::info!("Uploading packages to Artifactory server: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(publish_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Extract channel name from URL path
    let channel = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
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
    publish_data: &PublishData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::{
        AttestationSource, ForceOverwrite, PrefixData, SkipExisting,
    };
    use rattler_upload::upload::upload_package_to_prefix;

    tracing::info!("Uploading packages to Prefix.dev server: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(publish_data.build.common.auth_file.clone())
            .map_err(|e| miette::miette!("Failed to get authentication storage: {}", e))?;

    // Extract channel name from URL path
    let channel = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
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

    // Determine attestation source
    let attestation = if publish_data.generate_attestation {
        AttestationSource::GenerateAttestation
    } else {
        AttestationSource::NoAttestation
    };

    // Create PrefixData with server URL, channel, optional API key, attestation, skip_existing and force
    let prefix_data = PrefixData::new(
        server_url,
        channel,
        None,
        attestation,
        SkipExisting(false),
        ForceOverwrite(publish_data.force),
        false, // store_github_attestation
    );

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
    publish_data: &PublishData,
) -> miette::Result<()> {
    use rattler_upload::upload::opt::{AnacondaData, ForceOverwrite};
    use rattler_upload::upload::upload_package_to_anaconda;

    tracing::info!("Uploading packages to Anaconda.org: {}", url);

    // Get authentication storage
    let auth_storage =
        tool_configuration::get_auth_store(publish_data.build.common.auth_file.clone())
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
        channel.map(|c| vec![c]), // Automatically uses "main" channel if not specified
        None,                     // API key from auth storage
        Some(url.clone()),
        ForceOverwrite(publish_data.force),
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
    force: bool,
) -> miette::Result<()> {
    use rattler_index::ensure_channel_initialized_fs;
    use std::collections::HashSet;

    tracing::info!(
        "Copying packages to local channel: {}",
        target_dir.display()
    );

    // Create target directory if it doesn't exist
    fs_err::create_dir_all(target_dir).into_diagnostic()?;

    // Ensure channel is initialized with noarch/repodata.json
    ensure_channel_initialized_fs(target_dir)
        .await
        .map_err(|e| miette::miette!("Failed to initialize local channel: {}", e))?;

    // Collect unique subdirs from all packages
    let mut subdirs = HashSet::new();

    // Copy packages to the target directory organized by platform
    for package_path in package_paths {
        // Extract platform from package filename or metadata
        let package_name = package_path
            .file_name()
            .ok_or_else(|| miette::miette!("Invalid package path"))?;

        // Determine subdir from package
        let subdir = determine_package_subdir(package_path)?;
        subdirs.insert(subdir.clone());
        let target_subdir = target_dir.join(&subdir);

        fs_err::create_dir_all(&target_subdir).into_diagnostic()?;
        let target_path = target_subdir.join(package_name);

        // Check if package already exists
        if target_path.exists() && !force {
            return Err(miette::miette!(
                "Package already exists at {}. Use --force to overwrite.",
                target_path.display()
            ));
        }

        tracing::info!(
            "Copying {} to {}",
            package_path.display(),
            target_path.display()
        );
        fs_err::copy(package_path, &target_path).into_diagnostic()?;
    }

    // Run rattler-index on the local directory for each subdir
    tracing::info!("Indexing local channel at {}", target_dir.display());

    for subdir in subdirs {
        let target_platform = subdir
            .parse::<Platform>()
            .map_err(|e| miette::miette!("Invalid platform subdir '{}': {}", subdir, e))?;

        let index_config = IndexFsConfig {
            channel: target_dir.to_path_buf(),
            target_platform: Some(target_platform),
            repodata_patch: None,
            write_zst: true,
            write_shards: true,
            force: false,
            max_parallel: num_cpus::get_physical(),
            multi_progress: None,
        };

        index_fs(index_config)
            .await
            .map_err(|e| miette::miette!("Failed to index channel: {}", e))?;
    }

    tracing::info!("Successfully indexed local channel");
    Ok(())
}
