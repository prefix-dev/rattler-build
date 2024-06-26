use std::{fmt, path::PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

#[derive(Default, Debug, Serialize)]
pub struct SourceElement {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct Build {
    pub script: String,
    #[serde(skip_serializing_if = "Python::is_default")]
    pub python: Python,
}

#[derive(Default, Debug, Serialize)]
pub struct Python {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<String>,
}

impl Python {
    fn is_default(&self) -> bool {
        self.entry_points.is_empty()
    }
}

#[derive(Default, Debug, Serialize)]
pub struct About {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct Package {
    pub name: String,
    pub version: String,
}

#[derive(Default, Debug, Serialize)]
pub struct ScriptTest {
    pub script: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Test {
    Script(ScriptTest),
}

#[derive(Default, Debug, Serialize)]
pub struct Recipe {
    pub context: IndexMap<String, String>,
    pub package: Package,
    pub source: Vec<SourceElement>,
    pub build: Build,
    pub requirements: Requirements,
    pub tests: Vec<Test>,
    pub about: About,
}

#[derive(Default, Debug, Serialize)]
pub struct Requirements {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,
}

impl fmt::Display for Recipe {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let string = serde_yaml::to_string(self).unwrap();
        // add a newline before every top-level key
        let lines = string.split('\n').collect::<Vec<&str>>();
        let mut first_line = true;
        for line in lines {
            if line.chars().next().map(|c| c.is_alphabetic()) == Some(true) && !first_line {
                writeln!(f)?;
            }
            first_line = false;
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

/// Write a recipe to "{package_name}/recipe.yaml"
pub fn write_recipe(package_name: &str, recipe: &str) -> std::io::Result<()> {
    let path = PathBuf::from(&format!("{}/recipe.yaml", &package_name));
    fs_err::create_dir_all(path.parent().unwrap())?;

    if path.exists() {
        // move to backup
        let backup_path = path.with_extension("yaml.bak");
        fs_err::rename(&path, backup_path)?;
    }

    println!("Writing recipe to {}", path.display());

    fs_err::write(path, recipe)
}
