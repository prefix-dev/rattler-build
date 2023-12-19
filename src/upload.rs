use std::path::PathBuf;

use miette::IntoDiagnostic;
use rattler_networking::{redact_known_secrets_from_error, Authentication, AuthenticationStorage};
use reqwest::Method;
use sha2::Digest;
use tracing::info;
use url::Url;

pub async fn upload_package_to_quetz(
    storage: &AuthenticationStorage,
    api_key: Option<String>,
    package_file: PathBuf,
    url: Url,
    channel: String,
) -> miette::Result<()> {
    let token = match api_key {
        Some(api_key) => api_key,
        None => match storage.get_by_url(url.clone()) {
            Ok((_, Some(Authentication::CondaToken(token)))) => token,
            Ok((_, Some(_))) => {
                return Err(miette::miette!("A Conda token is required for authentication with quetz.
                        Authentication information found in the keychain / auth file, but it was not a Conda token"));
            }
            Ok((_, None)) => {
                return Err(miette::miette!("No quetz api key was given and none was found in the keychain / auth file"));
            }
            Err(e) => {
                return Err(miette::miette!(
                    "Failed to get authentication information form keychain: {e}"
                ));
            }
        },
    };

    let client = reqwest::Client::builder()
        .no_gzip()
        .build()
        .expect("failed to create client");

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
        .header("X-API-Key", token)
        .send()
        .await
        .map_err(redact_known_secrets_from_error)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Sending package to Quetz server failed: {e}"))?;

    req.error_for_status_ref()
        .map_err(redact_known_secrets_from_error)
        .into_diagnostic()
        .map_err(|e| miette::miette!("Quetz server responded with error: {e}"))?;
    
    info!("Package was successfully uploaded to Quetz server");
    Ok(())
}
