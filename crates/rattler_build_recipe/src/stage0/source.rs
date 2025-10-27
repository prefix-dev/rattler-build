use rattler_digest::{Md5Hash, Sha256Hash};
use serde::{Deserialize, Serialize};
use serde_with::{OneOrMany, formats::PreferMany, serde_as};
use std::path::PathBuf;

use crate::stage0::types::{ConditionalList, IncludeExclude, Value};

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
        Self::Value(Value::new_concrete("HEAD".to_string(), None))
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
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    /// Url(s) to the source code (template or concrete)
    /// Can be a single URL or a list of URLs (for mirrors)
    #[serde_as(as = "OneOrMany<_, PreferMany>")]
    #[serde(default)]
    pub url: Vec<Value<String>>,

    /// Optionally a sha256 checksum to verify the downloaded file
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "sha256_serialization::serialize"
    )]
    pub sha256: Option<Value<Sha256Hash>>,

    /// Optionally a md5 checksum to verify the downloaded file
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "md5_serialization::serialize"
    )]
    pub md5: Option<Value<Md5Hash>>,

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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "sha256_serialization::serialize"
    )]
    pub sha256: Option<Value<Sha256Hash>>,

    /// Optionally a md5 checksum to verify the source code
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "md5_serialization::serialize"
    )]
    pub md5: Option<Value<Md5Hash>>,

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

    /// Filter for files to include/exclude from the source
    #[serde(default)]
    pub filter: IncludeExclude,
}

fn default_use_gitignore() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// Serialize a SHA256 hash Value as a hex string
mod sha256_serialization {
    use super::*;
    use serde::Serializer;

    pub fn serialize<S>(value: &Option<Value<Sha256Hash>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(v) if v.is_concrete() => {
                serializer.serialize_str(&format!("{:x}", v.as_concrete().unwrap()))
            }
            Some(v) if v.is_template() => {
                serializer.serialize_str(v.as_template().unwrap().source())
            }
            _ => unreachable!("Value must be either concrete or template"),
        }
    }
}

/// Serialize an MD5 hash Value as a hex string
mod md5_serialization {
    use super::*;
    use serde::Serializer;

    pub fn serialize<S>(value: &Option<Value<Md5Hash>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            None => serializer.serialize_none(),
            Some(v) if v.is_concrete() => {
                serializer.serialize_str(&format!("{:x}", v.as_concrete().unwrap()))
            }
            Some(v) if v.is_template() => {
                serializer.serialize_str(v.as_template().unwrap().source())
            }
            _ => unreachable!("Value must be either concrete or template"),
        }
    }
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
        let GitSource {
            url,
            rev,
            tag,
            branch,
            depth,
            patches,
            target_directory,
            lfs,
        } = self;

        let mut vars = Vec::new();
        vars.extend(url.0.used_variables());
        if let Some(GitRev::Value(v)) = rev {
            vars.extend(v.used_variables());
        }
        if let Some(GitRev::Value(v)) = tag {
            vars.extend(v.used_variables());
        }
        if let Some(GitRev::Value(v)) = branch {
            vars.extend(v.used_variables());
        }
        if let Some(depth) = depth {
            vars.extend(depth.used_variables());
        }
        // Extract variables from patches
        for item in patches {
            if let crate::stage0::types::Item::Value(v) = item {
                vars.extend(v.used_variables());
            }
        }
        if let Some(td) = target_directory {
            if let Some(t) = td.as_template() {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        if let Some(lfs) = lfs {
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
        let UrlSource {
            url,
            sha256,
            md5,
            file_name,
            patches,
            target_directory,
        } = self;

        let mut vars = Vec::new();
        for url in url {
            vars.extend(url.used_variables());
        }
        if let Some(sha256) = sha256 {
            vars.extend(sha256.used_variables());
        }
        if let Some(md5) = md5 {
            vars.extend(md5.used_variables());
        }
        if let Some(file_name) = file_name {
            vars.extend(file_name.used_variables());
        }
        // Extract variables from patches
        for item in patches {
            if let crate::stage0::types::Item::Value(v) = item {
                vars.extend(v.used_variables());
            }
        }
        if let Some(td) = target_directory {
            if let Some(t) = td.as_template() {
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
        let PathSource {
            path,
            sha256,
            md5,
            patches: _,
            target_directory,
            file_name,
            use_gitignore: _,
            filter,
        } = self;

        let mut vars = Vec::new();
        if let Some(t) = path.as_template() {
            vars.extend(t.used_variables().iter().cloned());
        }
        if let Some(sha256) = sha256 {
            vars.extend(sha256.used_variables());
        }
        if let Some(md5) = md5 {
            vars.extend(md5.used_variables());
        }
        // Skip patches as PathBuf doesn't easily support template extraction
        if let Some(td) = target_directory {
            if let Some(t) = td.as_template() {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        if let Some(fn_val) = file_name {
            if let Some(t) = fn_val.as_template() {
                vars.extend(t.used_variables().iter().cloned());
            }
        }
        vars.extend(filter.used_variables());
        vars.sort();
        vars.dedup();
        vars
    }
}
