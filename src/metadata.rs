use serde::{Deserialize, Serialize};
use serde_with::formats::PreferOne;
use serde_with::serde_as;
use serde_with::OneOrMany;
use std::collections::BTreeMap;
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
pub struct BuildOptions {
    pub number: u64,
    pub string: Option<String>,
    pub script: Option<String>,
    pub ignore_run_exports: Option<Vec<String>>,
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
pub struct GitSrc {
    pub git_src: String,
    pub git_rev: String,
    pub git_depth: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UrlSrc {
    pub url: String,

    #[serde(flatten)]
    pub checksum: Checksum,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Source {
    Git(GitSrc),
    Url(UrlSrc),
}

pub struct BuildConfiguration {
    pub target_platform: String,
    pub build_platform: String,
    pub used_vars: Vec<String>,
    pub hash: String,
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
