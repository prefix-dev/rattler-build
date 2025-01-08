use async_once_cell::OnceCell;
use clap::Parser;
use miette::{IntoDiagnostic, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use super::write_recipe;
use crate::recipe_generator::serialize;

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

#[derive(Deserialize)]
struct PyPiRelease {
    filename: String,
    url: String,
    digests: HashMap<String, String>,
}

#[derive(Deserialize)]
struct PyPiInfo {
    name: String,
    version: String,
    summary: Option<String>,
    description: Option<String>,
    home_page: Option<String>,
    license: Option<String>,
    requires_dist: Option<Vec<String>>,
    project_urls: Option<HashMap<String, String>>,
    requires_python: Option<String>,
}

#[derive(Deserialize)]
struct PyPiResponse {
    info: PyPiInfo,
    releases: HashMap<String, Vec<PyPiRelease>>,
}

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
        Ok(mapping.into_iter().map(|m| (m.conda_name, m.pypi_name)).collect())
    }).await
}

fn format_requirement(req: &str) -> String {
    // Add space before version specifiers
    let mut req = req.to_string();
    // Find first non-alphanumeric/.-_ character and insert a space
    if let Some(pos) = req.find(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_')
    {
        req.insert(pos, ' ');
    }

    if req.contains(';') {
        let (package, marker) = req.split_once(';').unwrap();
        format!("{} ;MARKER; {}", package.trim(), marker.trim())
    } else {
        req.trim().to_string()
    }
}

fn post_process_markers(recipe_yaml: String) -> String {
    let mut result = Vec::new();
    for line in recipe_yaml.lines() {
        if line.contains(";MARKER;") {
            let mut l = line.replacen("- ", "# - ", 1);
            l = l.replace(";MARKER;", "#");
            result.push(l);
        } else {
            result.push(line.to_string());
        }
    }
    result.join("\n")
}

#[async_recursion::async_recursion]
pub async fn generate_pypi_recipe(opts: &PyPIOpts) -> miette::Result<()> {
    eprintln!("Generating recipe for {}", opts.package);

    let package = &opts.package;
    let client = reqwest::Client::new();

    // Fetch package metadata from PyPI JSON API
    let url = format!("https://pypi.org/pypi/{}/json", package);
    let response: PyPiResponse = client
        .get(&url)
        .send()
        .await
        .into_diagnostic()?
        .json()
        .await
        .into_diagnostic()?;

    let mut recipe = serialize::Recipe::default();
    recipe.package.name = response.info.name;
    recipe.package.version = response.info.version.clone();

    // Get the latest release
    let latest_release = response
        .releases
        .get(&response.info.version)
        .and_then(|releases| releases.iter().find(|r| r.filename.ends_with(".tar.gz")))
        .ok_or_else(|| miette::miette!("No source distribution found"))?;

    recipe.source.push(serialize::SourceElement {
        url: latest_release.url.clone(),
        sha256: latest_release.digests.get("sha256").cloned(),
        md5: None,
    });

    // Set Python requirements
    if let Some(python_req) = response.info.requires_python {
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

    // Process dependencies
    let mut requirements = Vec::new();
    if let Some(deps) = response.info.requires_dist {
        for req in deps {
            let conda_name = if opts.use_mapping {
                let mapping = conda_pypi_name_mapping().await?;
                // Get base package name without markers/version
                let base_name = req.split([' ', ';']).next().unwrap();
                mapping.get(base_name).map_or(req.clone(), |n| {
                    // Replace the package name but keep version and markers
                    req.replacen(base_name, n, 1)
                })
            } else {
                req
            };
            let formatted_req = format_requirement(&conda_name);
            recipe
                .requirements
                .run
                .push(formatted_req.trim_start_matches("- ").to_string());
            requirements.push(conda_name);
        }
    }

    recipe.build.script = "python -m pip install .".to_string();

    // Set metadata
    recipe.about.summary = response.info.summary;
    recipe.about.description = response.info.description;
    recipe.about.homepage = response.info.home_page;
    recipe.about.license = response.info.license;

    if let Some(urls) = response.info.project_urls {
        recipe.about.repository = urls.get("Source Code").cloned();
        recipe.about.documentation = urls.get("Documentation").cloned();
    }

    let string = format!("{}", recipe);
    let string = post_process_markers(string);
    if opts.write {
        write_recipe(package, &string).into_diagnostic()?;
    } else {
        print!("{}", string);
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
