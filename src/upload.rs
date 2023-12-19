use std::{fmt::Write, path::PathBuf};

use futures::TryStreamExt;
use indicatif::{style::TemplateError, HumanBytes, ProgressState};
use tokio_util::io::ReaderStream;

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::package::{IndexJson, PackageFile};
use rattler_digest::compute_file_digest;
use rattler_networking::{redact_known_secrets_from_error, Authentication, AuthenticationStorage};
use reqwest::Method;
use sha2::{Digest, Sha256};
use tracing::info;
use url::Url;

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

pub async fn upload_package_to_artifactory(
    storage: &AuthenticationStorage,
    username: Option<String>,
    password: Option<String>,
    package_file: PathBuf,
    url: Url,
    channel: String,
) -> miette::Result<()> {
    let package_dir = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Creating temporary directory failed")?;

    rattler_package_streaming::fs::extract(&package_file, package_dir.path()).into_diagnostic()?;

    let index_json = IndexJson::from_package_directory(package_dir.path()).into_diagnostic()?;
    let subdir = index_json
        .subdir
        .ok_or_else(|| miette::miette!("index.json of the package has no subdirectory. Cannot determine which directory to upload to"))?;

    let (username, password) = match (username, password) {
        (Some(u), Some(p)) => (u, p),
        (Some(_), _) | (_, Some(_)) => {
            return Err(miette::miette!("A username and password is required for authentication with artifactory, only one was given"));
        }
        _ => match storage.get_by_url(url.clone()) {
            Ok((_, Some(Authentication::BasicHTTP { username, password }))) => (username, password),
            Ok((_, Some(_))) => {
                return Err(miette::miette!("A username and password is required for authentication with artifactory.
                            Authentication information found in the keychain / auth file, but it was not a username and password"));
            }
            Ok((_, None)) => {
                return Err(miette::miette!(
                        "No username and password was given and none was found in the keychain / auth file"
                    ));
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

    let package_name = package_file
        .file_name()
        .expect("no filename found")
        .to_string_lossy();

    let upload_url = url
        .join(&format!("{}/{}/{}", channel, subdir, package_name))
        .into_diagnostic()?;

    let bytes = tokio::fs::read(package_file).await.into_diagnostic()?;

    client
        .request(Method::PUT, upload_url)
        .body(bytes)
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(redact_known_secrets_from_error)
        .into_diagnostic()
        .wrap_err("Sending package to artifactory server failed")?
        .error_for_status_ref()
        .map_err(redact_known_secrets_from_error)
        .into_diagnostic()
        .wrap_err("Artifactory responded with error")?;

    info!("Package was successfully uploaded to artifactory server");

    Ok(())
}

pub async fn upload_package_to_prefix(
    storage: &AuthenticationStorage,
    api_key: Option<String>,
    package_file: PathBuf,
    url: Url,
    channel: String,
) -> miette::Result<()> {
    let token = match api_key {
        Some(api_key) => api_key,
        None => match storage.get_by_url(url.clone()) {
            Ok((_, Some(Authentication::BearerToken(token)))) => token,
            Ok((_, Some(_))) => {
                return Err(miette::miette!("A Conda token is required for authentication with prefix.dev.
                        Authentication information found in the keychain / auth file, but it was not a Bearer token"));
            }
            Ok((_, None)) => {
                return Err(miette::miette!(
                    "No prefix.dev api key was given and none was found in the keychain / auth file"
                ));
            }
            Err(e) => {
                return Err(miette::miette!(
                    "Failed to get authentication information form keychain: {e}"
                ));
            }
        },
    };

    let filename = package_file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let file_size = package_file.metadata().into_diagnostic()?.len();

    println!("Uploading package to: {}", url);
    println!(
        "Package file:         {} ({})\n",
        package_file.display(),
        HumanBytes(file_size)
    );

    let url = url
        .join(&format!("api/v1/upload/{}", channel))
        .into_diagnostic()?;

    let client = reqwest::Client::builder()
        .no_gzip()
        .build()
        .expect("failed to create client");

    let sha256sum = format!(
        "{:x}",
        compute_file_digest::<Sha256>(&package_file).into_diagnostic()?
    );

    let file = tokio::fs::File::open(&package_file)
        .await
        .into_diagnostic()?;

    let progress_bar = indicatif::ProgressBar::new(file_size)
        .with_prefix("Uploading")
        .with_style(default_bytes_style().into_diagnostic()?);

    let reader_stream = ReaderStream::new(file)
        .inspect_ok(move |bytes| {
            progress_bar.inc(bytes.len() as u64);
        })
        .inspect_err(|e| {
            println!("Error while uploading: {}", e);
        });

    let body = reqwest::Body::wrap_stream(reader_stream);

    let response = client
        .post(url.clone())
        .header("X-File-Sha256", sha256sum)
        .header("X-File-Name", filename)
        .header("Content-Length", file_size)
        .header("Content-Type", "application/octet-stream")
        .bearer_auth(token)
        .body(body)
        .send()
        .await
        .into_diagnostic()?;

    if response.status().is_success() {
        println!("Upload successful!");
    } else {
        println!("Upload failed!");
        if response.status() == 401 {
            println!(
                "Authentication failed! Did you set the correct API key for {}",
                url
            );
        }

        std::process::exit(1);
    }

    Ok(())
}
