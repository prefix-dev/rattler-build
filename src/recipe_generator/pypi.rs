use async_once_cell::OnceCell;
use miette::{IntoDiagnostic, WrapErr};
use rattler_installs_packages::types::{ArtifactFromSource, Requirement, VersionOrUrl};
use rattler_installs_packages::{
    artifacts::SDist,
    index::{ArtifactRequest, PackageDb, PackageSources},
    types::{ArtifactName, NormalizedPackageName},
};
use serde::Deserialize;
use std::path::Path;
use std::{collections::HashMap, str::FromStr};
use tokio::io::AsyncWriteExt;

use crate::recipe_generator::serialize;

#[derive(Deserialize)]
struct CondaPyPiNameMapping {
    conda_name: String,
    pypi_name: String,
}

/// Downloads and caches the conda-forge conda-to-pypi name mapping.
pub async fn conda_pypi_name_mapping() -> miette::Result<&'static HashMap<String, String>> {
    static MAPPING: OnceCell<HashMap<String, String>> = OnceCell::new();
    MAPPING.get_or_try_init(async {
        let response = reqwest::get("https://raw.githubusercontent.com/regro/cf-graph-countyfair/master/mappings/pypi/name_mapping.json").await
            .into_diagnostic()
            .context("failed to download pypi name mapping")?;
        let mapping: Vec<CondaPyPiNameMapping> = response
            .json()
            .await
            .into_diagnostic()
            .context("failed to parse pypi name mapping")?;
        let mapping_by_name: HashMap<_, _> = mapping
            .into_iter()
            .map(|m| (m.conda_name, m.pypi_name))
            .collect();
        Ok(mapping_by_name)
    }).await
}

async fn download_sdist(url: &url::Url, dest: &Path) -> miette::Result<()> {
    let response = reqwest::get(url.clone()).await.into_diagnostic()?;

    let mut file = tokio::fs::File::create(&dest).await.into_diagnostic()?;

    let bytes = response.bytes().await.into_diagnostic()?;
    file.write_all(&bytes).await.into_diagnostic()?;

    Ok(())
}

async fn pypi_requirement(req: &Requirement) -> miette::Result<String> {
    let mut res = req.name.clone().to_lowercase();

    // check if the name is in the conda-forge pypi name mapping
    let mapping = conda_pypi_name_mapping()
        .await
        .wrap_err("failed to get conda-pypi name mapping")?;
    if let Some(conda_name) = mapping.get(&req.name) {
        res = conda_name.clone();
    }

    if let Some(VersionOrUrl::VersionSpecifier(version)) = &req.version_or_url {
        res.push_str(&format!(" {}", version));
    }

    if let Some(markers) = &req.marker {
        res.push_str(&format!("  MARKER {}", markers));
    }
    Ok(res)
}

pub async fn generate_pypi_recipe(package: &str) -> miette::Result<()> {
    let client = reqwest::Client::new();
    let client_with_middlewares = reqwest_middleware::ClientBuilder::new(client).build();
    let package_sources =
        PackageSources::from(url::Url::parse("https://pypi.org/simple/").unwrap());
    let tempdir = tempfile::tempdir().into_diagnostic()?;
    let artifact_request = ArtifactRequest::FromIndex(NormalizedPackageName::from_str(package)?);
    let package_db = PackageDb::new(package_sources, client_with_middlewares, tempdir.path())?;
    let artifacts = package_db.available_artifacts(artifact_request).await?;
    // find first artifact or bail out
    let first_artifact = artifacts
        .into_iter()
        .next()
        .ok_or_else(|| miette::miette!("No package artifacts found for {}", package))?;

    let source_dist = first_artifact
        .1
        .iter()
        .find(|artifact| matches!(artifact.filename, ArtifactName::SDist(_)))
        .ok_or_else(|| miette::miette!("No source distribution found for {}", package))?;

    let metadata = package_db
        .get_metadata(first_artifact.1, None)
        .await?
        .ok_or_else(|| miette::miette!("No metadata found for {}", package))?;

    let mut recipe = serialize::Recipe::default();
    recipe.package.name = metadata.1.name.as_str().to_string();
    recipe.package.version = metadata.1.version.to_string();

    let hash_string = source_dist
        .hashes
        .as_ref()
        .and_then(|h| h.sha256)
        .map(|h| format!("{:x}", h));

    let url = source_dist.url.to_string();
    let url = url.split_once('#').map(|(url, _)| url).unwrap_or(&url);

    recipe.source.push(serialize::SourceElement {
        url: url.to_string(),
        sha256: hash_string,
        md5: None,
    });

    // find package build time deps...
    let tempdir = tempfile::tempdir().into_diagnostic()?;

    // downlaod the sdist
    let filename = source_dist.url.to_string();
    let filename = filename.split('/').last().unwrap();
    // split off everything after the #
    let filename = filename
        .split_once('#')
        .map(|(fname, _)| fname)
        .unwrap_or(filename);

    let sdist_path = tempdir.path().join(filename);
    download_sdist(&source_dist.url, &sdist_path)
        .await
        .wrap_err("failed to download sdist")?;

    let sdist =
        SDist::from_path(&sdist_path, &NormalizedPackageName::from(metadata.1.name)).unwrap();

    let pyproject_toml = sdist.read_pyproject_toml().into_diagnostic()?;
    let (_, mut pkg_info) = sdist.read_package_info().into_diagnostic()?;

    println!("{:?}", pyproject_toml);

    for req in pyproject_toml.build_system.unwrap().requires {
        recipe.requirements.host.push(pypi_requirement(&req).await?);
    }
    if let Some(project) = pyproject_toml.project {
        if let Some(scripts) = project.scripts {
            recipe.build.python.entry_points = scripts
                .iter()
                .map(|(k, v)| format!("{} = {}", k, v))
                .collect();
        }
        // recipe.about.license_file = project.license_files.map(|p| p.join(", "));
        if let Some(urls) = project.urls {
            recipe.about.repository = urls.get("Source Code").map(|s| s.to_string());
            recipe.about.documentation = urls.get("Documentation").map(|s| s.to_string());
        }
    }

    if let Some(python_req) = metadata.1.requires_python.as_ref() {
        recipe
            .requirements
            .host
            .push(format!("python {}", python_req));
        recipe
            .requirements
            .run
            .push(format!("python {}", python_req));
    } else {
        recipe.requirements.host.push("python".to_string());
    }
    recipe.requirements.host.push("pip".to_string());

    for pkg in metadata.1.requires_dist {
        recipe.requirements.run.push(pypi_requirement(&pkg).await?);
    }

    recipe.build.script = "python -m pip install .".to_string();

    recipe.about.summary = pkg_info.parsed.take("Summary").ok();
    recipe.about.description = pkg_info.parsed.take("Description").ok();
    recipe.about.homepage = pkg_info.parsed.take("Home-page").ok();
    recipe.about.license = pkg_info.parsed.take("License").ok();

    let string = format!("{}", recipe);

    // find lines with MARKER on them and replace MARKER with # as well as adding a # in front
    let lines = string.split('\n').collect::<Vec<&str>>();
    let mut res = String::new();
    for line in lines {
        if line.contains("MARKER") {
            res.push_str(line.replace("- ", "# - ").replace("MARKER", "#").as_str());
        } else {
            res.push_str(line);
        }
        res.push('\n');
    }

    print!("{}", res);

    Ok(())
}