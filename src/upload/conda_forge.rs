use std::path::PathBuf;

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

    let channels = vec![opts.label];
    let mut checksums = Vec::new();

    for package_file in package_files {
        let package = package::ExtractedPackage::from_package_file(package_file)?;

        loop {
            anaconda
                .create_or_update_package(&opts.staging_channel, &package)
                .await?;

            anaconda
                .create_or_update_release(&opts.staging_channel, &package)
                .await?;

            let successful = anaconda
                .upload_file(&opts.staging_channel, &channels, false, &package)
                .await?;

            // When running with --force and experiencing a conflict error, we delete the conflicting file.
            // Anaconda automatically deletes releases / packages when the deletion of a file would leave them empty.
            // Therefore, we need to ensure that the release / package still exists before trying to upload again.
            if successful {
                checksums.push(package.sha256().into_diagnostic()?);
                break;
            }
        }
    }

    let payload = serde_json::json!({
        "feedstock": opts.feedstock,
        "outputs": checksums,
        "channel": opts.staging_channel,
        "comment_on_error": opts.post_comment_on_error,
        "hash_type": "sha256",
        "provider": opts.provider
    });

    let client = get_default_client().into_diagnostic()?;

    debug!("Sending payload to validation endpoint: {:?}", payload);

    let resp = client
        .post(opts.validation_endpoint)
        .json(&payload)
        .header("FEEDSTOCK_TOKEN", opts.feedstock_token)
        .send()
        .await
        .into_diagnostic()?;

    debug!("Response from validation endpoint: {:?}", resp);
    debug!("Response body: {:?}", resp.text().await);

    Ok(())
}
