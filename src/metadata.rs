use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize, Debug)]
pub struct BuildOptions {
    pub number: u32,
    pub string: Option<String>,
    pub script: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Recipe {
    pub context: BTreeMap<String, serde_yaml::Value>,
    pub name: String,
    pub version: String,
    pub source : Vec<Source>,
    #[serde(default)]
    pub build: BuildOptions,
    #[serde(default)]
    pub requirements: Requirements,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            number: 0,
            string: Default::default(),
            script: String::from("build.sh"),
        }
    }
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
pub enum Checksum {
    sha256(String),
    md5(String)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GitSrc {
    pub git_src : String,
    pub git_rev : String,
    pub git_depth : u32
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UrlSrc {
    pub url : String,

    #[serde(flatten)]
    pub checksum : Checksum
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Source {
    Git(GitSrc),
    Url(UrlSrc)
}

pub struct Output {
    pub build: BuildOptions,
    pub name: String,
    pub version: String,
    pub source : Vec<Source>,
    pub requirements: Requirements,
}
