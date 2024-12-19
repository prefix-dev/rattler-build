//! Conda-forge package uploader.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use miette::{miette, IntoDiagnostic};
use tracing::{debug, info};

use crate::{opt::CondaForgeOpts, upload::get_default_client};

use super::{
    anaconda,
    package::{self},
};

async fn get_channel_target_from_variant_config(
    variant_config_path: &Path,
) -> miette::Result<String> {
    let variant_config = tokio::fs::read_to_string(variant_config_path)
        .await
        .into_diagnostic()?;

    let variant_config: serde_yaml::Value =
        serde_yaml::from_str(&variant_config).into_diagnostic()?;

    let channel_target = variant_config
        .get("channel_targets")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            miette!("\"channel_targets\" not found or invalid format in variant_config")
        })?;

    let (channel, label) = channel_target
        .split_once(' ')
        .ok_or_else(|| miette!("Invalid channel_target format"))?;

    if channel != "conda-forge" {
        return Err(miette!("channel_target is not a conda-forge channel"));
    }

    Ok(label.to_string())
}

/// Uploads the package conda forge.
pub async fn upload_packages_to_conda_forge(
    opts: CondaForgeOpts,
    package_files: &Vec<PathBuf>,
) -> miette::Result<()> {
    let anaconda = anaconda::Anaconda::new(opts.staging_token, opts.anaconda_url.into());

    let mut channels: HashMap<String, HashMap<_, _>> = HashMap::new();

    for package_file in package_files {
        let package = package::ExtractedPackage::from_package_file(package_file)?;

        let variant_config_path = package
            .extraction_dir()
            .join("info")
            .join("recipe")
            .join("variant_config.yaml");

        let channel = get_channel_target_from_variant_config(&variant_config_path)
            .await
            .map_err(|e| {
                miette!(
                    "Failed to get channel_targets from variant config for {}: {}",
                    package.path().display(),
                    e
                )
            })?;

        if !opts.dry_run {
            anaconda
                .create_or_update_package(&opts.staging_channel, &package)
                .await?;

            anaconda
                .create_or_update_release(&opts.staging_channel, &package)
                .await?;

            anaconda
                .upload_file(&opts.staging_channel, &[channel.clone()], false, &package)
                .await?;
        } else {
            debug!(
                "Would have uploaded {} to anaconda.org {}/{}",
                package.path().display(),
                opts.staging_channel,
                channel
            );
        };

        let dist_name = format!(
            "{}/{}",
            package.subdir().ok_or(miette::miette!("No subdir found"))?,
            package
                .filename()
                .ok_or(miette::miette!("No filename found"))?
        );

        channels
            .entry(channel)
            .or_default()
            .insert(dist_name, package.sha256().into_diagnostic()?);
    }

    for (channel, checksums) in channels {
        info!("Uploading packages for conda-forge channel {}", channel);

        let payload = serde_json::json!({
            "feedstock": opts.feedstock,
            "outputs": checksums,
            "channel": channel,
            "comment_on_error": opts.post_comment_on_error,
            "hash_type": "sha256",
            "provider": opts.provider
        });

        let client = get_default_client().into_diagnostic()?;

        debug!(
            "Sending payload to validation endpoint: {}",
            serde_json::to_string_pretty(&payload).into_diagnostic()?
        );

        if opts.dry_run {
            debug!(
                "Would have sent payload to validation endpoint {}",
                opts.validation_endpoint
            );

            continue;
        }

        let resp = client
            .post(opts.validation_endpoint.clone())
            .json(&payload)
            .header("FEEDSTOCK_TOKEN", opts.feedstock_token.clone())
            .send()
            .await
            .into_diagnostic()?;

        let status = resp.status();

        let body: serde_json::Value = resp.json().await.into_diagnostic()?;

        debug!(
            "Copying to conda-forge/{} returned status code {} with body: {}",
            channel,
            status,
            serde_json::to_string_pretty(&body).into_diagnostic()?
        );
    }

    info!("Done uploading packages to conda-forge");

    Ok(())
}
