//! This module contains the implementation of the fetching for a `UrlSource` struct.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    recipe::parser::{Checksum, UrlSource},
    tool_configuration,
};
use rattler_digest::{compute_file_digest, Md5};
use tokio::io::AsyncWriteExt;

use super::SourceError;

fn validate_checksum(path: &Path, checksum: &Checksum) -> bool {
    match checksum {
        Checksum::Sha256(value) => {
            let digest =
                compute_file_digest::<sha2::Sha256>(path).expect("Could not compute SHA256");
            let computed_sha = hex::encode(digest);
            let checksum_sha = hex::encode(value);
            if !computed_sha.eq(&checksum_sha) {
                tracing::error!(
                    "SHA256 values of downloaded file not matching!\nDownloaded = {}, should be {}",
                    computed_sha,
                    checksum_sha
                );
                false
            } else {
                tracing::info!("Validated SHA256 values of the downloaded file!");
                true
            }
        }
        Checksum::Md5(value) => {
            let digest = compute_file_digest::<Md5>(path).expect("Could not compute SHA256");
            let computed_md5 = hex::encode(digest);
            let checksum_md5 = hex::encode(value);
            if !computed_md5.eq(&checksum_md5) {
                tracing::error!(
                    "MD5 values of downloaded file not matching!\nDownloaded = {}, should be {}",
                    computed_md5,
                    checksum_md5
                );
                false
            } else {
                tracing::info!("Validated MD5 values of the downloaded file!");
                true
            }
        }
    }
}

fn split_filename(filename: &str) -> (String, String) {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    let stem_without_tar = stem.trim_end_matches(".tar");

    let extension = Path::new(filename)
        .extension()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    let full_extension = if stem != stem_without_tar {
        format!(".tar.{}", extension)
    } else if !extension.is_empty() {
        format!(".{}", extension)
    } else {
        "".to_string()
    };

    (stem_without_tar.to_string(), full_extension)
}

fn cache_name_from_url(url: &url::Url, checksum: &Checksum) -> Option<String> {
    let filename = url.path_segments()?.last()?;
    let (stem, extension) = split_filename(filename);
    let checksum = checksum.to_hex();
    Some(format!("{}_{}{}", stem, &checksum[0..8], extension))
}

pub(crate) async fn url_src(
    source: &UrlSource,
    cache_dir: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<PathBuf, SourceError> {
    // convert sha256 or md5 to Checksum
    let checksum = Checksum::try_from(source)?;

    if source.url().scheme() == "file" {
        let local_path = source.url().to_file_path().map_err(|_| {
            SourceError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Invalid local file path",
            ))
        })?;

        if !local_path.is_file() {
            return Err(SourceError::FileNotFound(local_path));
        }

        if !validate_checksum(&local_path, &checksum) {
            return Err(SourceError::ValidationFailed);
        }

        tracing::info!("Using local source file.");
        return Ok(local_path);
    }

    let cache_name = PathBuf::from(cache_name_from_url(source.url(), &checksum).ok_or(
        SourceError::UnknownErrorStr("Failed to build cache name from url"),
    )?);
    let cache_name = cache_dir.join(cache_name);

    let metadata = fs::metadata(&cache_name);
    if metadata.is_ok() && metadata?.is_file() && validate_checksum(&cache_name, &checksum) {
        tracing::info!("Found valid source cache file.");
        return Ok(cache_name.clone());
    }

    let client = reqwest::Client::new();
    let download_size = {
        let resp = client.head(source.url().as_str()).send().await?;
        if resp.status().is_success() {
            resp.headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|ct_len| ct_len.to_str().ok())
                .and_then(|ct_len| ct_len.parse().ok())
                .unwrap_or(0)
        } else {
            return Err(SourceError::UrlNotFile(source.url().clone()));
        }
    };

    let progress_bar = tool_configuration.fancy_log_handler.add_progress_bar(
        indicatif::ProgressBar::new(download_size)
            .with_prefix("Downloading")
            .with_style(tool_configuration.fancy_log_handler.default_bytes_style()),
    );
    progress_bar.set_message(
        source
            .url()
            .path_segments()
            .and_then(|segs| segs.last())
            .map(str::to_string)
            .unwrap_or_else(|| "Unknown File".to_string()),
    );
    let mut file = tokio::fs::File::create(&cache_name).await?;

    let request = client.get(source.url().as_str());
    let mut download = request.send().await?;

    while let Some(chunk) = download.chunk().await? {
        progress_bar.inc(chunk.len() as u64);
        file.write_all(&chunk).await?;
    }

    progress_bar.finish();

    file.flush().await?;

    if !validate_checksum(&cache_name, &checksum) {
        tracing::error!("Checksum validation failed!");
        fs::remove_file(&cache_name)?;
        return Err(SourceError::ValidationFailed);
    }

    Ok(cache_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::parser::Checksum;
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
            (".hidden.tar.gz", (".hidden", ".tar.gz")),
        ];

        for (filename, expected) in test_cases {
            let (name, ending) = split_filename(filename);
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
        let cases =
            vec![
            (
                "https://cache-redirector.jetbrains.com/download.jetbrains.com/idea/jdbc-drivers/web/snowflake-3.13.27.zip",
                Checksum::Sha256(rattler_digest::parse_digest_from_hex::<Sha256>(
                    "6a15e95ee7e6c55b862dab9758ea803350aa2e3560d6183027b0c29919fcab18",
                ).unwrap()),
                "snowflake-3.13.27_6a15e95e.zip",
            ),
            (
                "https://example.com/example.tar.gz",
                Checksum::Sha256(rattler_digest::parse_digest_from_hex::<Sha256>(
                    "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                ).unwrap()),
                "example_12345678.tar.gz",
            ),
            (
                "https://github.com/mamba-org/mamba/archive/refs/tags/micromamba-12.23.12.tar.gz",
                Checksum::Sha256(rattler_digest::parse_digest_from_hex::<Sha256>(
                    "63fd8a1dbec811e63d4f9b5e27757af45d08a219d0900c7c7a19e0b177a576b8",
                ).unwrap()),
                "micromamba-12.23.12_63fd8a1d.tar.gz",
            ),
        ];

        for (url, checksum, expected) in cases {
            let url = Url::parse(url).unwrap();
            let name = cache_name_from_url(&url, &checksum).unwrap();
            assert_eq!(name, expected);
        }
    }
}
