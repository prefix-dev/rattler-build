use std::path::Path;

use base64::{engine::general_purpose, Engine};
use miette::IntoDiagnostic;
use rattler_conda_types::{package::{AboutJson, IndexJson, PackageFile}, PackageName, Version as PackageVersion};
use rattler_digest::{compute_file_digest, Md5};
use sha2::Sha256;

pub struct Package<'a> {
    file: &'a Path,
    about_json: AboutJson,
    index_json: IndexJson,
}

impl<'a> Package<'a> {
    pub fn from_package_file(file: &'a Path) -> miette::Result<Self> {
        let extraction_dir = tempfile::tempdir().into_diagnostic()?;

        rattler_package_streaming::fs::extract(file, extraction_dir.path()).into_diagnostic()?;

        let index_json =
            IndexJson::from_package_directory(extraction_dir.path()).into_diagnostic()?;

        let about_json =
            AboutJson::from_package_directory(extraction_dir.path()).into_diagnostic()?;

        Ok(Self {
            file,
            about_json,
            index_json,
        })
    }

    pub fn path(&self) -> &Path {
        self.file
    }

    pub fn package_name(&self) -> &PackageName {
        &self.index_json.name
    }

    pub fn package_version(&self) -> &PackageVersion {
        &self.index_json.version
    }

    pub fn sha256(&self) -> Result<String, std::io::Error> {
        Ok(format!(
            "{:x}",
            compute_file_digest::<Sha256>(&self.path())?
        ))
    }

    pub fn base64_md5(&self) -> Result<String, std::io::Error> {
        compute_file_digest::<Md5>(&self.file)
            .map(|digest| general_purpose::STANDARD.encode(digest))
    }

    pub fn filename(&self) -> Option<&str> {
        self.file
            .file_name()
            .and_then(|s| s.to_str())
    }

    pub fn file_size(&self) -> Result<u64, std::io::Error> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
    }

    pub fn about_json(&self) -> &AboutJson {
        &self.about_json
    }

    pub fn index_json(&self) -> &IndexJson {
        &self.index_json
    }
}