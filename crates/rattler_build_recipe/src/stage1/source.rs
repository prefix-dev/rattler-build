use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, str::FromStr};
use url::Url;

use super::glob_vec::GlobVec;

/// Source information - can be Git, Url, or Path (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    /// Git source pointing to a Git repository
    Git(GitSource),
    /// Url source pointing to a tarball or similar
    Url(UrlSource),
    /// Path source pointing to a local file or directory
    Path(PathSource),
}

/// A git revision (branch, tag or commit) - evaluated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitRev {
    /// A git branch
    Branch(String),
    /// A git tag
    Tag(String),
    /// A specific git commit hash
    Commit(String),
    /// The default revision (HEAD)
    Head,
}

impl GitRev {
    /// Returns true if the revision is HEAD
    pub fn is_head(&self) -> bool {
        matches!(self, Self::Head)
    }
}

impl fmt::Display for GitRev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Branch(branch) => write!(f, "refs/heads/{}", branch),
            Self::Tag(tag) => write!(f, "refs/tags/{}", tag),
            Self::Head => write!(f, "HEAD"),
            Self::Commit(commit) => write!(f, "{}", commit),
        }
    }
}

impl FromStr for GitRev {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        if s.to_uppercase() == "HEAD" {
            Ok(Self::Head)
        } else if let Some(tag) = s.strip_prefix("refs/tags/") {
            Ok(Self::Tag(tag.to_owned()))
        } else if let Some(branch) = s.strip_prefix("refs/heads/") {
            Ok(Self::Branch(branch.to_owned()))
        } else {
            Ok(Self::Commit(s.to_owned()))
        }
    }
}

impl Default for GitRev {
    fn default() -> Self {
        Self::Head
    }
}

/// A Git repository URL or a local path to a Git repository
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GitUrl {
    /// A remote Git repository URL
    Url(Url),
    /// A remote Git repository URL in scp style (e.g., git@github.com:user/repo.git)
    Ssh(String),
    /// A local path to a Git repository
    Path(PathBuf),
}

impl fmt::Display for GitUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitUrl::Url(url) => write!(f, "{url}"),
            GitUrl::Ssh(url) => write!(f, "{url}"),
            GitUrl::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

/// Git source information (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitSource {
    /// Url to the git repository
    #[serde(rename = "git")]
    pub url: GitUrl,

    /// Revision to checkout (defaults to HEAD)
    #[serde(default, skip_serializing_if = "GitRev::is_head")]
    pub rev: GitRev,

    /// Optionally a depth to clone the repository
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<i32>,

    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<PathBuf>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,

    /// Whether to request the lfs pull in git source
    #[serde(default, skip_serializing_if = "is_false")]
    pub lfs: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Checksum for verification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Checksum {
    /// SHA256 checksum (hex string)
    Sha256(String),
    /// MD5 checksum (hex string)
    Md5(String),
}

/// A url source (usually a tar.gz or tar.bz2 archive) - evaluated
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    /// Url(s) to the source code
    pub url: Vec<Url>,

    /// Optional checksum to verify the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<Checksum>,

    /// Optionally a file name to rename the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,

    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<PathBuf>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,
}

/// A local path source (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSource {
    /// Path to the local source code
    pub path: PathBuf,

    /// Optional checksum to verify the source code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<Checksum>,

    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<PathBuf>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,

    /// Optionally a file name to rename the file to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<PathBuf>,

    /// Whether to use the `.gitignore` file in the source directory
    #[serde(default = "default_use_gitignore", skip_serializing_if = "is_true")]
    pub use_gitignore: bool,

    /// Only take certain files from the source (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub filter: GlobVec,
}

fn default_use_gitignore() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Source {
    /// Get the patches for this source
    pub fn patches(&self) -> &[PathBuf] {
        match self {
            Self::Git(git) => &git.patches,
            Self::Url(url) => &url.patches,
            Self::Path(path) => &path.patches,
        }
    }

    /// Get the target directory for this source
    pub fn target_directory(&self) -> Option<&PathBuf> {
        match self {
            Self::Git(git) => git.target_directory.as_ref(),
            Self::Url(url) => url.target_directory.as_ref(),
            Self::Path(path) => path.target_directory.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_rev_from_str() {
        assert_eq!(GitRev::from_str("HEAD").unwrap(), GitRev::Head);
        assert_eq!(GitRev::from_str("head").unwrap(), GitRev::Head);
        assert_eq!(
            GitRev::from_str("refs/tags/v1.0.0").unwrap(),
            GitRev::Tag("v1.0.0".to_string())
        );
        assert_eq!(
            GitRev::from_str("refs/heads/main").unwrap(),
            GitRev::Branch("main".to_string())
        );
        assert_eq!(
            GitRev::from_str("abc123").unwrap(),
            GitRev::Commit("abc123".to_string())
        );
    }

    #[test]
    fn test_git_rev_display() {
        assert_eq!(GitRev::Head.to_string(), "HEAD");
        assert_eq!(
            GitRev::Branch("main".to_string()).to_string(),
            "refs/heads/main"
        );
        assert_eq!(
            GitRev::Tag("v1.0.0".to_string()).to_string(),
            "refs/tags/v1.0.0"
        );
        assert_eq!(GitRev::Commit("abc123".to_string()).to_string(), "abc123");
    }

    #[test]
    fn test_git_url_display() {
        let url = GitUrl::Url(Url::parse("https://github.com/user/repo.git").unwrap());
        assert_eq!(url.to_string(), "https://github.com/user/repo.git");

        let ssh = GitUrl::Ssh("git@github.com:user/repo.git".to_string());
        assert_eq!(ssh.to_string(), "git@github.com:user/repo.git");

        let path = GitUrl::Path(PathBuf::from("/path/to/repo"));
        assert!(path.to_string().contains("repo"));
    }
}
