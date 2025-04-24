//! Module to handle checksums and validate checksums of downloaded files.

use std::path::Path;

use rattler_digest::{Md5, Md5Hash, Sha256Hash, compute_file_digest, serde::SerializableHash};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::recipe::parser::{PathSource, UrlSource};

/// Checksum information.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Checksum {
    /// A SHA256 checksum
    Sha256(#[serde_as(as = "SerializableHash::<rattler_digest::Sha256>")] Sha256Hash),
    /// A MD5 checksum
    Md5(#[serde_as(as = "SerializableHash::<rattler_digest::Md5>")] Md5Hash),
}

impl Checksum {
    /// Create a checksum from a URL source.
    pub fn from_url_source(source: &UrlSource) -> Option<Self> {
        if let Some(sha256) = source.sha256() {
            Some(Checksum::Sha256(*sha256))
        } else {
            source.md5().map(|md5| Checksum::Md5(*md5))
        }
    }

    /// Create a checksum from a path source.
    pub fn from_path_source(source: &PathSource) -> Option<Self> {
        if let Some(sha256) = source.sha256 {
            Some(Checksum::Sha256(sha256))
        } else {
            source.md5.map(Checksum::Md5)
        }
    }

    /// Get the checksum as a hex string.
    pub fn to_hex(&self) -> String {
        match self {
            Checksum::Sha256(sha256) => hex::encode(sha256),
            Checksum::Md5(md5) => hex::encode(md5),
        }
    }

    /// Validate the checksum of a file.
    pub fn validate(&self, path: &Path) -> bool {
        match self {
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
}
