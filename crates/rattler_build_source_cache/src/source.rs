//! Source definitions for the cache

use rattler_git::{GitUrl as RattlerGitUrl, git::GitReference as RattlerGitReference};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Checksum types supported by the cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Checksum {
    Sha256(Vec<u8>),
    Md5(Vec<u8>),
}

impl Checksum {
    /// Convert checksum to hex string
    pub fn to_hex(&self) -> String {
        match self {
            Checksum::Sha256(bytes) => hex::encode(bytes),
            Checksum::Md5(bytes) => hex::encode(bytes),
        }
    }

    /// Validate a file against this checksum.
    /// Returns `Ok(())` if the checksum matches, or `Err(ChecksumMismatch)` with details.
    pub fn validate(&self, path: &std::path::Path) -> Result<(), ChecksumMismatch> {
        use md5::{Digest, Md5};
        use sha2::Sha256;
        use std::io::Read;

        let mut file = std::fs::File::open(path).map_err(|e| ChecksumMismatch {
            expected: self.to_hex(),
            actual: format!("<failed to open file: {e}>"),
            kind: self.kind(),
        })?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| ChecksumMismatch {
                expected: self.to_hex(),
                actual: format!("<failed to read file: {e}>"),
                kind: self.kind(),
            })?;

        let actual_hex = match self {
            Checksum::Sha256(_) => {
                let mut hasher = Sha256::new();
                hasher.update(&buffer);
                hex::encode(hasher.finalize())
            }
            Checksum::Md5(_) => {
                let mut hasher = Md5::new();
                hasher.update(&buffer);
                hex::encode(hasher.finalize())
            }
        };

        if actual_hex == self.to_hex() {
            Ok(())
        } else {
            Err(ChecksumMismatch {
                expected: self.to_hex(),
                actual: actual_hex,
                kind: self.kind(),
            })
        }
    }

    /// Returns the kind of this checksum.
    pub fn kind(&self) -> ChecksumKind {
        match self {
            Checksum::Sha256(_) => ChecksumKind::Sha256,
            Checksum::Md5(_) => ChecksumKind::Md5,
        }
    }

    /// Create from hex string
    pub fn from_hex_str(value: &str, kind: ChecksumKind) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(value)?;
        Ok(match kind {
            ChecksumKind::Sha256 => Checksum::Sha256(bytes),
            ChecksumKind::Md5 => Checksum::Md5(bytes),
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChecksumKind {
    Sha256,
    Md5,
}

impl std::fmt::Display for ChecksumKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChecksumKind::Sha256 => write!(f, "sha256"),
            ChecksumKind::Md5 => write!(f, "md5"),
        }
    }
}

/// Details about a checksum mismatch
#[derive(Debug, Clone)]
pub struct ChecksumMismatch {
    pub expected: String,
    pub actual: String,
    pub kind: ChecksumKind,
}

/// Git source specification that wraps rattler_git functionality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSource {
    pub url: url::Url,
    pub reference: RattlerGitReference,
    pub depth: Option<i32>,
    pub lfs: bool,
    /// Optionally an expected commit hash to verify after checkout
    pub expected_commit: Option<String>,
}

impl GitSource {
    /// Convert to rattler_git::GitUrl
    pub fn to_git_url(&self) -> RattlerGitUrl {
        RattlerGitUrl::from_reference(self.url.clone(), self.reference.clone())
    }

    /// Create a new GitSource
    pub fn new(
        url: url::Url,
        reference: RattlerGitReference,
        depth: Option<i32>,
        lfs: bool,
    ) -> Self {
        Self {
            url,
            reference,
            depth,
            lfs,
            expected_commit: None,
        }
    }

    /// Create a new GitSource with expected commit
    pub fn with_expected_commit(
        url: url::Url,
        reference: RattlerGitReference,
        depth: Option<i32>,
        lfs: bool,
        expected_commit: Option<String>,
    ) -> Self {
        Self {
            url,
            reference,
            depth,
            lfs,
            expected_commit,
        }
    }
}

/// URL source specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlSource {
    pub urls: Vec<url::Url>,
    pub checksums: Vec<Checksum>,
    pub file_name: Option<String>,
}

/// Source types that can be cached
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    Git(GitSource),
    Url(UrlSource),
    Path(PathBuf), // Path sources are passed through without caching
}
