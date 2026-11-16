use rattler_build_types::LateBoundPath;
use rattler_digest::{Md5, Md5Hash, Sha256, Sha256Hash, serde::SerializableHash};
use serde::{Deserialize, Serialize};
use serde_with::{
    OneOrMany,
    formats::{PreferMany, PreferOne},
    serde_as,
};
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
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum GitRev {
    /// A git branch
    Branch(String),
    /// A git tag
    Tag(String),
    /// A specific git commit hash
    Commit(String),
    /// The default revision (HEAD)
    #[default]
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
    pub patches: Vec<LateBoundPath>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,

    /// Whether to request the lfs pull in git source
    #[serde(default, skip_serializing_if = "is_false")]
    pub lfs: bool,

    /// Whether to recursively initialize and update submodules (defaults to true)
    #[serde(default = "default_submodules", skip_serializing_if = "is_true")]
    pub submodules: bool,

    /// Optionally an expected commit hash to verify after checkout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_commit: Option<String>,

    /// Only take certain files from the checked-out tree (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub filter: GlobVec,
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
    /// Optional immutable repository ID. Unlike the name, it does not move to
    /// a new repository when the original is renamed, transferred or deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// Optional immutable ID of the repository owner (user or organization)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    /// Optional deployment environment the signing workflow must have run in
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

impl Publisher {
    /// Convert to identity prefix and issuer for sigstore verification.
    ///
    /// The identity is a URL prefix that must match the certificate's SAN.
    /// The `ref_constraint` is checked separately against the signing
    /// workflow ref (or, for GitHub release attestations, the release tag).
    pub fn to_identity_and_issuer(&self) -> (String, String) {
        match self.provider {
            PublisherProvider::GitHub => {
                let identity = format!("https://github.com/{}/{}", self.owner, self.repo);
                (
                    identity,
                    "https://token.actions.githubusercontent.com".to_string(),
                )
            }
            PublisherProvider::GitLab => {
                let identity = format!("https://gitlab.com/{}/{}", self.owner, self.repo);
                (identity, "https://gitlab.com".to_string())
            }
        }
    }
}

impl fmt::Display for Publisher {
    /// Formats the compact form, e.g.
    /// `github:pallets(16748505)/flask(596892)@refs/tags/3.1.1`, which
    /// [`parse_publisher_string`] reads back. The environment has no slot in
    /// the string and is appended as ` (environment: …)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let with_id = |name: &str, id: &Option<String>| match id {
            Some(id) => format!("{name}({id})"),
            None => name.to_string(),
        };
        write!(
            f,
            "{}:{}/{}",
            self.provider,
            with_id(&self.owner, &self.owner_id),
            with_id(&self.repo, &self.repository_id)
        )?;
        if let Some(ref_constraint) = &self.ref_constraint {
            write!(f, "@{}", ref_constraint)?;
        }
        if let Some(environment) = &self.environment {
            write!(f, " (environment: {environment})")?;
        }
        Ok(())
    }
}

/// Check that a repository or owner ID is a plain number.
fn validate_numeric_id(key: &str, id: &str) -> Result<(), String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "Invalid publisher {key} '{id}': expected the numeric ID (e.g. 596892)"
        ));
    }
    Ok(())
}

/// Split `name{id}` into the name and the optional numeric ID.
fn split_pinned_name<'a>(
    part: &'a str,
    key: &str,
    s: &str,
) -> Result<(&'a str, Option<String>), String> {
    let (name, id) = match part.split_once('(') {
        Some((name, rest)) => {
            let id = rest.strip_suffix(')').ok_or_else(|| {
                format!("Invalid publisher format '{s}': expected `name(id)` with the ID last")
            })?;
            validate_numeric_id(key, id)?;
            (name, Some(id.to_string()))
        }
        None => (part, None),
    };
    if name.contains(['(', ')']) {
        return Err(format!(
            "Invalid publisher format '{s}': unexpected parenthesis in '{name}'"
        ));
    }
    Ok((name, id))
}

/// Build a publisher from the mapping form. `owner_repo` is the value of the
/// `github` / `gitlab` key and must not carry a ref; that goes in `ref`.
pub fn publisher_from_parts(
    provider: PublisherProvider,
    owner_repo: &str,
    ref_constraint: Option<String>,
    repository_id: Option<String>,
    owner_id: Option<String>,
    environment: Option<String>,
) -> Result<Publisher, String> {
    if owner_repo.contains('@') {
        return Err(format!(
            "Invalid publisher repository '{owner_repo}': put the ref in the `ref` field instead of after `@`"
        ));
    }
    if owner_repo.contains(['(', ')']) {
        return Err(format!(
            "Invalid publisher repository '{owner_repo}': put the IDs in the `repository_id` and `owner_id` fields instead of in parentheses"
        ));
    }
    let mut publisher = parse_publisher_string(&format!("{provider}:{owner_repo}"))?;
    for (key, id) in [("repository_id", &repository_id), ("owner_id", &owner_id)] {
        if let Some(id) = id {
            validate_numeric_id(key, id)?;
        }
    }
    if environment.as_deref() == Some("") {
        return Err("Invalid publisher environment: must not be empty".to_string());
    }
    publisher.ref_constraint = ref_constraint;
    publisher.repository_id = repository_id;
    publisher.owner_id = owner_id;
    publisher.environment = environment;
    Ok(publisher)
}

/// Parse a publisher string like "github:owner/repo" or "github:owner/repo@refs/tags/v1.0".
///
/// The owner and repository can each carry their immutable numeric ID in
/// parentheses: "github:pallets(16748505)/flask(596892)@refs/tags/3.1.1".
pub fn parse_publisher_string(s: &str) -> Result<Publisher, String> {
    let (provider_str, rest) = s.split_once(':').ok_or_else(|| {
        format!(
            "Invalid publisher format '{}': expected 'provider:owner/repo'",
            s
        )
    })?;

    let provider = match provider_str {
        "github" => PublisherProvider::GitHub,
        "gitlab" => PublisherProvider::GitLab,
        _ => {
            return Err(format!(
                "Unknown publisher provider '{}': expected 'github' or 'gitlab'",
                provider_str
            ));
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

    let (owner, owner_id) = split_pinned_name(owner, "owner_id", s)?;
    let (repo, repository_id) = split_pinned_name(repo, "repository_id", s)?;

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
        repository_id,
        owner_id,
        environment: None,
    })
}

/// Attestation verification configuration (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttestationConfig {
    /// URL to download the attestation bundle from (e.g., .sigstore.json file)
    /// Auto-derived for PyPI sources and uploaded GitHub release assets if not specified.
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

fn default_submodules() -> bool {
    true
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
    pub patches: Vec<LateBoundPath>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,

    /// Attestations the downloaded file must have. Every entry must verify.
    #[serde_as(as = "OneOrMany<_, PreferMany>")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attestations: Vec<AttestationConfig>,

    /// Only take certain files from the extracted source (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub filter: GlobVec,
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
    pub patches: Vec<LateBoundPath>,

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
    pub fn patches(&self) -> &[LateBoundPath] {
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
    fn test_url_source_attestations_round_trip() {
        // A single mapping is read as a one-entry list, and written as a list
        let single: UrlSource = serde_yaml::from_str(
            r#"
url: https://example.com/a.tar.gz
attestations:
  publishers:
    - provider: github
      owner: pallets
      repo: flask
"#,
        )
        .unwrap();
        assert_eq!(single.attestations.len(), 1);
        assert_eq!(single.attestations[0].publishers[0].repo, "flask");

        let written = serde_yaml::to_string(&single).unwrap();
        assert!(
            written.contains("attestations:\n- publishers:"),
            "{written}"
        );
        let read_back: UrlSource = serde_yaml::from_str(&written).unwrap();
        assert_eq!(read_back, single);
    }

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
    fn test_publisher_from_parts_pins_ids() {
        let p = publisher_from_parts(
            PublisherProvider::GitHub,
            "pallets/flask",
            Some("refs/tags/3.1.1".to_string()),
            Some("596892".to_string()),
            Some("16748505".to_string()),
            Some("publish".to_string()),
        )
        .unwrap();
        assert_eq!(p.owner, "pallets");
        assert_eq!(p.repo, "flask");
        assert_eq!(p.repository_id.as_deref(), Some("596892"));
        assert_eq!(
            p.to_string(),
            "github:pallets(16748505)/flask(596892)@refs/tags/3.1.1 (environment: publish)"
        );
    }

    #[test]
    fn test_publisher_from_parts_rejects_bad_values() {
        let parts = |repo: &str, id: &str| {
            publisher_from_parts(
                PublisherProvider::GitHub,
                repo,
                None,
                Some(id.to_string()),
                None,
                None,
            )
        };
        assert!(
            parts("pallets/flask@refs/tags/1", "1")
                .unwrap_err()
                .contains("`ref`")
        );
        assert!(
            parts("pallets/flask", "pallets")
                .unwrap_err()
                .contains("numeric")
        );
        assert!(parts("pallets/flask", "").unwrap_err().contains("numeric"));
        assert!(parts("flask", "1").is_err());
        assert!(
            parts("pallets(1)/flask", "2")
                .unwrap_err()
                .contains("`repository_id`")
        );
    }

    #[test]
    fn test_parse_publisher_with_ids() {
        let p = parse_publisher_string("github:pallets(16748505)/flask(596892)@refs/tags/3.1.1")
            .unwrap();
        assert_eq!(p.owner, "pallets");
        assert_eq!(p.owner_id.as_deref(), Some("16748505"));
        assert_eq!(p.repo, "flask");
        assert_eq!(p.repository_id.as_deref(), Some("596892"));
        assert_eq!(p.ref_constraint.as_deref(), Some("refs/tags/3.1.1"));

        // Each ID is optional on its own.
        let p = parse_publisher_string("github:pallets/flask(596892)").unwrap();
        assert_eq!(
            (p.owner_id, p.repository_id.as_deref()),
            (None, Some("596892"))
        );
        let p = parse_publisher_string("github:pallets(16748505)/flask").unwrap();
        assert_eq!(
            (p.owner_id.as_deref(), p.repository_id),
            (Some("16748505"), None)
        );
    }

    #[test]
    fn test_parse_publisher_rejects_bad_ids() {
        for bad in [
            "github:pallets/flask()",
            "github:pallets/flask(abc)",
            "github:pallets/flask(596892",
            "github:pallets/flask596892)",
            "github:pallets/flask(1)x",
            "github:pallets/fl(1)ask(2)",
            "github:(1)/flask",
            "github:pallets/(1)",
        ] {
            assert!(
                parse_publisher_string(bad).is_err(),
                "{bad} should not parse"
            );
        }
    }

    #[test]
    fn test_publisher_display_round_trips() {
        for s in [
            "github:pallets/flask",
            "github:pallets/flask@refs/tags/3.1.1",
            "github:pallets(16748505)/flask(596892)@refs/tags/3.1.1",
            "github:pallets/flask(596892)",
            "gitlab:org(7)/project(8)@refs/heads/main",
        ] {
            assert_eq!(parse_publisher_string(s).unwrap().to_string(), s);
        }
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
