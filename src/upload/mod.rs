//! The upload module provides the package upload functionality.

use crate::{
    tool_configuration::APP_USER_AGENT, AnacondaData, ArtifactoryData, PrefixData, QuetzData,
};
use futures::TryStreamExt;
use indicatif::{style::TemplateError, HumanBytes, ProgressState};
use reqwest_retry::{policies::ExponentialBackoff, RetryDecision, RetryPolicy};
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio_util::io::ReaderStream;
use trusted_publishing::{check_trusted_publishing, TrustedPublishResult};

use miette::{Context, IntoDiagnostic};
use rattler_networking::{Authentication, AuthenticationStorage};
use rattler_redaction::Redact;
use reqwest::{Method, StatusCode};
use tracing::{info, warn};
use url::Url;

use crate::upload::package::{sha256_sum, ExtractedPackage};

mod anaconda;
pub mod conda_forge;
mod package;
mod trusted_publishing;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the style to use for a progressbar that is currently in progress.
fn default_bytes_style() -> Result<indicatif::ProgressStyle, TemplateError> {
    Ok(indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} {prefix:20!} [{elapsed_precise}] [{bar:40!.bright.yellow/dim.white}] {bytes:>8} @ {smoothed_bytes_per_sec:8}")?
            .progress_chars("━━╾─")
            .with_key(
                "smoothed_bytes_per_sec",
                |s: &ProgressState, w: &mut dyn Write| match (s.pos(), s.elapsed().as_millis()) {
                    (pos, elapsed_ms) if elapsed_ms > 0 => {
                        // TODO: log with tracing?
                        _ = write!(w, "{}/s", HumanBytes((pos as f64 * 1000_f64 / elapsed_ms as f64) as u64));
                    }
                    _ => {
                        _ = write!(w, "-");
                    },
                },
            ))
}

fn get_default_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_gzip()
        .user_agent(APP_USER_AGENT)
        .build()
}

/// Returns a reqwest client with retry middleware.
fn get_client_with_retry() -> Result<reqwest_middleware::ClientWithMiddleware, reqwest::Error> {
    let client = reqwest::Client::builder()
        .no_gzip()
        .user_agent(APP_USER_AGENT)
        .build()?;

    Ok(reqwest_middleware::ClientBuilder::new(client)
        .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
            reqwest_retry::policies::ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .build())
}

/// Uploads package files to a Quetz server.
pub async fn upload_package_to_quetz(
    storage: &AuthenticationStorage,
    package_files: &Vec<PathBuf>,
    quetz_data: QuetzData,
) -> miette::Result<()> {
    let token = match quetz_data.api_key {
        Some(api_key) => api_key,
        None => match storage.get_by_url(Url::from(quetz_data.url.clone())) {
            Ok((_, Some(Authentication::CondaToken(token)))) => token,
            Ok((_, Some(_))) => {
                return Err(miette::miette!("A Conda token is required for authentication with quetz.
                        Authentication information found in the keychain / auth file, but it was not a Conda token"));
            }
            Ok((_, None)) => {
                return Err(miette::miette!(
                    "No quetz api key was given and none was found in the keychain / auth file"
                ));
            }
            Err(e) => {
                return Err(miette::miette!(
                    "Failed to get authentication information form keychain: {e}"
                ));
            }
        },
    };

    let client = get_default_client().into_diagnostic()?;

    for package_file in package_files {
        let upload_url = quetz_data
            .url
            .join(&format!(
                "api/channels/{}/upload/{}",
                quetz_data.channels,
                package_file.file_name().unwrap().to_string_lossy()
            ))
            .into_diagnostic()?;

        let hash = sha256_sum(package_file).into_diagnostic()?;

        let prepared_request = client
            .request(Method::POST, upload_url)
            .query(&[("force", "false"), ("sha256", &hash)])
            .header("X-API-Key", token.clone());

        send_request_with_retry(prepared_request, package_file).await?;
    }

    info!("Packages successfully uploaded to Quetz server");

    Ok(())
}

/// Uploads package files to an Artifactory server.
pub async fn upload_package_to_artifactory(
    storage: &AuthenticationStorage,
    package_files: &Vec<PathBuf>,
    artifactory_data: ArtifactoryData,
) -> miette::Result<()> {
    let token = match artifactory_data.token {
        Some(t) => t,
        _ => match storage.get_by_url(Url::from(artifactory_data.url.clone())) {
            Ok((_, Some(Authentication::BearerToken(token)))) => token,
            Ok((
                _,
                Some(Authentication::BasicHTTP {
                    username: _,
                    password,
                }),
            )) => {
                warn!("A bearer token is required for authentication with artifactory. Using the password from the keychain / auth file to authenticate. Consider switching to a bearer token instead for Artifactory.");
                password
            }
            Ok((_, Some(_))) => {
                return Err(miette::miette!("A bearer token is required for authentication with artifactory.
                            Authentication information found in the keychain / auth file, but it was not a bearer token"));
            }
            Ok((_, None)) => {
                return Err(miette::miette!(
                    "No bearer token was given and none was found in the keychain / auth file"
                ));
            }
            Err(e) => {
                return Err(miette::miette!(
                    "Failed to get authentication information form keychain: {e}"
                ));
            }
        },
    };

    for package_file in package_files {
        let package = ExtractedPackage::from_package_file(package_file)?;

        let subdir = package.subdir().ok_or_else(|| {
            miette::miette!(
                "index.json of package {} has no subdirectory. Cannot determine which directory to upload to",
                package_file.display()
            )
        })?;

        let package_name = package.filename().ok_or(miette::miette!(
            "Package file {} has no filename",
            package_file.display()
        ))?;

        let client = get_default_client().into_diagnostic()?;

        let upload_url = artifactory_data
            .url
            .join(&format!(
                "{}/{}/{}",
                artifactory_data.channels, subdir, package_name
            ))
            .into_diagnostic()?;

        let prepared_request = client
            .request(Method::PUT, upload_url)
            .bearer_auth(token.clone());

        send_request_with_retry(prepared_request, package_file).await?;
    }

    info!("Packages successfully uploaded to Artifactory server");

    Ok(())
}

/// Uploads package files to a prefix.dev server.
pub async fn upload_package_to_prefix(
    storage: &AuthenticationStorage,
    package_files: &Vec<PathBuf>,
    prefix_data: PrefixData,
) -> miette::Result<()> {
    let check_storage = || {
        match storage.get_by_url(Url::from(prefix_data.url.clone())) {
            Ok((_, Some(Authentication::BearerToken(token)))) => Ok(token),
            Ok((_, Some(_))) => {
                Err(miette::miette!("A Conda token is required for authentication with prefix.dev.
                        Authentication information found in the keychain / auth file, but it was not a Bearer token"))
            }
            Ok((_, None)) => {
                Err(miette::miette!(
                    "No prefix.dev api key was given and none was found in the keychain / auth file"
                ))
            }
            Err(e) => {
                Err(miette::miette!(
                    "Failed to get authentication information from keychain: {e}"
                ))
            }
        }
    };

    let token = match prefix_data.api_key {
        Some(api_key) => api_key,
        None => match check_trusted_publishing(
            &get_client_with_retry().into_diagnostic()?,
            &prefix_data.url,
        )
        .await
        {
            TrustedPublishResult::Configured(token) => token.secret().to_string(),
            TrustedPublishResult::Skipped => check_storage()?,
            TrustedPublishResult::Ignored(err) => {
                tracing::warn!("Checked for trusted publishing but failed with {err}");
                check_storage()?
            }
        },
    };

    for package_file in package_files {
        let filename = package_file
            .file_name()
            .expect("no filename found")
            .to_string_lossy()
            .to_string();

        let file_size = package_file.metadata().into_diagnostic()?.len();

        let url = prefix_data
            .url
            .join(&format!("api/v1/upload/{}", prefix_data.channel))
            .into_diagnostic()?;

        let hash = sha256_sum(package_file).into_diagnostic()?;

        // Note we cannot use the reqwest client with middleware because we stream
        // the file during upload
        let prepared_request = get_default_client()
            .into_diagnostic()?
            .post(url.clone())
            .header("X-File-Sha256", hash)
            .header("X-File-Name", filename)
            .header("Content-Length", file_size)
            .header("Content-Type", "application/octet-stream")
            .bearer_auth(token.clone());

        send_request_with_retry(prepared_request, package_file).await?;
    }

    info!("Packages successfully uploaded to prefix.dev server");

    Ok(())
}

/// Uploads package files to an Anaconda server.
pub async fn upload_package_to_anaconda(
    storage: &AuthenticationStorage,
    package_files: &Vec<PathBuf>,
    anaconda_data: AnacondaData,
) -> miette::Result<()> {
    let token = match anaconda_data.api_key {
        Some(token) => token,
        None => match storage.get("anaconda.org") {
            Ok(Some(Authentication::CondaToken(token))) => token,
            Ok(Some(_)) => {
                return Err(miette::miette!(
                    "A Conda token is required for authentication with anaconda.org.
                        Authentication information found in the keychain / auth file, but it was not a Conda token.
                        Please create a token on anaconda.org"
                ));
            }
            Ok(None) => {
                return Err(miette::miette!(
                    "No anaconda.org api key was given and no token were found in the keychain / auth file. Please create a token on anaconda.org"
                ));
            }
            Err(e) => {
                return Err(miette::miette!(
                    "Failed to get authentication information form keychain: {e}"
                ));
            }
        },
    };

    let anaconda = anaconda::Anaconda::new(token, anaconda_data.url);

    for package_file in package_files {
        loop {
            let package = package::ExtractedPackage::from_package_file(package_file)?;

            anaconda
                .create_or_update_package(&anaconda_data.owner, &package)
                .await?;

            anaconda
                .create_or_update_release(&anaconda_data.owner, &package)
                .await?;

            let successful = anaconda
                .upload_file(
                    &anaconda_data.owner,
                    &anaconda_data.channels,
                    anaconda_data.force,
                    &package,
                )
                .await?;

            // When running with --force and experiencing a conflict error, we delete the conflicting file.
            // Anaconda automatically deletes releases / packages when the deletion of a file would leave them empty.
            // Therefore, we need to ensure that the release / package still exists before trying to upload again.
            if successful {
                break;
            }
        }
    }
    Ok(())
}

async fn send_request_with_retry(
    prepared_request: reqwest::RequestBuilder,
    package_file: &Path,
) -> miette::Result<reqwest::Response> {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    let mut current_try = 0;

    let request_start = SystemTime::now();

    loop {
        let request = prepared_request
            .try_clone()
            .expect("Could not clone request. Does it have a streaming body?");
        let response = send_request(request, package_file).await?;

        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let body = response.text().await.into_diagnostic()?;
        let err = miette::miette!(
            "Failed to upload package file: {}\nStatus: {}\nBody: {}",
            package_file.display(),
            status,
            body
        );

        // Non-retry status codes
        match status {
            // Authentication/Authorization errors
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(miette::miette!("Authentication error: {}", err));
            }
            // Resource conflicts
            StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY => {
                return Err(miette::miette!("Resource conflict: {}", err));
            }
            // Client errors
            StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::PAYLOAD_TOO_LARGE => {
                return Err(miette::miette!("Client error: {}", err));
            }
            _ => {}
        }

        match retry_policy.should_retry(request_start, current_try) {
            RetryDecision::DoNotRetry => {
                return Err(err);
            }
            RetryDecision::Retry { execute_after } => {
                let sleep_for = execute_after
                    .duration_since(SystemTime::now())
                    .unwrap_or(Duration::ZERO);
                warn!(
                    "Failed to upload package file: {}\nStatus: {}\nBody: {}\nRetrying in {} seconds",
                    package_file.display(),
                    status,
                    body,
                    sleep_for.as_secs()
                );
                tokio::time::sleep(sleep_for).await;
            }
        }

        current_try += 1;
    }
}

/// Note that we need to use a regular request. reqwest_retry does not support streaming requests.
async fn send_request(
    prepared_request: reqwest::RequestBuilder,
    package_file: &Path,
) -> miette::Result<reqwest::Response> {
    let file = tokio::fs::File::open(package_file)
        .await
        .into_diagnostic()?;

    let file_size = file.metadata().await.into_diagnostic()?.len();
    info!(
        "Uploading package file: {} ({})\n",
        package_file
            .file_name()
            .expect("no filename found")
            .to_string_lossy(),
        HumanBytes(file_size)
    );
    let progress_bar = indicatif::ProgressBar::new(file_size)
        .with_prefix("Uploading")
        .with_style(default_bytes_style().into_diagnostic()?);

    let progress_bar_clone = progress_bar.clone();
    let reader_stream = ReaderStream::new(file)
        .inspect_ok(move |bytes| {
            progress_bar_clone.inc(bytes.len() as u64);
        })
        .inspect_err(|e| {
            println!("Error while uploading: {}", e);
        });

    let body = reqwest::Body::wrap_stream(reader_stream);

    let response = prepared_request
        .body(body)
        .send()
        .await
        .map_err(|e| e.redact())
        .into_diagnostic()?;

    response
        .error_for_status_ref()
        .map_err(|e| e.redact())
        .into_diagnostic()
        .wrap_err("Server responded with error")?;

    progress_bar.finish();
    info!(
        "\nUpload complete for package file: {}",
        package_file
            .file_name()
            .expect("no filename found")
            .to_string_lossy()
    );

    Ok(response)
}
