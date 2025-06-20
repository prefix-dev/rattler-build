//! This module contains the implementation of the fetching for a `UrlSource` struct.

use fs_err as fs;
use std::{
    ffi::OsStr,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use crate::{
    console_utils::LoggingOutputHandler,
    recipe::parser::UrlSource,
    source::extract::{extract_tar, extract_zip},
    tool_configuration::{self, APP_USER_AGENT},
};
use reqwest_middleware::Error as MiddlewareError;
use tokio::io::AsyncWriteExt;

use super::{SourceError, checksum::Checksum, extract::is_tarball};

/// Splits a path into stem and extension, handling special cases like .tar.gz
pub(crate) fn split_path(path: &Path) -> std::io::Result<(String, String)> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid path stem"))?;

    let (stem_no_tar, is_tar) = if let Some(s) = stem.strip_suffix(".tar") {
        (s, true)
    } else {
        (stem, false)
    };

    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let full_extension = if is_tar {
        format!(".tar.{}", extension)
    } else if !extension.is_empty() {
        format!(".{}", extension)
    } else {
        String::new()
    };

    Ok((stem_no_tar.replace('.', "_"), full_extension))
}

/// Generates a cache name from URL and checksum
fn cache_name_from_url(
    url: &url::Url,
    checksum: &Checksum,
    with_extension: bool,
) -> Option<String> {
    let filename = url.path_segments()?.filter(|x| !x.is_empty()).next_back()?;

    let (stem, extension) = split_path(Path::new(filename)).ok()?;
    let checksum_hex = checksum.to_hex();

    Some(if with_extension {
        format!("{}_{}{}", stem, &checksum_hex[..8], extension)
    } else {
        format!("{}_{}", stem, &checksum_hex[..8])
    })
}

async fn fetch_remote(
    url: &url::Url,
    target: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<(), SourceError> {
    let client = tool_configuration.client.for_host(url);

    let (mut response, download_size) = {
        let resp = client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, APP_USER_AGENT)
            .send()
            .await
            .map_err(|e| {
                let err_string = match &e {
                    MiddlewareError::Reqwest(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("SSL")
                            || err_str.contains("certificate")
                            || err_str.contains("handshake")
                        {
                            format!("SSL certificate error: {}", err_str)
                        } else {
                            err_str
                        }
                    }
                    MiddlewareError::Middleware(e) => {
                        let mut err_msg = e.to_string();
                        let mut source = e.source();

                        while let Some(err) = source {
                            let source_str = err.to_string();
                            if source_str.contains("SSL")
                                || source_str.contains("certificate")
                                || source_str.contains("handshake")
                                || source_str.contains("CERTIFICATE")
                            {
                                err_msg = format!("SSL certificate error: {}", source_str);
                                break;
                            } else if !source_str.contains("retry") {
                                err_msg = source_str;
                            }
                            source = err.source();
                        }
                        err_msg
                    }
                };

                SourceError::UnknownError(format!("Error downloading {}: {}", url, err_string))
            })?;

        match resp.error_for_status() {
            Ok(resp) => {
                let dl_size = resp
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|ct_len| ct_len.to_str().ok())
                    .and_then(|ct_len| ct_len.parse().ok())
                    .unwrap_or(0);
                (resp, dl_size)
            }
            Err(e) => {
                return Err(SourceError::Url(e));
            }
        }
    };

    let progress_bar = tool_configuration.fancy_log_handler.add_progress_bar(
        indicatif::ProgressBar::new(download_size)
            .with_prefix("Downloading")
            .with_style(tool_configuration.fancy_log_handler.default_bytes_style()),
    );

    progress_bar.set_message(
        url.path_segments()
            .and_then(|mut segs| segs.next_back())
            .map(str::to_string)
            .unwrap_or_else(|| "Unknown File".to_string()),
    );

    let mut file = tokio::fs::File::create(&target).await?;
    while let Some(chunk) = response.chunk().await.map_err(SourceError::Url)? {
        progress_bar.inc(chunk.len() as u64);
        file.write_all(&chunk).await?;
    }

    progress_bar.finish();

    file.flush().await?;
    Ok(())
}

fn extracted_folder(path: &Path) -> PathBuf {
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    // remove everything after first dot
    let filename = filename.split('.').next().unwrap_or_default();
    path.with_file_name(filename)
}

fn extract_to_cache(
    path: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<PathBuf, SourceError> {
    let target = extracted_folder(path);

    if target.is_dir() {
        tracing::info!("Using extracted directory from cache: {}", target.display());
        return Ok(target);
    }

    if is_tarball(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    ) {
        tracing::info!("Extracting tar file to cache: {}", path.display());
        extract_tar(path, &target, &tool_configuration.fancy_log_handler)?;
        return Ok(target);
    } else if path.extension() == Some(OsStr::new("zip")) {
        tracing::info!("Extracting zip file to cache: {}", path.display());
        extract_zip(path, &target, &tool_configuration.fancy_log_handler)?;
        return Ok(target);
    }

    Ok(path.to_path_buf())
}

fn copy_with_progress(
    source: &Path,
    dest: &Path,
    progress_handler: &LoggingOutputHandler,
) -> std::io::Result<u64> {
    let file_size = source.metadata()?.len();
    let progress_bar = progress_handler.add_progress_bar(
        indicatif::ProgressBar::new(file_size)
            .with_prefix("Copying")
            .with_style(progress_handler.default_bytes_style()),
    );

    let mut reader = std::io::BufReader::new(fs::File::open(source)?);
    let mut writer = std::io::BufWriter::new(fs::File::create(dest)?);
    let mut buffer = vec![0; 8192];
    let mut copied = 0u64;

    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n])?;
        copied += n as u64;
        progress_bar.set_position(copied);
    }

    progress_bar.finish();
    writer.flush()?;
    Ok(copied)
}

pub(crate) async fn url_src(
    source: &UrlSource,
    cache_dir: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<PathBuf, SourceError> {
    // convert sha256 or md5 to Checksum
    let checksum = Checksum::from_url_source(source).ok_or_else(|| {
        SourceError::NoChecksum(format!("No checksum found for url(s): {:?}", source.urls()))
    })?;

    let mut last_error = None;
    for url in source.urls() {
        let cache_name = PathBuf::from(cache_name_from_url(url, &checksum, true).ok_or(
            SourceError::UnknownErrorStr("Failed to build cache name from url"),
        )?);

        let cache_name = cache_dir.join(cache_name);

        if url.scheme() == "file" {
            let local_path = url.to_file_path().map_err(|_| {
                SourceError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Invalid local file path",
                ))
            })?;

            if !local_path.is_file() {
                return Err(SourceError::FileNotFound(local_path));
            }

            if !checksum.validate(&local_path) {
                return Err(SourceError::ValidationFailed);
            }

            // copy file to cache
            copy_with_progress(
                &local_path,
                &cache_name,
                &tool_configuration.fancy_log_handler,
            )?;

            tracing::info!("Using local source file.");
        } else {
            let metadata = fs::metadata(&cache_name);
            if metadata.is_ok() && metadata?.is_file() && checksum.validate(&cache_name) {
                tracing::info!("Found valid source cache file.");
            } else {
                match fetch_remote(url, &cache_name, tool_configuration).await {
                    Ok(_) => {
                        tracing::info!("Downloaded file from {}", url);

                        if !checksum.validate(&cache_name) {
                            tracing::error!("Checksum validation failed!");
                            fs::remove_file(&cache_name)?;
                            return Err(SourceError::ValidationFailed);
                        }
                    }
                    Err(e) => {
                        last_error = Some(e);
                        continue;
                    }
                }
            }
        }

        // If the source has a file name, we skip the extraction step
        if source.file_name().is_some() {
            return Ok(cache_name);
        } else {
            return extract_to_cache(&cache_name, tool_configuration);
        }
    }

    if let Some(last_error) = last_error {
        Err(last_error)
    } else {
        Err(SourceError::UnknownError(
            "Could not download any file".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Checksum;
    use sha2::Sha256;
    use url::Url;

    #[test]
    fn test_split_filename() {
        let test_cases = vec![
            ("example.tar.gz", ("example", ".tar.gz")),
            ("example.tar.bz2", ("example", ".tar.bz2")),
            ("example.zip", ("example", ".zip")),
            ("example.tar", ("example", ".tar")),
            ("example", ("example", "")),
            (".hidden.tar.gz", ("_hidden", ".tar.gz")),
        ];

        for (filename, expected) in test_cases {
            let (name, ending) = split_path(Path::new(filename)).unwrap();
            assert_eq!(
                (name.as_str(), ending.as_str()),
                expected,
                "Failed for filename: {}",
                filename
            );
        }
    }

    #[test]
    fn test_cache_name() {
        let cases = vec![
            (
                "https://cache-redirector.jetbrains.com/download.jetbrains.com/idea/jdbc-drivers/web/snowflake-3.13.27.zip",
                Checksum::Sha256(
                    rattler_digest::parse_digest_from_hex::<Sha256>(
                        "6a15e95ee7e6c55b862dab9758ea803350aa2e3560d6183027b0c29919fcab18",
                    )
                    .unwrap(),
                ),
                "snowflake-3_13_27_6a15e95e.zip",
            ),
            (
                "https://example.com/example.tar.gz",
                Checksum::Sha256(
                    rattler_digest::parse_digest_from_hex::<Sha256>(
                        "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    )
                    .unwrap(),
                ),
                "example_12345678.tar.gz",
            ),
            (
                "https://github.com/mamba-org/mamba/archive/refs/tags/micromamba-12.23.12.tar.gz",
                Checksum::Sha256(
                    rattler_digest::parse_digest_from_hex::<Sha256>(
                        "63fd8a1dbec811e63d4f9b5e27757af45d08a219d0900c7c7a19e0b177a576b8",
                    )
                    .unwrap(),
                ),
                "micromamba-12_23_12_63fd8a1d.tar.gz",
            ),
        ];

        for (url, checksum, expected) in cases {
            let url = Url::parse(url).unwrap();
            let name = cache_name_from_url(&url, &checksum, true).unwrap();
            assert_eq!(name, expected);
        }
    }
}
