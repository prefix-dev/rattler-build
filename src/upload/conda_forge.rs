use std::{collections::HashMap, path::PathBuf};

use miette::IntoDiagnostic;
use tracing::debug;

use crate::{upload::get_default_client, CondaForgeOpts};

use super::{
    anaconda,
    package::{self},
};

pub async fn upload_package_to_conda_forge(
    opts: CondaForgeOpts,
    package_files: &Vec<PathBuf>,
) -> miette::Result<()> {
    let anaconda = anaconda::Anaconda::new(opts.staging_token, opts.anaconda_url);

    let channels = vec![opts.label.clone()];
    let mut checksums = HashMap::new();

    for package_file in package_files {
        let package = package::ExtractedPackage::from_package_file(package_file)?;

        anaconda
            .create_or_update_package(&opts.staging_channel, &package)
            .await?;

        anaconda
            .create_or_update_release(&opts.staging_channel, &package)
            .await?;

        anaconda
            .upload_file(&opts.staging_channel, &channels, false, &package)
            .await?;

        let dist_name = format!(
            "{}/{}",
            package.subdir().ok_or(miette::miette!("No subdir found"))?,
            package
                .filename()
                .ok_or(miette::miette!("No filename found"))?
        );

        checksums.insert(dist_name, package.sha256().into_diagnostic()?);
    }

    let payload = serde_json::json!({
        "feedstock": opts.feedstock,
        "outputs": checksums,
        "channel": opts.label,
        "comment_on_error": opts.post_comment_on_error,
        "hash_type": "sha256",
        "provider": opts.provider
    });

    let client = get_default_client().into_diagnostic()?;

    debug!(
        "Sending payload to validation endpoint: {}",
        serde_json::to_string_pretty(&payload).into_diagnostic()?
    );

    let resp = client
        .post(opts.validation_endpoint)
        .json(&payload)
        .header("FEEDSTOCK_TOKEN", opts.feedstock_token)
        .send()
        .await
        .into_diagnostic()?;

    let status = resp.status();

    let body: serde_json::Value = resp.json().await.into_diagnostic()?;

    debug!(
        "Copying to conda-forge returned status code {} with body: {}",
        status,
        serde_json::to_string_pretty(&body).into_diagnostic()?
    );

    Ok(())
}
