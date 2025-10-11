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

    /// Validate a file against this checksum
    pub fn validate(&self, path: &std::path::Path) -> bool {
        use md5::{Digest, Md5};
        use sha2::Sha256;
        use std::io::Read;

        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return false,
        };

        let mut buffer = Vec::new();
        if file.read_to_end(&mut buffer).is_err() {
            return false;
        }

        match self {
            Checksum::Sha256(expected) => {
                let mut hasher = Sha256::new();
                hasher.update(&buffer);
                let result = hasher.finalize();
                result.as_slice() == expected.as_slice()
            }
            Checksum::Md5(expected) => {
                let mut hasher = Md5::new();
                hasher.update(&buffer);
                let result = hasher.finalize();
                result.as_slice() == expected.as_slice()
            }
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

/// Git source specification that wraps rattler_git functionality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSource {
    pub url: url::Url,
    pub reference: RattlerGitReference,
    pub depth: Option<i32>,
    pub lfs: bool,
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
        }
    }
}

/// URL source specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlSource {
    pub urls: Vec<url::Url>,
    pub checksum: Option<Checksum>,
    pub file_name: Option<String>,
}

/// Source types that can be cached
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    Git(GitSource),
    Url(UrlSource),
    Path(PathBuf), // Path sources are passed through without caching
}
