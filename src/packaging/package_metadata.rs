use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PathRecord {
    #[serde(rename = "_path")]
    pub path: PathBuf,
    pub path_type : String,
    pub sha256: String,
    pub size_in_bytes: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Paths {
    pub paths : Vec<PathRecord>
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            paths : Vec::new()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MetaIndex {
    // #[serde(serialize_with = "sort_alphabetically")]
    pub name: String,
    pub version: String,
    pub build: String,
    pub build_number: u64,

    pub arch : String,
    pub subdir : String,
    pub platform : String,

    pub license : String,
    pub license_family : String,

    pub timestamp : u128,

    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub constrains: Vec<String>,
}
