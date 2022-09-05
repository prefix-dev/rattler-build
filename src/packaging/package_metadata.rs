use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PathRecord {
    pub sha256: String,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MetaIndex {
    pub name: String,
    pub version: String,
    pub build_string: String,
    pub build_number: u64,

    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub constrains: Vec<String>,
}
