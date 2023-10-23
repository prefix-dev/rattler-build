use std::{fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};

/// Source information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    Git(GitSource),
    Url(UrlSource),
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
    pub fn folder(&self) -> Option<&PathBuf> {
        match self {
            Self::Git(git) => git.folder(),
            Self::Url(url) => url.folder(),
            Self::Path(path) => path.folder(),
        }
    }
}

impl TryConvertNode<Vec<Source>> for RenderedNode {
    fn try_convert(&self, _name: &str) -> Result<Vec<Source>, PartialParsingError> {
        let mut sources = Vec::new();

        match self {
            RenderedNode::Mapping(map) => {
                // Git source
                if map.contains_key("git_url") {
                    let git_src = map.try_convert("source")?;
                    sources.push(Source::Git(git_src));
                } else if map.contains_key("url") {
                    let url_src = map.try_convert("source")?;
                    sources.push(Source::Url(url_src));
                } else if map.contains_key("path") {
                    let path_src = map.try_convert("source")?;
                    sources.push(Source::Path(path_src));
                } else {
                    return Err(_partialerror!(
                        *self.span(),
                        ErrorKind::Other,
                        label = "unknown source type"
                    ));
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
                return Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "expected mapping or sequence"
                ))
            }
        }

        Ok(sources)
    }
}

/// Git source information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitSource {
    /// Url to the git repository
    url: GitUrl,
    /// Optionally a revision to checkout, defaults to `HEAD`
    #[serde(default)]
    rev: String,
    /// Optionally a depth to clone the repository, defaults to `None`
    depth: Option<i32>,
    /// Optionally patches to apply to the source code
    patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    folder: Option<PathBuf>,
}

impl GitSource {
    #[cfg(test)]
    pub fn create(
        url: GitUrl,
        rev: String,
        depth: Option<i32>,
        patches: Vec<PathBuf>,
        folder: Option<PathBuf>,
    ) -> Self {
        Self {
            url,
            rev,
            depth,
            patches,
            folder,
        }
    }

    /// Get the git url.
    pub const fn url(&self) -> &GitUrl {
        &self.url
    }

    /// Get the git revision.
    pub fn rev(&self) -> &str {
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

    /// Get the folder.
    pub const fn folder(&self) -> Option<&PathBuf> {
        self.folder.as_ref()
    }
}

impl TryConvertNode<GitSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<GitSource, PartialParsingError> {
        let mut url = None;
        let mut rev = None;
        let mut depth = None;
        let mut patches = Vec::new();
        let mut folder = None;

        for (k, v) in self.iter() {
            match k.as_str() {
                "git_url" => {
                    let url_str: String = v.try_convert("git_url")?;
                    let url_ = Url::from_str(&url_str);
                    match url_ {
                        Ok(url_) => url = Some(GitUrl::Url(url_)),
                        Err(err) => {
                            tracing::warn!("invalid `git_url` `{url_str}`: {err}");
                            tracing::warn!("attempting to parse as path");
                            let path = PathBuf::from(url_str);
                            url = Some(GitUrl::Path(path));
                        }
                    }
                }

                "git_rev" => {
                    rev = Some(v.try_convert("git_rev")?);
                }
                "git_depth" => {
                    depth = Some(v.try_convert("git_depth")?);
                }
                "patches" => {
                    patches = v.try_convert("patches")?;
                }
                "folder" => {
                    folder = Some(v.try_convert("folder")?);
                }
                _ => {
                    return Err(_partialerror!(
                        *k.span(),
                        ErrorKind::InvalidField(k.as_str().to_owned().into()),
                        help = "valid fields for git `source` are `git_url`, `git_rev`, `git_depth`, `patches` and `folder`"
                    ))
                }
            }
        }

        let url = url.ok_or_else(|| {
            _partialerror!(
                *self.span(),
                ErrorKind::MissingField("git_url".into()),
                help = "git `source` must have a `git_url` field"
            )
        })?;

        let rev = rev.unwrap_or_else(|| "HEAD".to_owned());

        Ok(GitSource {
            url,
            rev,
            depth,
            patches,
            folder,
        })
    }
}

/// git url
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GitUrl {
    Url(Url),
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    /// Url to the source code (usually a tar.gz or tar.bz2 etc. file)
    url: Url,
    /// Optionally a checksum to verify the downloaded file
    checksums: Vec<Checksum>,
    /// Patches to apply to the source code
    patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    folder: Option<PathBuf>,
}

impl UrlSource {
    /// Get the url.
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Get the checksum of the URL source.
    pub fn checksums(&self) -> &[Checksum] {
        self.checksums.as_slice()
    }

    /// Get the patches of the URL source.
    pub fn patches(&self) -> &[PathBuf] {
        self.patches.as_slice()
    }

    /// Get the folder of the URL source.
    pub const fn folder(&self) -> Option<&PathBuf> {
        self.folder.as_ref()
    }
}

impl TryConvertNode<UrlSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<UrlSource, PartialParsingError> {
        let mut url = None;
        let mut checksums = Vec::new();
        let mut patches = Vec::new();
        let mut folder = None;

        for (key, value) in self.iter() {
            match key.as_str() {
                "url" => url = Some(value.try_convert("url")?),
                "sha256" => {
                    let sha256_str: String = value.try_convert("sha256")?;
                    if sha256_str.len() != 64 {
                        return Err(_partialerror!(
                            *value.span(),
                            ErrorKind::InvalidSha256,
                            help = "`sha256` checksums must be 64 characters long"
                        ));
                    }
                    checksums.push(Checksum::Sha256(sha256_str));
                }
                "md5" => {
                    let md5_str: String = value.try_convert("md5")?;
                    if md5_str.len() != 32 {
                        return Err(_partialerror!(
                            *value.span(),
                            ErrorKind::InvalidMd5,
                            help = "`md5` checksums must be 32 characters long"
                        ));
                    }
                    checksums.push(Checksum::Md5(md5_str));
                }
                "patches" => patches = value.try_convert("patches")?,
                "folder" => folder = Some(value.try_convert("folder")?),
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_owned().into()),
                        help = "valid fields for URL `source` are `url`, `sha256`, `md5`, `patches` and `folder`"
                    ))
                }
            }
        }

        let url = url.ok_or_else(|| {
            _partialerror!(
                *self.span(),
                ErrorKind::MissingField("url".into()),
                help = "URL `source` must have a `url` field"
            )
        })?;

        Ok(UrlSource {
            url,
            checksums,
            patches,
            folder,
        })
    }
}

/// Checksum information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Checksum {
    Sha256(String),
    Md5(String),
}

/// A local path source. The source code will be copied to the `work`
/// (or `work/<folder>` directory).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSource {
    /// Path to the local source code
    path: PathBuf,
    /// Patches to apply to the source code
    patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    folder: Option<PathBuf>,
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

    /// Get the folder.
    pub const fn folder(&self) -> Option<&PathBuf> {
        self.folder.as_ref()
    }
}

impl TryConvertNode<PathSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PathSource, PartialParsingError> {
        let mut path = None;
        let mut patches = Vec::new();
        let mut folder = None;

        for (key, value) in self.iter() {
            match key.as_str() {
                "path" => path = Some(value.try_convert("path")?),
                "patches" => patches = value.try_convert("patches")?,
                "folder" => folder = Some(value.try_convert("folder")?),
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_string().into()),
                        help = "valid fields for path `source` are `path`, `patches` and `folder`"
                    ))
                }
            }
        }

        let path = path.ok_or_else(|| {
            _partialerror!(
                *self.span(),
                ErrorKind::MissingField("path".into()),
                help = "path `source` must have a `path` field"
            )
        })?;

        Ok(PathSource {
            path,
            patches,
            folder,
        })
    }
}
