use rattler_conda_types::package::EntryPoint;
use rattler_conda_types::NoArchType;
use serde::{Deserialize, Serialize};
use serde_with::formats::PreferOne;
use serde_with::serde_as;
use serde_with::OneOrMany;
use std::collections::BTreeMap;
use std::path::PathBuf;
use url::Url;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Requirements {
    #[serde(default)]
    pub build: Vec<String>,
    #[serde(default)]
    pub host: Vec<String>,
    #[serde(default)]
    pub run: Vec<String>,
    #[serde(default)]
    pub constrains: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct RunExports {
    #[serde(default)]
    pub strong: Vec<String>,
    #[serde(default)]
    pub weak: Vec<String>,
    #[serde(default)]
    pub weak_constrains: Vec<String>,
    #[serde(default)]
    pub strong_constrains: Vec<String>,
    #[serde(default)]
    pub noarch: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct BuildOptions {
    pub number: u64,
    pub string: Option<String>,
    pub script: Option<String>,
    pub ignore_run_exports: Option<Vec<String>>,
    pub run_exports: Option<RunExports>,
    #[serde(default = "NoArchType::default")]
    pub noarch: NoArchType,
    #[serde(default)]
    pub entry_points: Vec<EntryPoint>,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug)]
pub struct About {
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub home: Option<Vec<Url>>,
    pub license: Option<String>,
    pub license_family: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub doc_url: Option<Vec<Url>>,
    #[serde_as(as = "Option<OneOrMany<_, PreferOne>>")]
    pub dev_url: Option<Vec<Url>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Recipe {
    pub context: BTreeMap<String, serde_yaml::Value>,
    pub name: String,
    pub version: String,
    pub source: Vec<Source>,
    #[serde(default)]
    pub build: BuildOptions,
    #[serde(default)]
    pub requirements: Requirements,
    pub about: About,
}

pub struct Metadata {
    pub name: String,
    pub version: String,
    pub requirements: Vec<String>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            name: String::from(""),
            version: String::from("0.0.0"),
            requirements: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Checksum {
    Sha256(String),
    Md5(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GitRev(String);

impl Default for GitRev {
    fn default() -> Self {
        Self(String::from("HEAD"))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GitSrc {
    pub git_src: Url,

    #[serde(default)]
    pub git_rev: GitRev,

    pub git_depth: Option<u32>,

    pub patches: Option<Vec<PathBuf>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UrlSrc {
    pub url: Url,

    #[serde(flatten)]
    pub checksum: Checksum,

    pub patches: Option<Vec<PathBuf>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Source {
    Git(GitSrc),
    Url(UrlSrc),
}

pub struct BuildConfiguration {
    pub target_platform: String,
    pub host_platform: String,
    pub build_platform: String,
    pub used_vars: Vec<String>,
    pub hash: String,
    pub no_clean: bool,
}

impl BuildConfiguration {
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform
    }
}

pub struct Output {
    pub build: BuildOptions,
    pub name: String,
    pub version: String,
    pub source: Vec<Source>,
    pub requirements: Requirements,
    pub about: About,
    pub build_configuration: BuildConfiguration,
}
