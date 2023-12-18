use std::path::PathBuf;

use miette::IntoDiagnostic;
use rattler_networking::{redact_known_secrets_from_error, AuthenticatedClient};
use reqwest::Method;
use sha2::Digest;
use url::Url;

pub async fn upload_package_to_quetz(
    client: &AuthenticatedClient,
    package_file: PathBuf,
    url: Url,
    channel: String,
) -> miette::Result<()> {
    let upload_url = url
        .join(&format!(
            "api/channels/{}/upload/{}",
            channel,
            package_file.file_name().unwrap().to_string_lossy()
        ))
        .into_diagnostic()?;

    let bytes = tokio::fs::read(package_file).await.into_diagnostic()?;
    let upload_hash = sha2::Sha256::digest(&bytes);

    let req = client
        .request(Method::POST, upload_url)
        .query(&[("force", "false"), ("sha256", &hex::encode(upload_hash))])
        .body(bytes)
        .send()
        .await
        .map_err(redact_known_secrets_from_error)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Sending package to Quetz server failed: {e}"))?;

    req.error_for_status_ref()
        .map_err(redact_known_secrets_from_error)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Quetz server responded with error: {e}"))?;

    Ok(())
}
