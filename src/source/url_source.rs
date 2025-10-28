//! This module contains the implementation of the fetching for a `UrlSource` struct.

use crate::{
    console_utils::LoggingOutputHandler,
    recipe::parser::UrlSource,
    source::extract::{extract_7z, extract_tar, extract_zip, is_archive},
    tool_configuration::{self, APP_USER_AGENT},
};
use chrono;
use fs_err as fs;
use reqwest_middleware::Error as MiddlewareError;
use serde::{Deserialize, Serialize};
use std::hash::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::{
    ffi::OsStr,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};
use tokio::io::AsyncWriteExt;

use super::{SourceError, checksum::Checksum, extract::is_tarball};

/// Metadata about a downloaded file
#[derive(Debug, Serialize, Deserialize)]
struct DownloadMetadata {
    /// The original URL that was downloaded
    url: String,
    /// The actual filename from Content-Disposition header or URL
    actual_filename: Option<String>,
    /// The checksum of the downloaded file
    checksum: String,
    /// The checksum type (sha256, md5, etc.)
    checksum_type: String,
    /// Timestamp of when the file was downloaded
    download_time: chrono::DateTime<chrono::Utc>,
    /// Whether the filename came from Content-Disposition header
    filename_from_header: bool,
}

/// Splits a path into stem and extension, handling special cases like .tar.gz
/// Only splits known archive extensions, otherwise uses .archive as fallback
pub(crate) fn split_path(path: &Path) -> std::io::Result<(String, String)> {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid filename"))?;

    // Check if this is a known archive format
    if is_archive(filename) {
        // Handle compound extensions like .tar.gz, .tar.bz2, etc.
        let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid path stem")
        })?;

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
    } else {
        let clean_filename = filename.replace('.', "_");
        Ok((clean_filename, "".to_string()))
    }
}

/// Generates a cache name using actual filename (from Content-Disposition or URL)
fn cache_name_from_actual_filename(
    url: &url::Url,
    checksum: Option<&Checksum>,
    actual_filename: Option<&String>,
    with_extension: bool,
) -> Option<String> {
    let filename = if let Some(actual) = actual_filename {
        actual.as_str()
    } else {
        url.path_segments()?.filter(|x| !x.is_empty()).next_back()?
    };

    // Generate a simple hash from the URL if no checksum is provided
    let cache_suffix = if let Some(checksum) = checksum {
        checksum.to_hex()[..8].to_string()
    } else {
        // Use a simple hash of the URL for cache differentiation
        let mut hasher = DefaultHasher::new();
        url.to_string().hash(&mut hasher);
        format!("{:x}", hasher.finish())[..8].to_string()
    };

    // Special handling for GitHub tarball URLs when no actual filename is provided
    let (final_stem, final_extension) = split_path(Path::new(filename)).ok()?;

    Some(if with_extension {
        format!("{}_{}{}", final_stem, cache_suffix, final_extension)
    } else {
        format!("{}_{}", final_stem, cache_suffix)
    })
}

/// Extracts filename from Content-Disposition header
fn extract_filename_from_content_disposition(header_value: &str) -> Option<String> {
    // Parse Content-Disposition header like: attachment; filename="file.tar.gz"
    for part in header_value.split(';') {
        let part = part.trim();
        if part.starts_with("filename=") {
            let filename = part.strip_prefix("filename=")?;
            // Remove quotes if present
            let filename = filename.trim_matches('"').trim_matches('\'');
            if filename.is_empty() {
                return None;
            }
            return Some(filename.to_string());
        }
    }
    None
}

/// Gets the actual filename by sending a HEAD request to check Content-Disposition
async fn get_actual_filename(
    url: &url::Url,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<Option<String>, SourceError> {
    let client = tool_configuration.client.for_host(url);

    let response = client
        .head(url.clone())
        .header(reqwest::header::USER_AGENT, APP_USER_AGENT)
        .send()
        .await
        .map_err(|e| {
            tracing::debug!("HEAD request failed for {}: {}", url, e);
            // Don't fail the whole operation if HEAD fails, just return None
            SourceError::UnknownError(format!("HEAD request failed: {}", e))
        })?;

    if let Some(content_disposition) = response.headers().get("content-disposition")
        && let Ok(header_str) = content_disposition.to_str()
        && let Some(filename) = extract_filename_from_content_disposition(header_str)
    {
        tracing::info!("Found filename from Content-Disposition: {}", filename);
        return Ok(Some(filename));
    }

    Ok(None)
}

/// Saves download metadata as JSON file
async fn save_download_metadata(
    cache_file: &Path,
    metadata: &DownloadMetadata,
) -> Result<(), SourceError> {
    let metadata_path = cache_file.with_extension(format!(
        "{}.json",
        cache_file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("metadata")
    ));

    let json_content = serde_json::to_string_pretty(metadata)
        .map_err(|e| SourceError::UnknownError(format!("Failed to serialize metadata: {}", e)))?;

    tokio::fs::write(&metadata_path, json_content)
        .await
        .map_err(SourceError::Io)?;

    tracing::debug!("Saved download metadata to: {}", metadata_path.display());
    Ok(())
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
    actual_file_name: Option<&String>,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<PathBuf, SourceError> {
    let target = extracted_folder(path);

    if target.is_dir() {
        tracing::info!("Using extracted directory from cache: {}", target.display());
        return Ok(target);
    }

    let is_zip = actual_file_name
        .map(|name| name.ends_with(".zip"))
        .unwrap_or_else(|| path.extension() == Some(OsStr::new("zip")));

    let is_7z = actual_file_name
        .map(|name| name.ends_with(".7z"))
        .unwrap_or_else(|| path.extension() == Some(OsStr::new("7z")));

    let is_tarball = actual_file_name
        .map(|name| is_tarball(name))
        .unwrap_or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(is_tarball)
        });
    if is_tarball {
        tracing::info!("Extracting tar file to cache: {}", path.display());
        extract_tar(path, &target, &tool_configuration.fancy_log_handler)?;
        return Ok(target);
    } else if is_zip {
        tracing::info!("Extracting zip file to cache: {}", path.display());
        extract_zip(path, &target, &tool_configuration.fancy_log_handler)?;
        return Ok(target);
    } else if is_7z {
        tracing::info!("Extracting 7z file to cache: {}", path.display());
        extract_7z(path, &target, &tool_configuration.fancy_log_handler)?;
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
    // convert sha256 or md5 to Checksum - now optional
    let checksum = Checksum::from_url_source(source);

    let mut last_error = None;
    for url in source.urls() {
        // First, try to get the actual filename from Content-Disposition
        let actual_filename = if url.scheme() == "file" {
            // For file URLs, use the path directly
            url.path_segments()
                .and_then(|mut segs| segs.next_back())
                .map(str::to_string)
        } else {
            // For remote URLs, try to get the real filename from Content-Disposition
            match get_actual_filename(url, tool_configuration).await {
                Ok(filename) => filename.or_else(|| {
                    // Fallback to URL path if no Content-Disposition
                    url.path_segments()
                        .and_then(|mut segs| segs.next_back())
                        .map(str::to_string)
                }),
                Err(_) => {
                    // If HEAD request fails, fallback to URL path
                    url.path_segments()
                        .and_then(|mut segs| segs.next_back())
                        .map(str::to_string)
                }
            }
        };

        // Generate cache name using actual filename (for proper extension handling)
        let cache_name = PathBuf::from(
            cache_name_from_actual_filename(url, checksum.as_ref(), actual_filename.as_ref(), true)
                .ok_or(SourceError::UnknownErrorStr(
                    "Failed to build cache name from url",
                ))?,
        );

        let cache_name = cache_dir.join(cache_name);

        let filename_from_header = actual_filename.is_some() && url.scheme() != "file";

        if url.scheme() == "file" {
            let local_path = url
                .to_file_path()
                .map_err(|_| SourceError::Io(std::io::Error::other("Invalid local file path")))?;

            if !local_path.is_file() {
                return Err(SourceError::FileNotFound(local_path));
            }

            if let Some(checksum) = &checksum
                && !checksum.validate(&local_path)
            {
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
            let is_valid_cache = metadata.is_ok()
                && metadata?.is_file()
                && checksum.as_ref().is_none_or(|c| c.validate(&cache_name));

            if is_valid_cache {
                tracing::info!("Found valid source cache file.");
            } else {
                match fetch_remote(url, &cache_name, tool_configuration).await {
                    Ok(_) => {
                        tracing::info!("Downloaded file from {}", url);

                        if let Some(checksum) = &checksum
                            && !checksum.validate(&cache_name)
                        {
                            tracing::error!("Checksum validation failed!");
                            fs::remove_file(&cache_name)?;
                            return Err(SourceError::ValidationFailed);
                        }

                        // Save download metadata
                        let metadata = DownloadMetadata {
                            url: url.to_string(),
                            actual_filename: actual_filename.clone(),
                            checksum: checksum
                                .as_ref()
                                .map(|c| c.to_hex())
                                .unwrap_or_else(|| "none".to_string()),
                            checksum_type: checksum
                                .as_ref()
                                .map(|c| match c {
                                    Checksum::Sha256(_) => "sha256".to_string(),
                                    Checksum::Md5(_) => "md5".to_string(),
                                })
                                .unwrap_or_else(|| "none".to_string()),
                            download_time: chrono::Utc::now(),
                            filename_from_header,
                        };

                        if let Err(e) = save_download_metadata(&cache_name, &metadata).await {
                            tracing::warn!("Failed to save download metadata: {}", e);
                            // Don't fail the whole operation if metadata saving fails
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
            // Use actual filename to determine if we should extract
            let should_extract = actual_filename
                .as_ref()
                .map(|name| is_archive(name))
                .unwrap_or_else(|| {
                    // Fallback to checking the cache file extension if no filename available
                    is_archive(&cache_name.file_name().unwrap_or_default().to_string_lossy())
                });

            if should_extract {
                return extract_to_cache(&cache_name, actual_filename.as_ref(), tool_configuration);
            } else {
                return Ok(cache_name);
            }
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
            // Known archive formats - should split normally
            ("example.tar.gz", ("example", ".tar.gz")),
            ("example.tar.bz2", ("example", ".tar.bz2")),
            ("example.zip", ("example", ".zip")),
            ("example.tar", ("example", ".tar")),
            (".hidden.tar.gz", ("_hidden", ".tar.gz")),
            // Version-like names - should use .archive extension
            ("2.1.2", ("2_1_2", "")),
            ("example.1", ("example_1", "")),
            ("example", ("example", "")),
            ("1.0.0-beta.1", ("1_0_0-beta_1", "")),
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
    fn test_content_disposition_parsing() {
        let test_cases = vec![
            ("attachment; filename=\"file.tar.gz\"", Some("file.tar.gz")),
            ("attachment; filename=file.tar.gz", Some("file.tar.gz")),
            ("attachment; filename='file.tar.gz'", Some("file.tar.gz")),
            ("inline; filename=\"data.zip\"", Some("data.zip")),
            ("attachment", None),
            ("attachment; filename=", None),
        ];

        for (header, expected) in test_cases {
            let result = extract_filename_from_content_disposition(header);
            assert_eq!(result.as_deref(), expected, "Failed for header: {}", header);
        }
    }

    #[test]
    fn test_is_archive() {
        let test_cases = vec![
            ("file.7z", true),
            ("file.tar.gz", true),
            ("file.tar.bz2", true),
            ("file.tar.xz", true),
            ("file.zip", true),
            ("file.tar", true),
            ("file.txt", false),
            ("file.exe", false),
            ("file", false),
        ];

        for (filename, expected) in test_cases {
            assert_eq!(
                is_archive(filename),
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
            let name = cache_name_from_actual_filename(&url, Some(&checksum), None, true).unwrap();
            assert_eq!(name, expected);
        }
    }

    #[test]
    fn test_cache_name_with_actual_filename() {
        let cases = vec![
            // With actual filename from Content-Disposition
            (
                "https://api.github.com/repos/FreeTAKTeam/FreeTakServer/tarball/v2.2.1",
                Some("FreeTakServer-2.2.1.tar.gz".to_string()),
                Checksum::Sha256(
                    rattler_digest::parse_digest_from_hex::<Sha256>(
                        "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    )
                    .unwrap(),
                ),
                "FreeTakServer-2_2_1_12345678.tar.gz",
            ),
            // Regular URL with extension
            (
                "https://example.com/file.zip",
                None,
                Checksum::Sha256(
                    rattler_digest::parse_digest_from_hex::<Sha256>(
                        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                    )
                    .unwrap(),
                ),
                "file_abcdef12.zip",
            ),
        ];

        for (url, actual_filename, checksum, expected) in cases {
            let url = Url::parse(url).unwrap();
            let name = cache_name_from_actual_filename(
                &url,
                Some(&checksum),
                actual_filename.as_ref(),
                true,
            )
            .unwrap();
            assert_eq!(name, expected);
        }
    }

    #[test]
    fn test_cache_name_without_checksum() {
        let url =
            Url::parse("https://api.github.com/repos/FreeTAKTeam/FreeTakServer/tarball/v2.2.1")
                .unwrap();

        // Test without checksum - should generate URL-based hash
        let name_no_checksum = cache_name_from_actual_filename(&url, None, None, true).unwrap();
        assert_eq!(name_no_checksum, "v2_2_1_c5054c75");

        // Test with actual filename
        let name_with_filename = cache_name_from_actual_filename(
            &url,
            None,
            Some(&"FreeTakServer-2.2.1.tar.gz".to_string()),
            true,
        )
        .unwrap();

        assert!(name_with_filename.starts_with("FreeTakServer-2_2_1_"));
        assert!(name_with_filename.ends_with(".tar.gz"));
    }
}
