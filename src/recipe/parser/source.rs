//! Parse the source section of a recipe

use std::{fmt, path::PathBuf, str::FromStr};

use rattler_digest::{serde::SerializableHash, Md5, Md5Hash, Sha256, Sha256Hash};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use url::Url;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
    source::SourceError,
};

use super::FlattenErrors;

/// Source information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    /// Git source pointing to a Git repository to retrieve the source from
    Git(GitSource),
    /// Url source pointing to a tarball or similar to retrieve the source from
    Url(UrlSource),
    /// Path source pointing to a local file or directory to retrieve the source from
    Path(PathSource),
}

impl Source {
    /// Get the patches.
    pub fn patches(&self) -> &[PathBuf] {
        match self {
            Self::Git(git) => git.patches(),
            Self::Url(url) => url.patches(),
            Self::Path(path) => path.patches(),
        }
    }

    /// Get the folder.
    pub fn target_directory(&self) -> Option<&PathBuf> {
        match self {
            Self::Git(git) => git.target_directory(),
            Self::Url(url) => url.target_directory(),
            Self::Path(path) => path.target_directory(),
        }
    }
}

impl TryConvertNode<Vec<Source>> for RenderedNode {
    fn try_convert(&self, _name: &str) -> Result<Vec<Source>, Vec<PartialParsingError>> {
        let mut sources = Vec::new();

        match self {
            RenderedNode::Mapping(map) => {
                // Git source
                if map.contains_key("git") {
                    let git_src = map.try_convert("source")?;
                    sources.push(Source::Git(git_src));
                } else if map.contains_key("url") {
                    let url_src = map.try_convert("source")?;
                    sources.push(Source::Url(url_src));
                } else if map.contains_key("path") {
                    let path_src = map.try_convert("source")?;
                    sources.push(Source::Path(path_src));
                } else {
                    return Err(vec![_partialerror!(
                        *self.span(),
                        ErrorKind::Other,
                        label = "unknown source type"
                    )]);
                }
            }
            RenderedNode::Sequence(seq) => {
                for n in seq.iter() {
                    let srcs: Vec<_> = n.try_convert("source")?;
                    sources.extend(srcs);
                }
            }
            RenderedNode::Null(_) => (),
            RenderedNode::Scalar(s) => {
                return Err(vec![_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "expected mapping or sequence"
                )])
            }
        }

        Ok(sources)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A git revision (branch, tag or commit)
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
    /// Returns true if the revision is HEAD.
    pub fn is_head(&self) -> bool {
        matches!(self, Self::Head)
    }
}

impl ToString for GitRev {
    fn to_string(&self) -> String {
        match self {
            Self::Branch(branch) => format!("refs/heads/{}", branch),
            Self::Tag(tag) => format!("refs/tags/{}", tag),
            Self::Head => "HEAD".into(),
            Self::Commit(commit) => commit.clone(),
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

/// Serialize a GitRev to a string.
fn serialize_gitrev<S>(rev: &GitRev, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&rev.to_string())
}

/// Deserialize a GitRev from a string.
fn deserialize_gitrev<'de, D>(deserializer: D) -> Result<GitRev, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    GitRev::from_str(&s).map_err(serde::de::Error::custom)
}

/// Git source information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitSource {
    /// Url to the git repository
    #[serde(rename = "git")]
    pub url: GitUrl,
    /// Optionally a revision to checkout, defaults to `HEAD`
    #[serde(
        default,
        skip_serializing_if = "GitRev::is_head",
        serialize_with = "serialize_gitrev",
        deserialize_with = "deserialize_gitrev"
    )]
    pub rev: GitRev,
    /// Optionally a depth to clone the repository, defaults to `None`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<i32>,
    /// Optionally patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<PathBuf>,
    /// Optionally request the lfs pull in git source
    #[serde(default, skip_serializing_if = "should_not_serialize_lfs")]
    pub lfs: bool,
}

/// A helper method to skip serializing the lfs flag if it is false.
fn should_not_serialize_lfs(lfs: &bool) -> bool {
    !lfs
}

impl GitSource {
    #[cfg(test)]
    pub fn create(
        url: GitUrl,
        rev: GitRev,
        depth: Option<i32>,
        patches: Vec<PathBuf>,
        target_directory: Option<PathBuf>,
        lfs: bool,
    ) -> Self {
        Self {
            url,
            rev,
            depth,
            patches,
            target_directory,
            lfs,
        }
    }

    /// Get the git url.
    pub const fn url(&self) -> &GitUrl {
        &self.url
    }

    /// Get the git revision.
    pub fn rev(&self) -> &GitRev {
        &self.rev
    }

    /// Get the git depth.
    pub const fn depth(&self) -> Option<i32> {
        self.depth
    }

    /// Get the patches.
    pub fn patches(&self) -> &[PathBuf] {
        self.patches.as_slice()
    }

    /// Get the target_directory.
    pub const fn target_directory(&self) -> Option<&PathBuf> {
        self.target_directory.as_ref()
    }

    /// Get true if source requires lfs.
    pub const fn lfs(&self) -> bool {
        self.lfs
    }
}

impl TryConvertNode<GitSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<GitSource, Vec<PartialParsingError>> {
        let mut url = None;
        let mut rev = None;
        let mut depth = None;
        let mut patches = Vec::new();
        let mut target_directory = None;
        let mut lfs = false;

        self.iter().map(|(k, v)| {
            match k.as_str() {
                "git" => {
                    let url_str: String = v.try_convert("git")?;
                    let url_ = Url::from_str(&url_str);
                    match url_ {
                        Ok(url_) => url = Some(GitUrl::Url(url_)),
                        Err(err) => {
                            tracing::warn!("invalid url for `GitSource` `{url_str}`: {err}");
                            tracing::warn!("attempting to parse as path");
                            let path = PathBuf::from(url_str);
                            url = Some(GitUrl::Path(path));
                        }
                    }
                }
                "rev" | "tag" | "branch" => {
                    if rev.is_some() {
                        return Err(vec![_partialerror!(
                            *k.span(),
                            ErrorKind::Other,
                            help = "git `source` can only have one of `rev`, `tag` or `branch`"
                        )]);
                    }

                    match k.as_str() {
                        "rev" => {
                            let rev_str: String = v.try_convert("rev")?;
                            rev = Some(GitRev::Commit(rev_str));
                        }
                        "tag" => {
                            let tag_str: String = v.try_convert("tag")?;
                            rev = Some(GitRev::Tag(tag_str));
                        }
                        "branch" => {
                            let branch_str: String = v.try_convert("branch")?;
                            rev = Some(GitRev::Branch(branch_str));
                        }
                        _ => unreachable!(),
                    }
                }
                "depth" => {
                    depth = Some(v.try_convert("git_depth")?);
                }
                "patches" => {
                    patches = v.try_convert("patches")?;
                }
                "target_directory" => {
                    target_directory = Some(v.try_convert("target_directory")?);
                }
                "lfs" => {
                    lfs = v.try_convert("lfs")?;
                }
                _ => {
                    return Err(vec![_partialerror!(
                        *k.span(),
                        ErrorKind::InvalidField(k.as_str().to_owned().into()),
                        help = "valid fields for git `source` are `git`, `rev`, `tag`, `branch`, `depth`, `patches`, `lfs` and `target_directory`"
                    )])
                }
            }
            Ok(())
        }).flatten_errors()?;

        let url = url.ok_or_else(|| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("git_url".into()),
                help = "git `source` must have a `url` field"
            )]
        })?;

        // Use HEAD as default rev
        let rev = rev.unwrap_or_default();

        if !rev.is_head() && depth.is_some() {
            return Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::Other,
                help = "git `source` with a `tag`, `branch` or `rev` cannot have a `depth`"
            )]);
        }

        Ok(GitSource {
            url,
            rev,
            depth,
            patches,
            target_directory,
            lfs,
        })
    }
}

/// A Git repository URL or a local path to a Git repository
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GitUrl {
    /// A remote Git repository URL
    Url(Url),
    /// A local path to a Git repository
    Path(PathBuf),
}

impl fmt::Display for GitUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitUrl::Url(url) => write!(f, "{url}"),
            GitUrl::Path(path) => write!(f, "{path:?}"),
        }
    }
}

/// A url source (usually a tar.gz or tar.bz2 archive). A compressed file
/// will be extracted to the `work` (or `work/<folder>` directory).
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    /// Url to the source code (usually a tar.gz or tar.bz2 etc. file)
    url: Url,

    /// Optionally a sha256 checksum to verify the downloaded file
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<SerializableHash::<rattler_digest::Sha256>>")]
    sha256: Option<Sha256Hash>,

    /// Optionally a md5 checksum to verify the downloaded file
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<SerializableHash::<rattler_digest::Md5>>")]
    md5: Option<Md5Hash>,

    /// Optionally a file name to rename the downloaded file (does not apply to archives)
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    #[serde(skip_serializing_if = "Option::is_none")]
    target_directory: Option<PathBuf>,
}

impl UrlSource {
    /// Get the url.
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Get the SHA256 checksum of the URL source.
    pub fn sha256(&self) -> Option<&Sha256Hash> {
        self.sha256.as_ref()
    }

    /// Get the MD5 checksum of the URL source.
    pub fn md5(&self) -> Option<&Md5Hash> {
        self.md5.as_ref()
    }

    /// Get the patches of the URL source.
    pub fn patches(&self) -> &[PathBuf] {
        self.patches.as_slice()
    }

    /// Get the folder of the URL source.
    pub const fn target_directory(&self) -> Option<&PathBuf> {
        self.target_directory.as_ref()
    }

    /// Get the file name of the URL source.
    pub const fn file_name(&self) -> Option<&String> {
        self.file_name.as_ref()
    }
}

impl TryConvertNode<UrlSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<UrlSource, Vec<PartialParsingError>> {
        let mut url = None;
        let mut sha256 = None;
        let mut md5 = None;
        let mut patches = Vec::new();
        let mut target_directory = None;
        let mut file_name = None;

        self.iter().map(|(key, value)| {
            let key_str = key.as_str();
            match key_str {
                "url" => url = value.try_convert(key_str)?,
                "sha256" => {
                    let sha256_str: RenderedScalarNode = value.try_convert(key_str)?;
                    let sha256_out = rattler_digest::parse_digest_from_hex::<Sha256>(sha256_str.as_str()).ok_or_else(|| vec![_partialerror!(*sha256_str.span(), ErrorKind::InvalidSha256)])?;
                    sha256 = Some(sha256_out);
                }
                "md5" => {
                    let md5_str: RenderedScalarNode = value.try_convert(key_str)?;
                    let md5_out = rattler_digest::parse_digest_from_hex::<Md5>(md5_str.as_str()).ok_or_else(|| vec![_partialerror!(*md5_str.span(), ErrorKind::InvalidMd5)])?;
                    md5 = Some(md5_out);
                }
                "file_name" => file_name = value.try_convert(key_str)?,
                "patches" => patches = value.try_convert(key_str)?,
                "target_directory" => target_directory = value.try_convert(key_str)?,
                invalid_key => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_owned().into()),
                        help = "valid fields for URL `source` are `url`, `sha256`, `md5`, `patches`, `file_name` and `target_directory`"
                    )])
                }
            }
            Ok(())
        }).flatten_errors()?;

        let url = url.ok_or_else(|| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("url".into()),
                help = "URL `source` must have a `url` field"
            )]
        })?;

        if md5.is_none() && sha256.is_none() {
            return Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("sha256 or md5".into()),
                help = "URL `source` must have a `sha256` or `md5` checksum field"
            )]);
        }

        Ok(UrlSource {
            url,
            md5,
            sha256,
            file_name,
            patches,
            target_directory,
        })
    }
}

/// Checksum information.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Checksum {
    /// A SHA256 checksum
    Sha256(#[serde_as(as = "SerializableHash::<rattler_digest::Sha256>")] Sha256Hash),
    /// A MD5 checksum
    Md5(#[serde_as(as = "SerializableHash::<rattler_digest::Md5>")] Md5Hash),
}

impl TryFrom<&UrlSource> for Checksum {
    type Error = SourceError;

    fn try_from(source: &UrlSource) -> Result<Self, Self::Error> {
        if let Some(sha256) = source.sha256() {
            Ok(Checksum::Sha256(*sha256))
        } else if let Some(md5) = source.md5() {
            Ok(Checksum::Md5(*md5))
        } else {
            return Err(SourceError::NoChecksum(source.url().clone()));
        }
    }
}

impl Checksum {
    pub fn to_hex(&self) -> String {
        match self {
            Checksum::Sha256(sha256) => hex::encode(sha256),
            Checksum::Md5(md5) => hex::encode(md5),
        }
    }
}

/// A local path source. The source code will be copied to the `work`
/// (or `work/<folder>` directory).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSource {
    /// Path to the local source code
    path: PathBuf,
    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    #[serde(skip_serializing_if = "Option::is_none")]
    target_directory: Option<PathBuf>,
    /// Optionally a file name to rename the file to
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<PathBuf>,
    /// Whether to use the `.gitignore` file in the source directory. Defaults to `true`.
    #[serde(skip_serializing_if = "should_not_serialize_use_gitignore")]
    use_gitignore: bool,
}

/// Helper method to skip serializing the use_gitignore flag if it is true.
fn should_not_serialize_use_gitignore(use_gitignore: &bool) -> bool {
    *use_gitignore
}

impl PathSource {
    /// Get the path.
    pub const fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the patches.
    pub fn patches(&self) -> &[PathBuf] {
        self.patches.as_slice()
    }

    /// Get the target_directory.
    pub const fn target_directory(&self) -> Option<&PathBuf> {
        self.target_directory.as_ref()
    }

    /// Get the file name.
    pub const fn file_name(&self) -> Option<&PathBuf> {
        self.file_name.as_ref()
    }

    /// Whether to use the `.gitignore` file in the source directory.
    pub const fn use_gitignore(&self) -> bool {
        self.use_gitignore
    }
}

impl TryConvertNode<PathSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PathSource, Vec<PartialParsingError>> {
        let mut path = None;
        let mut patches = Vec::new();
        let mut target_directory = None;
        let mut use_gitignore = true;
        let mut file_name = None;

        self.iter().map(|(key, value)| {
            match key.as_str() {
                "path" => path = value.try_convert("path")?,
                "patches" => patches = value.try_convert("patches")?,
                "target_directory" => target_directory = value.try_convert("target_directory")?,
                "file_name" => file_name = value.try_convert("file_name")?,
                "use_gitignore" => use_gitignore = value.try_convert("use_gitignore")?,
                invalid_key => {
                    return Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_string().into()),
                        help = "valid fields for path `source` are `path`, `patches`, `target_directory`, `file_name` and `use_gitignore`"
                    )])
                }
            }
            Ok(())
        }).flatten_errors()?;

        let path = path.ok_or_else(|| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("path".into()),
                help = "path `source` must have a `path` field"
            )]
        })?;

        Ok(PathSource {
            path,
            patches,
            target_directory,
            file_name,
            use_gitignore,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_serialization() {
        let git = GitSource {
            url: GitUrl::Url(Url::parse("https://test.com/test.git").unwrap()),
            rev: GitRev::Branch("master".into()),
            depth: None,
            patches: Vec::new(),
            target_directory: None,
            lfs: false,
        };

        let yaml = serde_yaml::to_string(&git).unwrap();

        insta::assert_snapshot!(yaml);

        let parsed_git: GitSource = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed_git.url, git.url);
    }
}
