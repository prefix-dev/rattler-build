use rattler_digest::{Md5, Md5Hash, Sha256, Sha256Hash, serde::SerializableHash};
use serde::{Deserialize, Serialize};
use serde_with::{OneOrMany, formats::PreferOne, serde_as};
use std::{fmt, path::PathBuf, str::FromStr};
use url::Url;

use super::GlobVec;

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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Serialize a GitRev to a string
fn serialize_gitrev<S>(rev: &GitRev, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&rev.to_string())
}

/// Deserialize a GitRev from a string
fn deserialize_gitrev<'de, D>(deserializer: D) -> Result<GitRev, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    GitRev::from_str(&s).map_err(serde::de::Error::custom)
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
    #[serde(
        default,
        skip_serializing_if = "GitRev::is_head",
        serialize_with = "serialize_gitrev",
        deserialize_with = "deserialize_gitrev"
    )]
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

    /// Optionally an expected commit hash to verify after checkout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_commit: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Provider for a publisher identity
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublisherProvider {
    GitHub,
    GitLab,
}

impl fmt::Display for PublisherProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitHub => write!(f, "github"),
            Self::GitLab => write!(f, "gitlab"),
        }
    }
}

/// A parsed publisher identity (e.g., "github:owner/repo@refs/tags/v1.0")
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Publisher {
    /// The provider (github, gitlab)
    pub provider: PublisherProvider,
    /// Repository owner
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// Optional ref constraint (e.g., "refs/tags/v1.0")
    pub ref_constraint: Option<String>,
}

impl Publisher {
    /// Convert to identity prefix and issuer for sigstore verification.
    ///
    /// The identity is a URL prefix that must match the certificate's SAN.
    /// The ref_constraint is not used here — it can be checked separately if needed.
    pub fn to_identity_and_issuer(&self) -> (String, String) {
        match self.provider {
            PublisherProvider::GitHub => {
                let identity =
                    format!("https://github.com/{}/{}", self.owner, self.repo);
                (
                    identity,
                    "https://token.actions.githubusercontent.com".to_string(),
                )
            }
            PublisherProvider::GitLab => {
                let identity =
                    format!("https://gitlab.com/{}/{}", self.owner, self.repo);
                (identity, "https://gitlab.com".to_string())
            }
        }
    }
}

impl fmt::Display for Publisher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}/{}", self.provider, self.owner, self.repo)?;
        if let Some(ref_constraint) = &self.ref_constraint {
            write!(f, "@{}", ref_constraint)?;
        }
        Ok(())
    }
}

/// Parse a publisher string like "github:owner/repo" or "github:owner/repo@refs/tags/v1.0"
pub fn parse_publisher_string(s: &str) -> Result<Publisher, String> {
    let (provider_str, rest) = s
        .split_once(':')
        .ok_or_else(|| format!("Invalid publisher format '{}': expected 'provider:owner/repo'", s))?;

    let provider = match provider_str {
        "github" => PublisherProvider::GitHub,
        "gitlab" => PublisherProvider::GitLab,
        _ => {
            return Err(format!(
                "Unknown publisher provider '{}': expected 'github' or 'gitlab'",
                provider_str
            ))
        }
    };

    let (owner_repo, ref_constraint) = if let Some((or, r)) = rest.split_once('@') {
        (or, Some(r.to_string()))
    } else {
        (rest, None)
    };

    let (owner, repo) = owner_repo.split_once('/').ok_or_else(|| {
        format!(
            "Invalid publisher format '{}': expected 'provider:owner/repo'",
            s
        )
    })?;

    if owner.is_empty() || repo.is_empty() {
        return Err(format!(
            "Invalid publisher format '{}': owner and repo must not be empty",
            s
        ));
    }

    Ok(Publisher {
        provider,
        owner: owner.to_string(),
        repo: repo.to_string(),
        ref_constraint,
    })
}

/// Attestation verification configuration (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttestationConfig {
    /// URL to download the attestation bundle from (e.g., .sigstore.json file)
    /// Auto-derived for PyPI sources if not specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_url: Option<Url>,

    /// Publisher identities to verify. All must match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publishers: Vec<Publisher>,
}

impl AttestationConfig {
    /// Check if the attestation config is empty
    pub fn is_empty(&self) -> bool {
        self.bundle_url.is_none() && self.publishers.is_empty()
    }
}

fn attestation_is_none_or_empty(s: &Option<AttestationConfig>) -> bool {
    s.as_ref().map(|c| c.is_empty()).unwrap_or(true)
}

/// A url source (usually a tar.gz or tar.bz2 archive) - evaluated
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    /// Url(s) to the source code
    /// Can be a single URL or a list of URLs (for mirrors)
    #[serde_as(as = "OneOrMany<_, PreferOne>")]
    pub url: Vec<Url>,

    /// Optional SHA256 checksum to verify the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<SerializableHash::<Sha256>>")]
    pub sha256: Option<Sha256Hash>,

    /// Optional MD5 checksum to verify the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<SerializableHash::<Md5>>")]
    pub md5: Option<Md5Hash>,

    /// Optionally a file name to rename the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,

    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<PathBuf>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,

    /// Optional attestation verification configuration
    #[serde(default, skip_serializing_if = "attestation_is_none_or_empty")]
    pub attestation: Option<AttestationConfig>,
}

/// A local path source (evaluated)
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSource {
    /// Path to the local source code
    pub path: PathBuf,

    /// Optional SHA256 checksum to verify the source code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<SerializableHash::<Sha256>>")]
    pub sha256: Option<Sha256Hash>,

    /// Optional MD5 checksum to verify the source code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<SerializableHash::<Md5>>")]
    pub md5: Option<Md5Hash>,

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

    #[test]
    fn test_parse_publisher_github() {
        let p = parse_publisher_string("github:pallets/flask").unwrap();
        assert_eq!(p.provider, PublisherProvider::GitHub);
        assert_eq!(p.owner, "pallets");
        assert_eq!(p.repo, "flask");
        assert_eq!(p.ref_constraint, None);
    }

    #[test]
    fn test_parse_publisher_github_with_ref() {
        let p = parse_publisher_string("github:owner/repo@refs/tags/v1.0").unwrap();
        assert_eq!(p.provider, PublisherProvider::GitHub);
        assert_eq!(p.owner, "owner");
        assert_eq!(p.repo, "repo");
        assert_eq!(p.ref_constraint, Some("refs/tags/v1.0".to_string()));
    }

    #[test]
    fn test_parse_publisher_gitlab() {
        let p = parse_publisher_string("gitlab:org/project").unwrap();
        assert_eq!(p.provider, PublisherProvider::GitLab);
        assert_eq!(p.owner, "org");
        assert_eq!(p.repo, "project");
    }

    #[test]
    fn test_parse_publisher_invalid_no_colon() {
        assert!(parse_publisher_string("github-pallets/flask").is_err());
    }

    #[test]
    fn test_parse_publisher_invalid_no_slash() {
        assert!(parse_publisher_string("github:palletsflask").is_err());
    }

    #[test]
    fn test_parse_publisher_invalid_provider() {
        assert!(parse_publisher_string("bitbucket:owner/repo").is_err());
    }

    #[test]
    fn test_parse_publisher_empty_owner() {
        assert!(parse_publisher_string("github:/repo").is_err());
    }

    #[test]
    fn test_parse_publisher_empty_repo() {
        assert!(parse_publisher_string("github:owner/").is_err());
    }

    #[test]
    fn test_publisher_display() {
        let p = parse_publisher_string("github:pallets/flask").unwrap();
        assert_eq!(p.to_string(), "github:pallets/flask");

        let p = parse_publisher_string("gitlab:org/repo@refs/tags/v2.0").unwrap();
        assert_eq!(p.to_string(), "gitlab:org/repo@refs/tags/v2.0");
    }

    #[test]
    fn test_publisher_to_identity_github() {
        let p = parse_publisher_string("github:pallets/flask").unwrap();
        let (identity, issuer) = p.to_identity_and_issuer();
        assert_eq!(identity, "https://github.com/pallets/flask");
        assert_eq!(issuer, "https://token.actions.githubusercontent.com");
    }

    #[test]
    fn test_publisher_to_identity_gitlab() {
        let p = parse_publisher_string("gitlab:org/project").unwrap();
        let (identity, issuer) = p.to_identity_and_issuer();
        assert_eq!(identity, "https://gitlab.com/org/project");
        assert_eq!(issuer, "https://gitlab.com");
    }
}
