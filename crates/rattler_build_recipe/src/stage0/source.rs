use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::stage0::types::{ConditionalList, Value};

/// Source information - can be Git, Url, or Path
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

/// A git revision (branch, tag or commit)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GitRev {
    /// A git branch, tag, or commit (template or concrete)
    Value(Value<String>),
}

impl Default for GitRev {
    fn default() -> Self {
        Self::Value(Value::Concrete("HEAD".to_string()))
    }
}

/// A Git repository URL (can be template or concrete)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitUrl(pub Value<String>);

/// Git source information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitSource {
    /// Url to the git repository (template or concrete)
    #[serde(rename = "git")]
    pub url: GitUrl,

    /// Optionally a revision to checkout (can specify as rev, tag, or branch)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<GitRev>,

    /// Optionally a tag to checkout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<GitRev>,

    /// Optionally a branch to checkout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<GitRev>,

    /// Optionally a depth to clone the repository
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<Value<i32>>,

    /// Optionally patches to apply to the source code
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub patches: ConditionalList<String>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<Value<PathBuf>>,

    /// Optionally request the lfs pull in git source
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lfs: Option<Value<bool>>,
}

/// A url source (usually a tar.gz or tar.bz2 archive)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    /// Url to the source code (template or concrete, can be list)
    #[serde(default)]
    pub url: Vec<Value<String>>,

    /// Optionally a sha256 checksum to verify the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Value<String>>,

    /// Optionally a md5 checksum to verify the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<Value<String>>,

    /// Optionally a file name to rename the downloaded file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<Value<String>>,

    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub patches: ConditionalList<String>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<Value<PathBuf>>,
}

/// A local path source
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSource {
    /// Path to the local source code (template or concrete)
    pub path: Value<PathBuf>,

    /// Optionally a sha256 checksum to verify the source code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Value<String>>,

    /// Optionally a md5 checksum to verify the source code
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<Value<String>>,

    /// Patches to apply to the source code
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub patches: ConditionalList<String>,

    /// Optionally a folder name under the `work` directory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_directory: Option<Value<PathBuf>>,

    /// Optionally a file name to rename the file to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<Value<PathBuf>>,

    /// Whether to use the `.gitignore` file in the source directory
    #[serde(default = "default_use_gitignore", skip_serializing_if = "is_true")]
    pub use_gitignore: bool,

    /// Only take certain files from the source (glob patterns)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub filter: ConditionalList<String>,
}

fn default_use_gitignore() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Source {
    /// Collect all variables used in this source
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Source::Git(git) => git.used_variables(),
            Source::Url(url) => url.used_variables(),
            Source::Path(path) => path.used_variables(),
        }
    }
}

impl GitSource {
    /// Collect all variables used in the git source
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        vars.extend(self.url.0.used_variables());
        if let Some(GitRev::Value(v)) = &self.rev {
            vars.extend(v.used_variables());
        }
        if let Some(GitRev::Value(v)) = &self.tag {
            vars.extend(v.used_variables());
        }
        if let Some(GitRev::Value(v)) = &self.branch {
            vars.extend(v.used_variables());
        }
        if let Some(depth) = &self.depth {
            vars.extend(depth.used_variables());
        }
        // Extract variables from patches
        for item in &self.patches {
            if let crate::stage0::types::Item::Value(v) = item {
                vars.extend(v.used_variables());
            }
        }
        if let Some(target_dir) = &self.target_directory {
            if let Value::Template(t) = target_dir {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        if let Some(lfs) = &self.lfs {
            vars.extend(lfs.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }
}

impl UrlSource {
    /// Collect all variables used in the url source
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for url in &self.url {
            vars.extend(url.used_variables());
        }
        if let Some(sha256) = &self.sha256 {
            vars.extend(sha256.used_variables());
        }
        if let Some(md5) = &self.md5 {
            vars.extend(md5.used_variables());
        }
        if let Some(file_name) = &self.file_name {
            vars.extend(file_name.used_variables());
        }
        // Extract variables from patches
        for item in &self.patches {
            if let crate::stage0::types::Item::Value(v) = item {
                vars.extend(v.used_variables());
            }
        }
        if let Some(target_dir) = &self.target_directory {
            if let Value::Template(t) = target_dir {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}

impl PathSource {
    /// Collect all variables used in the path source
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        if let Value::Template(t) = &self.path {
            vars.extend(t.used_variables().iter().cloned());
        }
        if let Some(sha256) = &self.sha256 {
            vars.extend(sha256.used_variables());
        }
        if let Some(md5) = &self.md5 {
            vars.extend(md5.used_variables());
        }
        // Skip patches as PathBuf doesn't easily support template extraction
        if let Some(target_dir) = &self.target_directory {
            if let Value::Template(t) = target_dir {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        if let Some(file_name) = &self.file_name {
            if let Value::Template(t) = file_name {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        for item in &self.filter {
            if let crate::stage0::types::Item::Value(v) = item {
                vars.extend(v.used_variables());
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
