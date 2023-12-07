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
};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<i32>,
    /// Optionally patches to apply to the source code
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patches: Vec<PathBuf>,
    /// Optionally a folder name under the `work` directory to place the source code
    #[serde(skip_serializing_if = "Option::is_none")]
    folder: Option<PathBuf>,
    /// Optionally request the lfs pull in git source
    #[serde(skip_serializing_if = "should_not_serialize_lfs")]
    lfs: bool,
}

/// A helper method to skip serializing the lfs flag if it is false.
fn should_not_serialize_lfs(lfs: &bool) -> bool {
    !lfs
}

impl GitSource {
    #[cfg(test)]
    pub fn create(
        url: GitUrl,
        rev: String,
        depth: Option<i32>,
        patches: Vec<PathBuf>,
        folder: Option<PathBuf>,
        lfs: bool,
    ) -> Self {
        Self {
            url,
            rev,
            depth,
            patches,
            folder,
            lfs,
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

    /// Get true if source requires lfs.
    pub const fn lfs(&self) -> bool {
        self.lfs
    }
}

impl TryConvertNode<GitSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<GitSource, PartialParsingError> {
        let mut url = None;
        let mut rev = None;
        let mut depth = None;
        let mut patches = Vec::new();
        let mut folder = None;
        let mut lfs = false;

        // TODO: is there a better place for this error?
        // raising the error during parsing allows us to suggest fixes in future
        // in case we build linting functionality on top
        if self.contains_key("git_rev") {
            if let Some((k, _)) = self.get_key_value("git_depth") {
                return Err(_partialerror!(
                    *k.span(),
                    ErrorKind::InvalidField(k.as_str().to_owned().into()),
                    help = "use of `git_depth` with `git_rev` is invalid"
                ));
            }
        }

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
                "lfs" => {
                    lfs = v.try_convert("lfs")?;
                }
                _ => {
                    return Err(_partialerror!(
                        *k.span(),
                        ErrorKind::InvalidField(k.as_str().to_owned().into()),
                        help = "valid fields for git `source` are `git_url`, `git_rev`, `git_depth`, `patches`, `lfs` and `folder`"
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
    folder: Option<PathBuf>,
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
    pub const fn folder(&self) -> Option<&PathBuf> {
        self.folder.as_ref()
    }

    /// Get the file name of the URL source.
    pub const fn file_name(&self) -> Option<&String> {
        self.file_name.as_ref()
    }
}

impl TryConvertNode<UrlSource> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<UrlSource, PartialParsingError> {
        let mut url = None;
        let mut sha256 = None;
        let mut md5 = None;
        let mut patches = Vec::new();
        let mut folder = None;
        let mut file_name = None;

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "url" => url = value.try_convert(key_str)?,
                "sha256" => {
                    let sha256_str: RenderedScalarNode = value.try_convert(key_str)?;
                    let sha256_out = rattler_digest::parse_digest_from_hex::<Sha256>(sha256_str.as_str()).ok_or_else(|| _partialerror!(*sha256_str.span(), ErrorKind::InvalidSha256))?;
                    sha256 = Some(sha256_out);
                }
                "md5" => {
                    let md5_str: RenderedScalarNode = value.try_convert(key_str)?;
                    let md5_out = rattler_digest::parse_digest_from_hex::<Md5>(md5_str.as_str()).ok_or_else(|| _partialerror!(*md5_str.span(), ErrorKind::InvalidMd5))?;
                    md5 = Some(md5_out);
                }
                "file_name" => file_name = value.try_convert(key_str)?,
                "patches" => patches = value.try_convert(key_str)?,
                "folder" => folder = value.try_convert(key_str)?,
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_owned().into()),
                        help = "valid fields for URL `source` are `url`, `sha256`, `md5`, `patches`, `file_name` and `folder`"
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

        if md5.is_none() && sha256.is_none() {
            return Err(_partialerror!(
                *self.span(),
                ErrorKind::MissingField("sha256 or md5".into()),
                help = "URL `source` must have a `sha256` or `md5` checksum field"
            ));
        }

        Ok(UrlSource {
            url,
            md5,
            sha256,
            file_name,
            patches,
            folder,
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
    folder: Option<PathBuf>,
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

    /// Get the folder.
    pub const fn folder(&self) -> Option<&PathBuf> {
        self.folder.as_ref()
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
    fn try_convert(&self, _name: &str) -> Result<PathSource, PartialParsingError> {
        let mut path = None;
        let mut patches = Vec::new();
        let mut folder = None;
        let mut use_gitignore = true;
        let mut file_name = None;

        for (key, value) in self.iter() {
            match key.as_str() {
                "path" => path = value.try_convert("path")?,
                "patches" => patches = value.try_convert("patches")?,
                "folder" => folder = value.try_convert("folder")?,
                "file_name" => file_name = value.try_convert("file_name")?,
                "use_gitignore" => use_gitignore = value.try_convert("use_gitignore")?,
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_string().into()),
                        help = "valid fields for path `source` are `path`, `patches`, `folder`, `file_name` and `use_gitignore`"
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
            file_name,
            use_gitignore,
        })
    }
}
