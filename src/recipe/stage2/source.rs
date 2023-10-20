use std::{fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, Node, RenderedMappingNode, RenderedNode, SequenceNodeInternal, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1, OldRender,
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
    pub(super) fn from_stage1(
        source: stage1::Source,
        jinja: &Jinja,
    ) -> Result<Vec<Self>, PartialParsingError> {
        let mut sources = Vec::new();

        if let Some(node) = source.node {
            sources.extend(Self::from_node(&node, jinja)?);
        }

        Ok(sources)
    }

    fn from_node(node: &Node, jinja: &Jinja) -> Result<Vec<Self>, PartialParsingError> {
        let mut sources = Vec::new();

        // we are expecting mapping or sequence
        match node {
            Node::Mapping(map) => {
                // common fields
                let patches = map
                    .get("patches")
                    .map(|node| match node {
                        Node::Scalar(s) => {
                            let s = jinja.render_str(s.as_str()).map_err(|err| {
                                _partialerror!(
                                    *s.span(),
                                    ErrorKind::JinjaRendering(err),
                                    label = "error rendering patches"
                                )
                            })?;
                            Ok(vec![PathBuf::from(s)])
                        }
                        Node::Sequence(_) => todo!(),
                        Node::Mapping(_) | Node::Null(_) => Err(_partialerror!(
                            *node.span(),
                            ErrorKind::Other,
                            label = "expected scalar or sequence"
                        )),
                    })
                    .transpose()?
                    .unwrap_or_default();

                let folder = map
                    .get("folder")
                    .map(|node| match node.as_scalar() {
                        Some(s) => {
                            let s = jinja.render_str(s.as_str()).map_err(|err| {
                                _partialerror!(
                                    *s.span(),
                                    ErrorKind::JinjaRendering(err),
                                    label = "error rendering folder"
                                )
                            })?;
                            Ok(PathBuf::from(s))
                        }
                        None => Err(_partialerror!(
                            *node.span(),
                            ErrorKind::Other,
                            label = "expected scalar"
                        )),
                    })
                    .transpose()?;

                // Git source
                if map.contains_key("git_url") {
                    let git_url = map.get("git_url").unwrap();
                    let git_url = match git_url.as_scalar() {
                        Some(s) => {
                            let git_url = s.as_str();
                            let git_url = git_url.trim();
                            let git_url = jinja.render_str(git_url).map_err(|err| {
                                _partialerror!(
                                    *s.span(),
                                    ErrorKind::JinjaRendering(err),
                                    label = "error rendering git_url"
                                )
                            })?;
                            let url = Url::from_str(&git_url);
                            match url {
                                Ok(url) => GitUrl::Url(url),
                                Err(_err) => {
                                    let path = PathBuf::from(git_url.as_str());
                                    GitUrl::Path(path)
                                }
                            }
                        }
                        _ => {
                            return Err(_partialerror!(
                                *git_url.span(),
                                ErrorKind::Other,
                                label = "expected string"
                            ))
                        }
                    };

                    let rev = map
                        .get("git_rev")
                        .map(|node| match node.as_scalar() {
                            Some(rev) => jinja.render_str(rev.as_str()).map_err(|err| {
                                _partialerror!(
                                    *rev.span(),
                                    ErrorKind::JinjaRendering(err),
                                    label = "error rendering git_rev"
                                )
                            }),
                            None => Err(_partialerror!(
                                *node.span(),
                                ErrorKind::Other,
                                label = "expected scalar"
                            )),
                        })
                        .transpose()?
                        .unwrap_or_else(|| "HEAD".to_owned());

                    let depth = map
                        .get("git_depth")
                        .map(|node| match node.as_scalar() {
                            Some(s) => {
                                let depth = jinja.render_str(s.as_str()).map_err(|err| {
                                    _partialerror!(
                                        *s.span(),
                                        ErrorKind::JinjaRendering(err),
                                        label = "error rendering git_depth"
                                    )
                                })?;

                                depth.parse::<i32>().map_err(|_err| {
                                    _partialerror!(
                                        *s.span(),
                                        ErrorKind::Other,
                                        label = "error parsing `git_depth` as integer"
                                    )
                                })
                            }
                            None => Err(_partialerror!(
                                *node.span(),
                                ErrorKind::Other,
                                label = "expected scalar"
                            )),
                        })
                        .transpose()?;

                    sources.push(Self::Git(GitSource {
                        url: git_url,
                        rev,
                        depth,
                        patches,
                        folder,
                    }));
                } else if map.contains_key("url") {
                    // Url source
                    let url = map.get("url").unwrap();
                    let url = url.render(jinja, "url")?;

                    let is_sha256 = map.contains_key("sha256");
                    let is_md5 = map.contains_key("md5");
                    let checksums = match (is_sha256, is_md5) {
                        // prefer sha256 if there is both
                        (true, _) => {
                            let sha256 = map.get("sha256").unwrap();
                            match sha256.as_scalar() {
                                Some(s) => {
                                    let s = jinja.render_str(s.as_str()).map_err(|err| {
                                        _partialerror!(
                                            *s.span(),
                                            ErrorKind::JinjaRendering(err),
                                            label = "error rendering sha256"
                                        )
                                    })?;
                                    vec![Checksum::Sha256(s)]
                                }
                                _ => {
                                    return Err(_partialerror!(
                                        *sha256.span(),
                                        ErrorKind::Other,
                                        label = "expected string"
                                    ))
                                }
                            }
                        }
                        (false, true) => {
                            let md5 = map.get("md5").unwrap();
                            match md5.as_scalar() {
                                Some(s) => {
                                    let s = jinja.render_str(s.as_str()).map_err(|err| {
                                        _partialerror!(
                                            *s.span(),
                                            ErrorKind::JinjaRendering(err),
                                            label = "error rendering md5"
                                        )
                                    })?;
                                    vec![Checksum::Md5(s)]
                                }
                                _ => {
                                    return Err(_partialerror!(
                                        *md5.span(),
                                        ErrorKind::Other,
                                        label = "expected string"
                                    ))
                                }
                            }
                        }
                        (false, false) => vec![],
                    };

                    let file_name = map
                        .get("file_name")
                        .map(|node| match node.as_scalar() {
                            Some(s) => {
                                let file_name = jinja.render_str(s.as_str()).map_err(|err| {
                                    _partialerror!(
                                        *s.span(),
                                        ErrorKind::JinjaRendering(err),
                                        label = "error rendering file_name"
                                    )
                                })?;
                                Ok(file_name)
                            }
                            None => Err(_partialerror!(
                                *node.span(),
                                ErrorKind::Other,
                                label = "expected scalar"
                            )),
                        })
                        .transpose()?;

                    sources.push(Self::Url(UrlSource {
                        url,
                        checksums,
                        patches,
                        folder,
                        file_name,
                    }))
                } else if map.contains_key("path") {
                    // Path source
                    let path = map.get("path").unwrap();

                    let path = match path.as_scalar() {
                        Some(s) => {
                            let path = s.as_str();
                            let path = path.trim();
                            let path = jinja.render_str(path).map_err(|err| {
                                _partialerror!(
                                    *s.span(),
                                    ErrorKind::JinjaRendering(err),
                                    label = "error rendering path"
                                )
                            })?;
                            PathBuf::from(path)
                        }
                        _ => {
                            return Err(_partialerror!(
                                *path.span(),
                                ErrorKind::Other,
                                label = "expected string"
                            ))
                        }
                    };

                    sources.push(Self::Path(PathSource {
                        path,
                        patches,
                        folder,
                    }))
                }
            }
            Node::Sequence(s) => {
                for inner in s.iter() {
                    match inner {
                        SequenceNodeInternal::Simple(node) => {
                            sources.extend(Self::from_node(node, jinja)?)
                        }
                        SequenceNodeInternal::Conditional(if_sel) => {
                            let if_res = if_sel.process(jinja)?;
                            if let Some(if_res) = if_res {
                                sources.extend(Self::from_node(&if_res, jinja)?)
                            }
                        }
                    }
                }
            }
            Node::Scalar(s) | Node::Null(s) => {
                return Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "expected mapping or sequence"
                ))
            }
        }

        Ok(sources)
    }

    pub fn from_rendered_node(node: &RenderedNode) -> Result<Vec<Self>, PartialParsingError> {
        let mut sources = Vec::new();

        match node {
            RenderedNode::Mapping(map) => {
                // Common fields to all sources
                let patches = map
                    .get("patches")
                    .map(parse_patches)
                    .transpose()?
                    .unwrap_or_default();

                let folder = map
                    .get("folder")
                    .map(|node| node.try_convert("folder"))
                    .transpose()?;

                // Git source
                if map.contains_key("git_url") {
                    let git_src = GitSource::from_rendered_map(map, patches, folder)?;
                    sources.push(Self::Git(git_src));
                } else if map.contains_key("url") {
                    let url_src = UrlSource::from_rendered_map(map, patches, folder)?;
                    sources.push(Self::Url(url_src));
                } else if map.contains_key("path") {
                    let path_src = PathSource::from_rendered_map(map, patches, folder)?;
                    sources.push(Self::Path(path_src));
                } else {
                    return Err(_partialerror!(
                        *node.span(),
                        ErrorKind::Other,
                        label = "unknown source type"
                    ));
                }
            }
            RenderedNode::Sequence(seq) => {
                for n in seq.iter() {
                    sources.extend(Self::from_rendered_node(n)?)
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

fn parse_patches(node: &RenderedNode) -> Result<Vec<PathBuf>, PartialParsingError> {
    let mut patches = Vec::new();

    match node {
        RenderedNode::Scalar(s) => {
            let s = s.try_convert("patches")?;
            patches.push(s);
        }
        RenderedNode::Sequence(seq) => {
            for n in seq.iter() {
                patches.extend(parse_patches(n)?)
            }
        }
        RenderedNode::Null(_) => (),
        RenderedNode::Mapping(map) => {
            return Err(_partialerror!(
                *map.span(),
                ErrorKind::Other,
                label = "expected scalar or sequence"
            ))
        }
    }

    Ok(patches)
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

    pub(super) fn from_rendered_map(
        map: &RenderedMappingNode,
        patches: Vec<PathBuf>,
        folder: Option<PathBuf>,
    ) -> Result<Self, PartialParsingError> {
        // Error on invalid fields for git source
        let invalid_field = map.keys().find(|k| {
            matches!(
                k.as_str(),
                "git_url" | "git_rev" | "git_depth" | "patches" | "folder"
            )
        });

        if let Some(invalid_field) = invalid_field {
            return Err(_partialerror!(
        *invalid_field.span(),
        ErrorKind::InvalidField(invalid_field.as_str().to_owned().into()),
        help = "valid fields for git `source` are `git_url`, `git_rev`, `git_depth`, `patches` and `folder`"
    ));
        }

        // Ok to unwrap because we just checked if it exists in this map
        let url = match map.get("git_url").unwrap().as_scalar() {
            Some(s) => {
                let url = Url::from_str(s.as_str());
                match url {
                    Ok(url) => GitUrl::Url(url),
                    Err(err) => {
                        tracing::warn!("invalid `git_url` `{}`: {err}", s.as_str());
                        tracing::warn!("attempting to parse as path");
                        let path = PathBuf::from(s.as_str());
                        GitUrl::Path(path)
                    }
                }
            }
            None => {
                return Err(_partialerror!(
                    *map.span(),
                    ErrorKind::ExpectedScalar,
                    label = "expected a string here"
                ))
            }
        };

        let rev = map
            .get("git_rev")
            .map(|node| match node.as_scalar() {
                Some(rev) => Ok(rev.as_str()),
                None => Err(_partialerror!(*node.span(), ErrorKind::ExpectedScalar)),
            })
            .transpose()?
            .unwrap_or("HEAD")
            .to_string();

        let depth = map
            .get("git_depth")
            .map(|node| match node.as_scalar() {
                Some(s) => {
                    let depth = s.as_str();
                    depth.parse::<i32>().map_err(|err| {
                        _partialerror!(
                            *s.span(),
                            ErrorKind::from(err),
                            label = "`git_depth` value must be a integer"
                        )
                    })
                }
                None => Err(_partialerror!(*node.span(), ErrorKind::ExpectedScalar)),
            })
            .transpose()?;

        Ok(GitSource {
            url,
            rev,
            depth,
            patches,
            folder,
        })
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
    /// Optionally a file name to rename the downloaded file (does not apply to archives)
    file_name: Option<String>,
}

impl UrlSource {
    pub(super) fn from_rendered_map(
        map: &RenderedMappingNode,
        patches: Vec<PathBuf>,
        folder: Option<PathBuf>,
    ) -> Result<Self, PartialParsingError> {
        let invalid_field = map
            .keys()
            .find(|k| matches!(k.as_str(), "url" | "sha256" | "md5" | "patches" | "folder"));

        if let Some(invalid_field) = invalid_field {
            return Err(_partialerror!(
                *invalid_field.span(),
                ErrorKind::InvalidField(invalid_field.as_str().to_owned().into()),
                help = "valid fields for URL `source` are `url`, `sha256`, `md5`, `patches` and `folder`"
            ));
        }

        let url = map.get("url").ok_or_else(|| {
            _partialerror!(
                *map.span(),
                ErrorKind::MissingField("url".into()),
                help = "URL `source` must have a `url` field"
            )
        })?;

        let url = url.try_convert("url")?;
        let sha256 = map
            .get("sha256")
            .map(|sha256| {
                let sha256_str: String = sha256.try_convert("sha256")?;
                if sha256_str.len() != 64 {
                    return Err(_partialerror!(
                        *sha256.span(),
                        ErrorKind::InvalidSha256,
                        help = "sha256 checksums must be 64 characters long"
                    ));
                }
                Ok(Checksum::Sha256(sha256_str))
            })
            .transpose()?;
        let md5 = map
            .get("md5")
            .map(|md5| {
                let md5_str: String = md5.try_convert("md5")?;
                if md5_str.len() != 32 {
                    return Err(_partialerror!(
                        *md5.span(),
                        ErrorKind::InvalidMd5,
                        help = "md5 checksums must be 32 characters long"
                    ));
                }
                Ok(Checksum::Md5(md5_str))
            })
            .transpose()?;
        let checksums = match (sha256, md5) {
            (None, None) => Vec::new(),
            (Some(ck), None) | (None, Some(ck)) => vec![ck],
            (Some(sha256), Some(md5)) => vec![sha256, md5],
        };

        Ok(UrlSource {
            url,
            checksums,
            patches,
            folder,
        })
    }

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

    /// Get the file name of the URL source.
    pub const fn file_name(&self) -> Option<&String> {
        self.file_name.as_ref()
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
    pub(super) fn from_rendered_map(
        map: &RenderedMappingNode,
        patches: Vec<PathBuf>,
        folder: Option<PathBuf>,
    ) -> Result<Self, PartialParsingError> {
        let invalid_field = map
            .keys()
            .find(|k| matches!(k.as_str(), "path" | "patches" | "folder"));

        if let Some(invalid_field) = invalid_field {
            return Err(_partialerror!(
                *invalid_field.span(),
                ErrorKind::InvalidField(invalid_field.as_str().to_owned().into()),
                help = "valid fields for path `source` are `path`, `patches` and `folder`"
            ));
        }

        let path = map
            .get("path")
            .ok_or_else(|| {
                _partialerror!(
                    *map.span(),
                    ErrorKind::MissingField("path".into()),
                    help = "path `source` must have a `path` field"
                )
            })?
            .try_convert("path")?;

        Ok(PathSource {
            path,
            patches,
            folder,
        })
    }

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
