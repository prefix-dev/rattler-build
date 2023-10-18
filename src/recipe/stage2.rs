//! Second and final stage of the recipe parser pipeline.
//!
//! This stage takes the [`RawRecipe`] from the first stage and parses it into a [`Recipe`], where
//! if-selectors are handled and any jinja string is processed, resulting in a rendered recipe.

use std::{collections::BTreeMap, fmt, path::PathBuf, str::FromStr};

use minijinja::Value;
use rattler_conda_types::{package::EntryPoint, MatchSpec, NoArchKind, NoArchType, PackageName};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    _partialerror,
    recipe::{
        error::{ErrorKind, ParsingError, PartialParsingError},
        jinja::{Jinja, Pin},
        stage1::{
            self,
            node::{MappingNode, ScalarNode, SequenceNodeInternal},
            Node, RawRecipe,
        },
    },
    selectors::SelectorConfig,
};

/// A recipe that has been parsed and validated.
#[derive(Debug, Clone, Serialize)]
pub struct Recipe {
    package: Package,
    source: Vec<Source>,
    build: Build,
    requirements: Requirements,
    test: Test,
    about: About,
    extra: (),
}

impl Recipe {
    /// Build a recipe from a YAML string.
    pub fn from_yaml(yaml: &str, jinja_opt: SelectorConfig) -> Result<Self, ParsingError> {
        let raw = RawRecipe::from_yaml(yaml)?;
        Self::from_raw(raw, jinja_opt).map_err(|err| ParsingError::from_partial(yaml, err))
    }

    /// Build a recipe from a YAML string and use a given package hash string as default value.
    pub fn from_yaml_with_default_hash_str(
        yaml: &str,
        default_pkg_hash: &str,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, ParsingError> {
        let mut recipe = Self::from_yaml(yaml, jinja_opt)?;

        // Set the build string to the package hash if it is not set
        if recipe.build.string.is_none() {
            recipe.build.string = Some(format!("{}_{}", default_pkg_hash, recipe.build.number));
        }
        Ok(recipe)
    }

    /// Build a recipe from a [`RawRecipe`].
    pub fn from_raw(
        raw: RawRecipe,
        jinja_opt: SelectorConfig,
    ) -> Result<Self, PartialParsingError> {
        // Init minijinja
        let mut jinja = Jinja::new(jinja_opt);

        for (k, v) in raw.context {
            let rendered = jinja.render_str(v.as_str()).map_err(|err| {
                _partialerror!(
                    *v.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering context"
                )
            })?;

            jinja
                .context_mut()
                .insert(k.as_str().to_owned(), Value::from_safe_string(rendered));
        }

        let package = Package::from_stage1(&raw.package, &jinja)?;
        let source = Source::from_stage1(raw.source, &jinja)?;

        let about = raw
            .about
            .as_ref()
            .map(|about| About::from_stage1(about, &jinja))
            .transpose()?
            .unwrap_or_default();

        let requirements = raw
            .requirements
            .as_ref()
            .map(|req| Requirements::from_stage1(req, &jinja))
            .transpose()?
            .unwrap_or_default();

        let build = Build::from_stage1(&raw.build, &jinja)?;
        let test = Test::from_stage1(&raw.test, &jinja)?;

        Ok(Self {
            package,
            source,
            build,
            requirements,
            test,
            about,
            extra: (),
        })
    }

    /// Get the package information.
    pub const fn package(&self) -> &Package {
        &self.package
    }

    /// Get the source information.
    pub fn sources(&self) -> &[Source] {
        self.source.as_slice()
    }

    /// Get the build information.
    pub const fn build(&self) -> &Build {
        &self.build
    }

    /// Get the requirements information.
    pub const fn requirements(&self) -> &Requirements {
        &self.requirements
    }

    /// Get the test information.
    pub const fn test(&self) -> &Test {
        &self.test
    }

    /// Get the about information.
    pub const fn about(&self) -> &About {
        &self.about
    }
}

/// A recipe package information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    name: PackageName,
    version: String,
}

impl Package {
    fn from_stage1(package: &stage1::Package, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        let name = jinja.render_str(package.name.as_str()).map_err(|err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::JinjaRendering(err),
                label = "error rendering package name"
            )
        })?;
        let name = PackageName::from_str(name.as_str()).map_err(|_err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::Other,
                label = "error parsing package name"
            )
        })?;
        let version = jinja.render_str(package.version.as_str()).map_err(|err| {
            _partialerror!(
                *package.name.span(),
                ErrorKind::JinjaRendering(err),
                label = "error rendering package version"
            )
        })?;
        Ok(Package { name, version })
    }

    /// Get the package name.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Get the package version.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// Source information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    Git(GitSource),
    Url(UrlSource),
    Path(PathSource),
}

impl Source {
    fn from_stage1(
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
                        Node::Mapping(_) => Err(_partialerror!(
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
                    let url = match url.as_scalar() {
                        Some(s) => {
                            let url = s.as_str();
                            let url = url.trim();
                            let url = jinja.render_str(url).map_err(|err| {
                                _partialerror!(
                                    *s.span(),
                                    ErrorKind::JinjaRendering(err),
                                    label = "error rendering url"
                                )
                            })?;
                            Url::from_str(&url).map_err(|_err| {
                                _partialerror!(
                                    *s.span(),
                                    ErrorKind::Other,
                                    label = "error parsing url"
                                )
                            })
                        }
                        _ => {
                            return Err(_partialerror!(
                                *url.span(),
                                ErrorKind::Other,
                                label = "expected string"
                            ))
                        }
                    }?;

                    let is_sha256 = map.contains_key("sha256");
                    let is_md5 = map.contains_key("md5");
                    let checksum = match (is_sha256, is_md5) {
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
                                    Some(Checksum::Sha256(s))
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
                                    Some(Checksum::Md5(s))
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
                        (false, false) => None,
                    };

                    sources.push(Self::Url(UrlSource {
                        url,
                        checksum,
                        patches,
                        folder,
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
            Node::Scalar(s) => {
                return Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "expected mapping or sequence, got scalar"
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
    checksum: Option<Checksum>,
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
    pub const fn checksum(&self) -> Option<&Checksum> {
        self.checksum.as_ref()
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

/// About information.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct About {
    homepage: Option<Url>,
    repository: Option<Url>,
    documentation: Option<Url>,
    license: Option<String>,
    license_family: Option<String>,
    license_files: Vec<String>,
    license_url: Option<Url>,
    summary: Option<String>,
    description: Option<String>,
    prelink_message: Option<String>,
}

impl About {
    fn from_stage1(about: &stage1::About, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        let homepage = about
            .homepage
            .as_ref()
            .and_then(|n| n.as_scalar())
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.homepage.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering homepage"
                )
            })?
            .map(|url| Url::from_str(&url).unwrap());
        let repository = about
            .repository
            .as_ref()
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.repository.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering repository"
                )
            })?
            .map(|url| Url::from_str(url.as_str()).unwrap());
        let documentation = about
            .documentation
            .as_ref()
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.repository.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering repository"
                )
            })?
            .map(|url| Url::from_str(url.as_str()).unwrap());
        let license = about.license.as_ref().map(|s| s.as_str().to_owned());
        let license_family = about.license_family.as_ref().map(|s| s.as_str().to_owned());
        let license_url = about
            .license_url
            .as_ref()
            .map(|s| s.as_str().to_owned())
            .map(|url| Url::from_str(&url).unwrap());
        let license_files = about
            .license_file
            .as_ref()
            .map(|node| parse_license_files(node, jinja))
            .transpose()?
            .unwrap_or_default();
        let summary = about.summary.as_ref().map(|s| s.as_str().to_owned());
        let description = about.description.as_ref().map(|s| s.as_str().to_owned());
        let prelink_message = about
            .prelink_message
            .as_ref()
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.prelink_message.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering prelink_message"
                )
            })?;

        Ok(Self {
            homepage,
            repository,
            documentation,
            license,
            license_family,
            license_files,
            license_url,
            summary,
            description,
            prelink_message,
        })
    }

    /// Get the homepage.
    pub const fn homepage(&self) -> Option<&Url> {
        self.homepage.as_ref()
    }

    /// Get the repository.
    pub const fn repository(&self) -> Option<&Url> {
        self.repository.as_ref()
    }

    /// Get the documentation.
    pub const fn documentation(&self) -> Option<&Url> {
        self.documentation.as_ref()
    }

    /// Get the license.
    pub fn license(&self) -> Option<&str> {
        self.license.as_deref()
    }

    /// Get the license family.
    pub fn license_family(&self) -> Option<&str> {
        self.license_family.as_deref()
    }

    /// Get the license file.
    pub fn license_files(&self) -> &[String] {
        self.license_files.as_slice()
    }

    /// Get the license url.
    pub const fn license_url(&self) -> Option<&Url> {
        self.license_url.as_ref()
    }

    /// Get the summary.
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// Get the description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Get the prelink message.
    pub fn prelink_message(&self) -> Option<&str> {
        self.prelink_message.as_deref()
    }
}

fn parse_license_files(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let script = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `script`"
                )
            })?;
            Ok(vec![script])
        }
        Node::Sequence(seq) => {
            let mut scripts = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => scripts.extend(parse_script(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            scripts.extend(parse_script(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(scripts)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

/// The requirements at build- and runtime are defined in the `requirements` section of the recipe.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Requirements {
    /// Requirements at _build_ time are requirements that can
    /// be run on the machine that is executing the build script.
    /// The environment will thus be resolved with the appropriate platform
    /// that is currently running (e.g. on linux-64 it will be resolved with linux-64).
    /// Typically things like compilers, build tools, etc. are installed here.
    #[serde(default)]
    pub build: Vec<Dependency>,
    /// Requirements at _host_ time are requirements that the final executable is going
    /// to _link_ against. The environment will be resolved with the target_platform
    /// architecture (e.g. if you build _on_ linux-64 _for_ linux-aarch64, then the
    /// host environment will be resolved with linux-aarch64).
    ///
    /// Typically things like libraries, headers, etc. are installed here.
    #[serde(default)]
    pub host: Vec<Dependency>,
    /// Requirements at _run_ time are requirements that the final executable is going
    /// to _run_ against. The environment will be resolved with the target_platform
    /// at runtime.
    #[serde(default)]
    pub run: Vec<Dependency>,
    /// Constrains are optional runtime requirements that are used to constrain the
    /// environment that is resolved. They are not installed by default, but when
    /// installed they will have to conform to the constrains specified here.
    #[serde(default)]
    pub run_constrained: Vec<Dependency>,
}

impl Requirements {
    fn from_stage1(req: &stage1::Requirements, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        let build = req
            .build
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());
        let host = req
            .host
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());
        let run = req
            .run
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());
        let run_constrained = req
            .run_constrained
            .as_ref()
            .map(|node| Dependency::from_node(node, jinja))
            .transpose()?
            .unwrap_or(Vec::new());

        Ok(Self {
            build,
            host,
            run,
            run_constrained,
        })
    }

    /// Get the build requirements.
    pub fn build(&self) -> &[Dependency] {
        self.build.as_slice()
    }

    /// Get the host requirements.
    pub fn host(&self) -> &[Dependency] {
        self.host.as_slice()
    }

    /// Get the run requirements.
    pub fn run(&self) -> &[Dependency] {
        self.run.as_slice()
    }

    /// Get the run constrained requirements.
    pub fn run_constrained(&self) -> &[Dependency] {
        self.run_constrained.as_slice()
    }

    /// Get all requirements in one iterator.
    pub fn all(&self) -> impl Iterator<Item = &Dependency> {
        self.build
            .iter()
            .chain(self.host.iter())
            .chain(self.run.iter())
            .chain(self.run_constrained.iter())
    }

    /// Check if all requirements are empty.
    pub fn is_empty(&self) -> bool {
        self.build.is_empty()
            && self.host.is_empty()
            && self.run.is_empty()
            && self.run_constrained.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinSubpackage {
    pin_subpackage: Pin,
}

impl PinSubpackage {
    /// Get the [`Pin`] value.
    pub const fn pin_value(&self) -> &Pin {
        &self.pin_subpackage
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Compiler {
    compiler: String,
}

impl Compiler {
    /// Get the compiler value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.compiler
    }

    /// Get the compiler value without the `__COMPILER` prefix.
    pub fn without_prefix(&self) -> &str {
        self.compiler
            .strip_prefix("__COMPILER ")
            .expect("compiler without prefix")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Dependency {
    #[serde(deserialize_with = "deserialize_match_spec")]
    Spec(MatchSpec),
    PinSubpackage(PinSubpackage),
    Compiler(Compiler),
}

impl Dependency {
    fn from_node(node: &Node, jinja: &Jinja) -> Result<Vec<Self>, PartialParsingError> {
        match node {
            Node::Scalar(s) => {
                let dep = Self::from_scalar(s, jinja)?;
                Ok(vec![dep])
            }
            Node::Sequence(seq) => {
                let mut deps = Vec::new();
                for inner in seq.iter() {
                    match inner {
                        SequenceNodeInternal::Simple(n) => deps.extend(Self::from_node(n, jinja)?),
                        SequenceNodeInternal::Conditional(if_sel) => {
                            let if_res = if_sel.process(jinja)?;
                            if let Some(if_res) = if_res {
                                deps.extend(Self::from_node(&if_res, jinja)?)
                            }
                        }
                    }
                }
                Ok(deps)
            }
            Node::Mapping(_) => Err(_partialerror!(
                *node.span(),
                ErrorKind::Other,
                label = "expected scalar or sequence"
            )),
        }
    }

    fn from_scalar(s: &ScalarNode, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        // compiler
        if s.as_str().contains("compiler(") {
            let compiler = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering compiler"
                )
            })?;
            Ok(Self::Compiler(Compiler { compiler }))
        } else if s.as_str().contains("pin_subpackage(") {
            let pin_subpackage = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering pin_subpackage"
                )
            })?;

            // Panic should never happen from this strip unless the prefix magic for the pin
            // subpackage changes
            let internal_repr = pin_subpackage
                .strip_prefix("__PIN_SUBPACKAGE ")
                .expect("pin subpackage without prefix __PIN_SUBPACKAGE ");
            let pin_subpackage = Pin::from_internal_repr(internal_repr);
            Ok(Self::PinSubpackage(PinSubpackage { pin_subpackage }))
        } else {
            let spec = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering spec"
                )
            })?;
            let spec = MatchSpec::from_str(&spec).map_err(|_err| {
                _partialerror!(*s.span(), ErrorKind::Other, label = "error parsing spec")
            })?;
            Ok(Self::Spec(spec))
        }
    }
}

impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DependencyVisitor;

        impl<'de> serde::de::Visitor<'de> for DependencyVisitor {
            type Value = Dependency;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(
                    "a string starting with '__COMPILER', '__PIN_SUBPACKAGE', or a MatchSpec",
                )
            }

            fn visit_str<E>(self, value: &str) -> Result<Dependency, E>
            where
                E: serde::de::Error,
            {
                if let Some(compiler) = value.strip_prefix("__COMPILER ") {
                    Ok(Dependency::Compiler(Compiler {
                        compiler: compiler.to_lowercase(),
                    }))
                } else if let Some(pin) = value.strip_prefix("__PIN_SUBPACKAGE ") {
                    Ok(Dependency::PinSubpackage(PinSubpackage {
                        pin_subpackage: Pin::from_internal_repr(pin),
                    }))
                } else {
                    // Assuming MatchSpec can be constructed from a string.
                    MatchSpec::from_str(value)
                        .map(Dependency::Spec)
                        .map_err(serde::de::Error::custom)
                }
            }
        }

        deserializer.deserialize_str(DependencyVisitor)
    }
}

/// The build options contain information about how to build the package and some additional
/// metadata about the package.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Build {
    /// The build number is a number that should be incremented every time the recipe is built.
    number: u64,
    /// The build string is usually set automatically as the hash of the variant configuration.
    /// It's possible to override this by setting it manually, but not recommended.
    string: Option<String>,
    /// List of conditions under which to skip the build of the package.
    skip: Vec<Value>,
    /// The build script can be either a list of commands or a path to a script. By
    /// default, the build script is set to `build.sh` or `build.bat` on Unix and Windows respectively.
    script: Vec<String>,
    /// Environment variables to pass through or set in the script
    script_env: ScriptEnv,
    /// A recipe can choose to ignore certain run exports of its dependencies
    ignore_run_exports: Vec<PackageName>,
    /// A recipe can choose to ignore all run exports of coming from some packages
    ignore_run_exports_from: Vec<PackageName>,
    /// The recipe can specify a list of run exports that it provides
    run_exports: RunExports,
    /// A noarch package runs on any platform. It can be either a python package or a generic package.
    noarch: NoArchType,
    /// For a Python noarch package to have executables it is necessary to specify the python entry points.
    /// These contain the name of the executable and the module + function that should be executed.
    entry_points: Vec<EntryPoint>,
    // TODO: Add and parse the rest of the fields
}

impl Build {
    fn from_stage1(build: &stage1::Build, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        Ok(build
            .node
            .as_ref()
            .map(|node| Self::from_node(node, jinja))
            .transpose()?
            .unwrap_or_default())
    }

    fn from_node(node: &MappingNode, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        let mut build = Self::default();

        for (key, value) in node.iter() {
            match key.as_str() {
                "number" => {
                    let number = value.as_scalar().ok_or_else(|| {
                        _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
                    })?;
                    let number = jinja.render_str(number.as_str()).map_err(|err| {
                        _partialerror!(
                            *number.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering number"
                        )
                    })?;
                    let number = number.parse::<u64>().map_err(|_err| {
                        _partialerror!(
                            *value.span(),
                            ErrorKind::Other,
                            label = "error parsing number"
                        )
                    })?;
                    build.number = number;
                }
                "string" => {
                    let string = value.as_scalar().ok_or_else(|| {
                        _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
                    })?;
                    let string = jinja.render_str(string.as_str()).map_err(|err| {
                        _partialerror!(
                            *string.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering string"
                        )
                    })?;
                    build.string = Some(string);
                }
                "skip" => build.skip = parse_skip(value, jinja)?,
                "script" => build.script = parse_script(value, jinja)?,
                "script_env" => build.script_env = ScriptEnv::from_node(value, jinja)?,
                "ignore_run_exports" => {
                    build.ignore_run_exports = parse_ignore_run_exports(value, jinja)?;
                }
                "ignore_run_exports_from" => {
                    // Abuse parse_ignore_run_exports since in structure they are the same
                    // We may want to change this in the future for better error messages.
                    build.ignore_run_exports_from = parse_ignore_run_exports(value, jinja)?;
                }
                "noarch" => {
                    let noarch = value.as_scalar().ok_or_else(|| {
                        _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
                    })?;
                    let noarch = jinja.render_str(noarch.as_str()).map_err(|err| {
                        _partialerror!(
                            *noarch.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering noarch"
                        )
                    })?;
                    let noarch = match noarch.as_str() {
                        "python" => NoArchType::python(),
                        "generic" => NoArchType::generic(),
                        _ => {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::Other,
                                label = "expected `python` or `generic`"
                            ))
                        }
                    };
                    build.noarch = noarch;
                }
                "run_exports" => {
                    build.run_exports = RunExports::from_node(value, jinja)?;
                }
                "entry_points" => {
                    if let Some(NoArchKind::Generic) = build.noarch.kind() {
                        return Err(_partialerror!(
                            *key.span(),
                            ErrorKind::Other,
                            label = "entry_points are only allowed for python noarch packages"
                        ));
                    }

                    build.entry_points = parse_entry_points(value, jinja)?;
                }
                _ => unimplemented!("unimplemented field: {}", key.as_str()),
            }
        }

        Ok(build)
    }

    /// Get the build number.
    pub const fn number(&self) -> u64 {
        self.number
    }

    /// Get the build string.
    pub fn string(&self) -> Option<&str> {
        self.string.as_deref()
    }

    /// Get the skip conditions.
    pub fn skip(&self) -> &[Value] {
        self.skip.as_slice()
    }

    /// Get the build script.
    pub fn scripts(&self) -> &[String] {
        self.script.as_slice()
    }

    /// Get the build script environment.
    pub const fn script_env(&self) -> &ScriptEnv {
        &self.script_env
    }

    /// Get run exports.
    pub const fn run_exports(&self) -> &RunExports {
        &self.run_exports
    }

    /// Get the ignore run exports.
    ///
    /// A recipe can choose to ignore certain run exports of its dependencies
    pub fn ignore_run_exports(&self) -> &[PackageName] {
        self.ignore_run_exports.as_slice()
    }

    /// Get the ignore run exports from.
    ///
    /// A recipe can choose to ignore all run exports of coming from some packages
    pub fn ignore_run_exports_from(&self) -> &[PackageName] {
        self.ignore_run_exports_from.as_slice()
    }

    /// Get the noarch type.
    pub const fn noarch(&self) -> &NoArchType {
        &self.noarch
    }

    /// Get the entry points.
    pub fn entry_points(&self) -> &[EntryPoint] {
        self.entry_points.as_slice()
    }

    /// Check if the build should be skipped.
    pub fn is_skip_build(&self) -> bool {
        !self.skip.is_empty() && self.skip.iter().any(|v| v.is_true())
    }
}

fn parse_skip(node: &Node, jinja: &Jinja) -> Result<Vec<Value>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let skip = jinja.eval(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error evaluating `skip` expression"
                )
            })?;
            Ok(vec![skip])
        }
        Node::Sequence(seq) => {
            let mut skip = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => skip.extend(parse_skip(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            skip.extend(parse_skip(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(skip)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_script(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let script = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `script`"
                )
            })?;
            Ok(vec![script])
        }
        Node::Sequence(seq) => {
            let mut scripts = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => scripts.extend(parse_script(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            scripts.extend(parse_script(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(scripts)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_entry_points(node: &Node, jinja: &Jinja) -> Result<Vec<EntryPoint>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let entry_point = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `entry_points`"
                )
            })?;
            let entry_point = EntryPoint::from_str(&entry_point).map_err(|_err| {
                // TODO: Better handling of this
                _partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "error in the entrypoint format"
                )
            })?;
            Ok(vec![entry_point])
        }
        Node::Sequence(seq) => {
            let mut entry_points = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        entry_points.extend(parse_entry_points(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            entry_points.extend(parse_entry_points(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(entry_points)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_ignore_run_exports(
    node: &Node,
    jinja: &Jinja,
) -> Result<Vec<PackageName>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let ignore_run_export = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `ignore_run_exports`"
                )
            })?;

            if ignore_run_export.is_empty() {
                Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "empty string is not allowed in `ignore_run_exports`"
                ))
            } else {
                let ignore_run_export =
                    PackageName::from_str(&ignore_run_export).map_err(|_err| {
                        // TODO: Better handling of this
                        _partialerror!(
                            *s.span(),
                            ErrorKind::Other,
                            label = "error parsing `ignore_run_exports`"
                        )
                    })?;
                Ok(vec![ignore_run_export])
            }
        }
        Node::Sequence(seq) => {
            let mut ignore_run_exports = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        ignore_run_exports.extend(parse_ignore_run_exports(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            ignore_run_exports.extend(parse_ignore_run_exports(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(ignore_run_exports)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

/// Extra environment variables to set during the build script execution
#[derive(Debug, Default, Clone, Serialize)]
pub struct ScriptEnv {
    /// Environments variables to leak into the build environment from the host system.
    /// During build time these variables are recorded and stored in the package output.
    /// Use `secrets` for environment variables that should not be recorded.
    passthrough: Vec<String>,
    /// Environment variables to set in the build environment.
    env: BTreeMap<String, String>,
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information. Use with care because this might make recipes no
    /// longer reproducible on other machines.
    secrets: Vec<String>,
}

impl ScriptEnv {
    fn from_node(node: &Node, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        if let Some(map) = node.as_mapping() {
            let env = map
                .get("env")
                .map(|node| parse_env(node, jinja))
                .transpose()?
                .unwrap_or_default();

            let passthrough = map
                .get("passthrough")
                .map(|node| parse_passthrough(node, jinja))
                .transpose()?
                .unwrap_or_default();

            let secrets = map
                .get("secrets")
                .map(|node| parse_secrets(node, jinja))
                .transpose()?
                .unwrap_or_default();

            Ok(Self {
                passthrough,
                env,
                secrets,
            })
        } else {
            Err(_partialerror!(
                *node.span(),
                ErrorKind::Other,
                label = "expected mapping on `script_env`"
            ))
        }
    }

    /// Check if the script environment is empty is all its fields.
    pub fn is_empty(&self) -> bool {
        self.passthrough.is_empty() && self.env.is_empty() && self.secrets.is_empty()
    }

    /// Get the passthrough environment variables.
    ///
    /// Those are the environments variables to leak into the build environment from the host system.
    ///
    /// During build time these variables are recorded and stored in the package output.
    /// Use `secrets` for environment variables that should not be recorded.
    pub fn passthrough(&self) -> &[String] {
        self.passthrough.as_slice()
    }

    /// Get the environment variables to set in the build environment.
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Get the secrets environment variables.
    ///
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information.
    ///
    /// # Warning
    /// Use with care because this might make recipes no longer reproducible on other machines.
    pub fn secrets(&self) -> &[String] {
        self.secrets.as_slice()
    }
}

fn parse_env(node: &Node, jinja: &Jinja) -> Result<BTreeMap<String, String>, PartialParsingError> {
    if let Some(map) = node.as_mapping() {
        let mut env = BTreeMap::new();
        for (key, value) in map.iter() {
            let key = key.as_str();
            let value = value.as_scalar().ok_or_else(|| {
                _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
            })?;
            let value = jinja.render_str(value.as_str()).map_err(|err| {
                _partialerror!(
                    *value.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `env` map value"
                )
            })?;
            env.insert(key.to_owned(), value);
        }
        Ok(env)
    } else {
        Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected mapping on `env`"
        ))
    }
}

// TODO: make the `secrets` not possible to be seen in the memory
fn parse_secrets(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let secret = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `secrets`"
                )
            })?;

            if secret.is_empty() {
                Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "empty string is not allowed in `secrets`"
                ))
            } else {
                Ok(vec![secret])
            }
        }
        Node::Sequence(seq) => {
            let mut secrets = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => secrets.extend(parse_secrets(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            secrets.extend(parse_secrets(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(secrets)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_passthrough(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let passthrough = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `passthrough`"
                )
            })?;

            if passthrough.is_empty() {
                Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "empty string is not allowed in `passthrough`"
                ))
            } else {
                Ok(vec![passthrough])
            }
        }
        Node::Sequence(seq) => {
            let mut passthrough = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        passthrough.extend(parse_passthrough(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            passthrough.extend(parse_passthrough(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(passthrough)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

/// Run exports are applied to downstream packages that depend on this package.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RunExports {
    /// Noarch run exports are the only ones looked at when building noarch packages.
    pub noarch: Vec<Dependency>,
    /// Strong run exports apply from the build and host env to the run env.
    pub strong: Vec<Dependency>,
    /// Strong run constrains add run_constrains from the build and host env.
    pub strong_constrains: Vec<Dependency>,
    /// Weak run exports apply from the host env to the run env.
    pub weak: Vec<Dependency>,
    /// Weak run constrains add run_constrains from the host env.
    pub weak_constrains: Vec<Dependency>,
}

impl RunExports {
    fn from_node(node: &Node, jinja: &Jinja) -> Result<RunExports, PartialParsingError> {
        let mut run_exports = RunExports::default();

        match node {
            Node::Scalar(_) | Node::Sequence(_) => {
                let deps = parse_dependency(node, jinja)?;
                run_exports.strong = deps;
            }
            Node::Mapping(map) => {
                for (key, value) in map.iter() {
                    match key.as_str() {
                        "noarch" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.noarch = deps;
                        }
                        "strong" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.strong = deps;
                        }
                        "strong_constrains" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.strong_constrains = deps;
                        }
                        "weak" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.weak = deps;
                        }
                        "weak_constrains" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.weak_constrains = deps;
                        }
                        _ => unreachable!("invalid field: {}", key.as_str()),
                    }
                }
            }
        }
        Ok(run_exports)
    }

    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constrains.is_empty()
            && self.weak.is_empty()
            && self.weak_constrains.is_empty()
    }

    /// Get the noarch run exports.
    pub fn noarch(&self) -> &[Dependency] {
        self.noarch.as_slice()
    }

    /// Get the strong run exports.
    pub fn strong(&self) -> &[Dependency] {
        self.strong.as_slice()
    }

    /// Get the strong run constrains.
    pub fn strong_constrains(&self) -> &[Dependency] {
        self.strong_constrains.as_slice()
    }

    /// Get the weak run exports.
    pub fn weak(&self) -> &[Dependency] {
        self.weak.as_slice()
    }

    /// Get the weak run constrains.
    pub fn weak_constrains(&self) -> &[Dependency] {
        self.weak_constrains.as_slice()
    }
}

fn parse_dependency(node: &Node, jinja: &Jinja) -> Result<Vec<Dependency>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let dep = Dependency::from_scalar(s, jinja)?;
            Ok(vec![dep])
        }
        Node::Sequence(seq) => {
            let mut deps = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => deps.extend(parse_dependency(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            deps.extend(parse_dependency(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(deps)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

/// Define tests in your recipe that are executed after successfully building the package.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Test {
    /// Try importing a python module as a sanity check
    imports: Vec<String>,
    /// Run a list of given commands
    commands: Vec<String>,
    /// Extra requirements to be installed at test time
    requires: Vec<String>,
    /// Extra files to be copied to the test environment from the source dir (can be globs)
    source_files: Vec<String>,
    /// Extra files to be copied to the test environment from the build dir (can be globs)
    files: Vec<String>,
}

impl Test {
    fn from_stage1(test: &stage1::Test, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        Ok(test
            .node
            .as_ref()
            .map(|node| Self::from_node(node, jinja))
            .transpose()?
            .unwrap_or_default())
    }

    fn from_node(node: &Node, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        /// Parse a [`Node`] that can be or a scalar, sequence of scalar or a conditional that results in scalar.
        fn parse(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
            match node {
                Node::Scalar(s) => {
                    let imports = jinja.render_str(s.as_str()).map_err(|err| {
                        _partialerror!(
                            *s.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering `imports`"
                        )
                    })?;
                    Ok(vec![imports])
                }
                Node::Sequence(seq) => {
                    let mut imports = Vec::new();
                    for inner in seq.iter() {
                        match inner {
                            SequenceNodeInternal::Simple(n) => imports.extend(parse(n, jinja)?),
                            SequenceNodeInternal::Conditional(if_sel) => {
                                let if_res = if_sel.process(jinja)?;
                                if let Some(if_res) = if_res {
                                    imports.extend(parse(&if_res, jinja)?)
                                }
                            }
                        }
                    }
                    Ok(imports)
                }
                Node::Mapping(_) => Err(_partialerror!(
                    *node.span(),
                    ErrorKind::Other,
                    label = "expected scalar or sequence"
                )),
            }
        }

        match node {
            Node::Mapping(map) => {
                let mut test = Self::default();

                for (key, value) in map.iter() {
                    match key.as_str() {
                        "imports" => test.imports = parse(value, jinja)?,
                        "commands" => test.commands = parse(value, jinja)?,
                        "requires" => test.requires = parse(value, jinja)?,
                        "source_files" => test.source_files = parse(value, jinja)?,
                        "files" => test.files = parse(value, jinja)?,
                        _ => unreachable!("unimplemented field: {}", key.as_str()),
                    }
                }

                Ok(test)
            }
            Node::Sequence(_) => todo!("Unimplemented: Sequence on Test"),
            Node::Scalar(_) => Err(_partialerror!(
                *node.span(),
                ErrorKind::Other,
                label = "expected mapping"
            )),
        }
    }

    /// Get the imports.
    pub fn imports(&self) -> &[String] {
        self.imports.as_slice()
    }

    /// Get the commands.
    pub fn commands(&self) -> &[String] {
        self.commands.as_slice()
    }

    /// Get the requires.
    pub fn requires(&self) -> &[String] {
        self.requires.as_slice()
    }

    /// Get the source files.
    pub fn source_files(&self) -> &[String] {
        self.source_files.as_slice()
    }

    /// Get the files.
    pub fn files(&self) -> &[String] {
        self.files.as_slice()
    }

    /// Check if there is not test commands to be run
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let recipe = include_str!("stage1/testfiles/xtensor_recipe.yaml");
        let recipe = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap();
        dbg!(&recipe);
    }
}
