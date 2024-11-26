use async_once_cell::OnceCell;
use clap::Parser;
use miette::{IntoDiagnostic, WrapErr};
use rattler_installs_packages::index::CheckAvailablePackages;
use rattler_installs_packages::python_env::Pep508EnvMakers;
use rattler_installs_packages::resolve::solve_options::ResolveOptions;
use rattler_installs_packages::types::{ArtifactFromSource, Requirement, VersionOrUrl};
use rattler_installs_packages::wheel_builder::WheelBuilder;
use rattler_installs_packages::{
    artifacts::SDist,
    index::{ArtifactRequest, PackageDb, PackageSources},
    types::{ArtifactName, NormalizedPackageName},
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{collections::HashMap, str::FromStr};
use tokio::io::AsyncWriteExt;

use crate::recipe_generator::serialize;

use super::write_recipe;

#[derive(Deserialize)]
struct CondaPyPiNameMapping {
    conda_name: String,
    pypi_name: String,
}

#[derive(Debug, Clone, Parser)]
pub struct PyPIOpts {
    /// Name of the package to generate
    pub package: String,

    /// Whether to write the recipe to a folder
    #[arg(short, long)]
    pub write: bool,

    /// Whether to use the conda-forge PyPI name mapping
    #[arg(short, long, default_value = "true")]
    pub use_mapping: bool,

    /// Whether to generate recipes for all dependencies
    #[arg(short, long)]
    pub tree: bool,
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

async fn pypi_requirement(req: &Requirement, use_mapping: bool) -> miette::Result<String> {
    let mut res = req.name.clone().to_lowercase();

    // check if the name is in the conda-forge pypi name mapping
    if use_mapping {
        let mapping = conda_pypi_name_mapping()
            .await
            .wrap_err("failed to get conda-pypi name mapping")?;

        if let Some(conda_name) = mapping.get(&req.name) {
            // reinit the result with the mapped conda name
            res = conda_name.clone();
        }
    }

    if let Some(VersionOrUrl::VersionSpecifier(version)) = &req.version_or_url {
        res.push_str(&format!(" {}", version));
    }

    if let Some(markers) = &req.marker {
        res.push_str(&format!("  MARKER {}", markers));
    }
    Ok(res)
}

#[async_recursion::async_recursion]
pub async fn generate_pypi_recipe(opts: &PyPIOpts) -> miette::Result<()> {
    eprintln!("Generating recipe for {}", opts.package);

    let package = &opts.package;
    let client = reqwest::Client::new();
    let client_with_middlewares = reqwest_middleware::ClientBuilder::new(client).build();
    let package_sources =
        PackageSources::from(url::Url::parse("https://pypi.org/simple/").unwrap());
    let tempdir = tempfile::tempdir().into_diagnostic()?;
    let artifact_request = ArtifactRequest::FromIndex(NormalizedPackageName::from_str(package)?);

    // keep tempdir
    let tempdir_path = tempdir.into_path();
    let package_db = Arc::new(PackageDb::new(
        package_sources,
        client_with_middlewares,
        &tempdir_path.join("pkg-db"),
        CheckAvailablePackages::Always,
    )?);
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

    let env_markers = Arc::new(Pep508EnvMakers::from_env().await.unwrap().0);
    let wheel_builder = WheelBuilder::new(
        package_db.clone(),
        env_markers,
        None,
        ResolveOptions::default(),
    )
    .unwrap();

    let metadata = package_db
        .get_metadata(first_artifact.1, Some(&wheel_builder))
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

    // Download the sdist
    let filename = source_dist.url.to_string();
    let filename = filename.split('/').last().unwrap();
    // split off everything after the #
    let filename = filename
        .split_once('#')
        .map(|(fname, _)| fname)
        .unwrap_or(filename);

    let sdist_path = tempdir_path.join(filename);
    download_sdist(&source_dist.url, &sdist_path)
        .await
        .wrap_err("failed to download sdist")?;

    // get the metadata
    let wheel_metadata = metadata.1;
    let sdist = SDist::from_path(
        &sdist_path,
        &NormalizedPackageName::from(wheel_metadata.name),
    )
    .unwrap();

    let pyproject_toml = sdist.read_pyproject_toml().ok();
    let (_, mut pkg_info) = sdist.read_package_info().into_diagnostic()?;

    if let Some(pyproject_toml) = pyproject_toml {
        if let Some(build_system) = pyproject_toml.build_system {
            for req in build_system.requires {
                recipe
                    .requirements
                    .host
                    .push(pypi_requirement(&req, opts.use_mapping).await?);
            }
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
    }

    if let Some(python_req) = wheel_metadata.requires_python.as_ref() {
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

    let mut requirements = Vec::new();
    for pkg in wheel_metadata.requires_dist {
        let conda_name = pypi_requirement(&pkg, opts.use_mapping).await?;
        recipe.requirements.run.push(conda_name.clone());
        requirements.push(conda_name);
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

    if opts.write {
        write_recipe(package, &res).into_diagnostic()?;
    } else {
        print!("{}", res);
    }

    if opts.tree {
        for dep in requirements {
            let dep = dep.split_whitespace().next().unwrap();
            if !PathBuf::from(dep).exists() {
                let opts = PyPIOpts {
                    package: dep.to_string(),
                    ..opts.clone()
                };
                generate_pypi_recipe(&opts).await?;
            }
        }
    }

    Ok(())
}
