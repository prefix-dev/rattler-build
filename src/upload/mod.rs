use futures::TryStreamExt;
use indicatif::{style::TemplateError, HumanBytes, ProgressState};
use std::{
    fmt::Write,
    path::{Path, PathBuf},
};
use tokio_util::io::ReaderStream;

use miette::{Context, IntoDiagnostic};
use rattler_networking::{Authentication, AuthenticationStorage, Redact};
use reqwest::Method;
use tracing::info;
use url::Url;

use crate::upload::package::{sha256_sum, ExtractedPackage};

mod anaconda;
pub mod conda_forge;
mod package;

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
        .user_agent(format!("rattler-build/{}", VERSION))
        .build()
}

pub async fn upload_package_to_quetz(
    storage: &AuthenticationStorage,
    api_key: Option<String>,
    package_files: &Vec<PathBuf>,
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

    let client = get_default_client().into_diagnostic()?;

    for package_file in package_files {
        let upload_url = url
            .join(&format!(
                "api/channels/{}/upload/{}",
                channel,
                package_file.file_name().unwrap().to_string_lossy()
            ))
            .into_diagnostic()?;

        let hash = sha256_sum(package_file).into_diagnostic()?;

        let prepared_request = client
            .request(Method::POST, upload_url)
            .query(&[("force", "false"), ("sha256", &hash)])
            .header("X-API-Key", token.clone());

        send_request(prepared_request, package_file).await?;
    }

    info!("Packages successfully uploaded to Quetz server");

    Ok(())
}

pub async fn upload_package_to_artifactory(
    storage: &AuthenticationStorage,
    username: Option<String>,
    password: Option<String>,
    package_files: &Vec<PathBuf>,
    url: Url,
    channel: String,
) -> miette::Result<()> {
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

        let upload_url = url
            .join(&format!("{}/{}/{}", channel, subdir, package_name))
            .into_diagnostic()?;

        let prepared_request = client
            .request(Method::PUT, upload_url)
            .basic_auth(username.clone(), Some(password.clone()));

        send_request(prepared_request, package_file).await?;
    }

    info!("Packages successfully uploaded to Artifactory server");

    Ok(())
}

pub async fn upload_package_to_prefix(
    storage: &AuthenticationStorage,
    api_key: Option<String>,
    package_files: &Vec<PathBuf>,
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

    for package_file in package_files {
        let filename = package_file
            .file_name()
            .expect("no filename found")
            .to_string_lossy()
            .to_string();

        let file_size = package_file.metadata().into_diagnostic()?.len();

        let url = url
            .join(&format!("api/v1/upload/{}", channel))
            .into_diagnostic()?;

        let client = get_default_client().into_diagnostic()?;

        let hash = sha256_sum(package_file).into_diagnostic()?;

        let prepared_request = client
            .post(url.clone())
            .header("X-File-Sha256", hash)
            .header("X-File-Name", filename)
            .header("Content-Length", file_size)
            .header("Content-Type", "application/octet-stream")
            .bearer_auth(token.clone());

        send_request(prepared_request, package_file).await?;
    }

    info!("Packages successfully uploaded to prefix.dev server");

    Ok(())
}

pub async fn upload_package_to_anaconda(
    storage: &AuthenticationStorage,
    token: Option<String>,
    package_files: &Vec<PathBuf>,
    url: Url,
    owner: String,
    channels: Vec<String>,
    force: bool,
) -> miette::Result<()> {
    let token = match token {
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

    let anaconda = anaconda::Anaconda::new(token, url);

    for package_file in package_files {
        loop {
            let package = package::ExtractedPackage::from_package_file(package_file)?;

            anaconda.create_or_update_package(&owner, &package).await?;

            anaconda.create_or_update_release(&owner, &package).await?;

            let successful = anaconda
                .upload_file(&owner, &channels, force, &package)
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
