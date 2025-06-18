use async_once_cell::OnceCell;
use clap::Parser;
use miette::{IntoDiagnostic, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Read as _};
use std::path::PathBuf;
use zip::ZipArchive;

use super::write_recipe;
use crate::recipe_generator::serialize::{
    self, PythonTest, PythonTestInner, Test, UrlSourceElement,
};

#[derive(Deserialize)]
struct CondaPyPiNameMapping {
    conda_name: String,
    pypi_name: String,
}

#[derive(Debug, Clone, Parser)]
pub struct PyPIOpts {
    /// Name of the package to generate
    pub package: String,

    /// Select a version of the package to generate (defaults to latest)
    #[arg(long)]
    pub version: Option<String>,

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

#[derive(Deserialize, Clone, Debug, Default)]
struct PyPiRelease {
    filename: String,
    url: String,
    digests: HashMap<String, String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
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

async fn extract_entry_points_from_wheel(
    url: &str,
    client: &reqwest::Client,
) -> miette::Result<Option<Vec<String>>> {
    // Download the wheel
    let wheel_data = client
        .get(url)
        .send()
        .await
        .into_diagnostic()?
        .bytes()
        .await
        .into_diagnostic()?;

    // Read wheel as zip
    let reader = Cursor::new(wheel_data);
    let mut archive = ZipArchive::new(reader).into_diagnostic()?;

    // Find entry_points.txt in any .dist-info directory
    let entry_points_file = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .map(|file| file.name().ends_with(".dist-info/entry_points.txt"))
            .unwrap_or(false)
    });

    if let Some(index) = entry_points_file {
        let mut file = archive.by_index(index).into_diagnostic()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).into_diagnostic()?;

        // Parse console_scripts section
        let console_scripts: Vec<String> = contents
            .lines()
            .skip_while(|l| !l.contains("[console_scripts]"))
            .skip(1) // Skip the [console_scripts] line
            .take_while(|l| !l.trim().is_empty() && !l.starts_with('['))
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .map(|s| {
                // make sure that there is a space around the `=` sign
                let (name, script) = s.split_once('=').unwrap();
                format!("{} = {}", name.trim(), script.trim())
            })
            .collect();

        if !console_scripts.is_empty() {
            return Ok(Some(console_scripts));
        }
    }

    Ok(None)
}

#[derive(Deserialize)]
struct PyPiResponse {
    info: PyPiInfo,
    releases: HashMap<String, Vec<PyPiRelease>>,
}

#[derive(Deserialize)]
struct PyPrReleaseResponse {
    info: PyPiInfo,
    urls: Vec<PyPiRelease>,
}

#[derive(Debug, Clone)]
pub struct PyPiMetadata {
    info: PyPiInfo,
    urls: Vec<PyPiRelease>,
    release: PyPiRelease,
    wheel_url: Option<String>,
}

async fn extract_build_requirements(
    url: &str,
    client: &reqwest::Client,
) -> miette::Result<Vec<String>> {
    let tar_data = client
        .get(url)
        .send()
        .await
        .into_diagnostic()?
        .bytes()
        .await
        .into_diagnostic()?;
    let tar = flate2::read::GzDecoder::new(&tar_data[..]);
    let mut archive = tar::Archive::new(tar);

    // Find and read pyproject.toml
    for entry in archive.entries().into_diagnostic()? {
        let mut entry = entry.into_diagnostic()?;
        if entry.path().into_diagnostic()?.ends_with("pyproject.toml") {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).into_diagnostic()?;

            // Parse TOML
            let toml: toml::Value = contents.parse().into_diagnostic()?;

            // Try different build system specs
            return Ok(match toml.get("build-system") {
                Some(build) => {
                    let reqs = build
                        .get("requires")
                        .and_then(|r| r.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    reqs
                }
                None => Vec::new(),
            });
        }
    }

    Ok(Vec::new())
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
    // Split package name from version specifiers
    let req = req.trim();
    let (name, version) = if let Some(pos) =
        req.find(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_')
    {
        (&req[..pos], &req[pos..])
    } else {
        (req, "")
    };

    // Handle markers separately
    if let Some((version, marker)) = version.split_once(';') {
        format!(
            "{} {} ;MARKER; {}",
            name.to_lowercase(),
            version.trim(),
            marker.trim()
        )
    } else {
        format!("{} {}", name.to_lowercase(), version.trim())
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

async fn is_noarch_python(urls: &[PyPiRelease]) -> bool {
    let wheels: Vec<_> = urls
        .iter()
        .filter(|r| r.filename.ends_with(".whl"))
        .collect();

    if wheels.is_empty() {
        // Conservative: if no wheels found, assume arch-specific
        return false;
    }

    // Check if all wheels are pure Python
    wheels
        .iter()
        .all(|wheel| wheel.filename.contains("-none-any.whl"))
}

async fn fetch_pypi_metadata(
    opts: &PyPIOpts,
    client: &reqwest::Client,
) -> miette::Result<PyPiMetadata> {
    let (info, urls) = if let Some(version) = &opts.version {
        let url = format!("https://pypi.org/pypi/{}/{}/json", opts.package, version);
        let release: PyPrReleaseResponse = client
            .get(&url)
            .send()
            .await
            .into_diagnostic()?
            .json()
            .await
            .into_diagnostic()?;
        (release.info, release.urls)
    } else {
        let url = format!("https://pypi.org/pypi/{}/json", opts.package);
        let response: PyPiResponse = client
            .get(&url)
            .send()
            .await
            .into_diagnostic()?
            .json()
            .await
            .into_diagnostic()?;

        // Get the latest release
        let urls = response
            .releases
            .get(&response.info.version)
            .ok_or_else(|| miette::miette!("No source distribution found"))?;
        (response.info, urls.clone())
    };

    let release = urls
        .iter()
        .find(|r| r.filename.ends_with(".tar.gz"))
        .ok_or_else(|| miette::miette!("No source distribution found"))?
        .clone();

    let wheel_url = urls
        .iter()
        .find(|r| r.filename.ends_with(".whl"))
        .map(|r| r.url.clone());

    Ok(PyPiMetadata {
        info,
        urls,
        release,
        wheel_url,
    })
}

async fn map_requirement(
    req: &str,
    mapping: &HashMap<String, String>,
    use_mapping: bool,
) -> String {
    if !use_mapping {
        return req.to_string();
    }
    // Get base package name without markers/version
    if let Some(base_name) = req.split([' ', ';']).next() {
        if let Some(mapped_name) = mapping.get(base_name) {
            // Replace the package name but keep version and markers
            return req.replacen(base_name, mapped_name, 1).to_string();
        }
    }
    req.to_string()
}

pub async fn create_recipe(
    opts: &PyPIOpts,
    metadata: &PyPiMetadata,
    client: &reqwest::Client,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();
    recipe
        .context
        .insert("version".to_string(), metadata.info.version.clone());
    recipe.package.name = metadata.info.name.to_lowercase();
    recipe.package.version = "${{ version }}".to_string();

    // replace URL with the shorter version that does not contain the hash
    let release_url = if metadata
        .release
        .url
        .starts_with("https://files.pythonhosted.org/")
    {
        let simple_url = format!(
            "https://pypi.org/packages/source/{}/{}/{}-{}.tar.gz",
            &metadata.info.name.to_lowercase()[..1],
            metadata.info.name.to_lowercase(),
            metadata.info.name.to_lowercase().replace("-", "_"),
            metadata.info.version
        );

        // Check if the simple URL exists
        if client.head(&simple_url).send().await.is_ok() {
            simple_url
        } else {
            metadata.release.url.clone()
        }
    } else {
        metadata.release.url.clone()
    };

    recipe.source.push(
        UrlSourceElement {
            url: vec![release_url.replace(metadata.info.version.as_str(), "${{ version }}")],
            sha256: metadata.release.digests.get("sha256").cloned(),
            md5: None,
        }
        .into(),
    );

    if let Some(wheel_url) = &metadata.wheel_url {
        if let Some(entry_points) = extract_entry_points_from_wheel(wheel_url, client).await? {
            recipe.build.python.entry_points = entry_points;
        }
    } else {
        tracing::warn!(
            "No wheel found for {} - cannot extract entry points.",
            opts.package
        );
    }

    // Check if package is noarch: python
    if is_noarch_python(&metadata.urls).await {
        recipe.build.noarch = Some("python".to_string());
    }

    // Set Python requirements
    if let Some(python_req) = &metadata.info.requires_python {
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

    let mapping = if opts.use_mapping {
        conda_pypi_name_mapping().await?
    } else {
        &HashMap::new()
    };

    // Check for build requirements
    let build_reqs = extract_build_requirements(&metadata.release.url, client).await?;
    if !build_reqs.is_empty() {
        for req in build_reqs {
            let mapped_req = map_requirement(&req, mapping, opts.use_mapping).await;
            recipe.requirements.host.push(mapped_req);
        }
    }
    recipe.requirements.host.push("pip".to_string());

    // Process runtime dependencies
    if let Some(deps) = &metadata.info.requires_dist {
        for req in deps {
            let mapped_req = map_requirement(req, mapping, opts.use_mapping).await;
            let formatted_req = format_requirement(&mapped_req);
            recipe
                .requirements
                .run
                .push(formatted_req.trim_start_matches("- ").to_string());
        }
    }

    recipe.build.script = "${{ PYTHON }} -m pip install .".to_string();

    recipe.tests.push(Test::Python(PythonTest {
        python: PythonTestInner {
            imports: vec![metadata.info.name.clone()],
            pip_check: true,
        },
    }));

    // Set metadata
    recipe.about.summary = metadata.info.summary.clone();
    recipe.about.description = metadata.info.description.clone();
    recipe.about.homepage = metadata.info.home_page.clone();
    recipe.about.license = metadata.info.license.clone();

    if let Some(urls) = &metadata.info.project_urls {
        recipe.about.repository = urls.get("Source Code").cloned();
        recipe.about.documentation = urls.get("Documentation").cloned();
    }

    Ok(recipe)
}

#[async_recursion::async_recursion]
pub async fn generate_pypi_recipe(opts: &PyPIOpts) -> miette::Result<()> {
    tracing::info!("Generating recipe for {}", opts.package);
    let client = reqwest::Client::new();

    let metadata = fetch_pypi_metadata(opts, &client).await?;
    let recipe = create_recipe(opts, &metadata, &client).await?;

    let string = format!("{}", recipe);
    let string = post_process_markers(string);

    if opts.write {
        write_recipe(&opts.package, &string).into_diagnostic()?;
    } else {
        print!("{}", string);
    }

    if opts.tree {
        for dep in metadata.info.requires_dist.unwrap_or_default() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_yaml_snapshot;

    #[tokio::test]
    async fn test_recipe_generation() {
        let opts = PyPIOpts {
            package: "numpy".into(),
            version: Some("1.24.0".into()),
            write: false,
            use_mapping: true,
            tree: false,
        };

        let client = reqwest::Client::new();
        let metadata = fetch_pypi_metadata(&opts, &client).await.unwrap();
        let recipe = create_recipe(&opts, &metadata, &client).await.unwrap();

        assert_yaml_snapshot!(recipe);
    }

    #[tokio::test]
    async fn test_flask_noarch_recipe_generation() {
        let opts = PyPIOpts {
            package: "flask".into(),
            version: Some("3.1.0".into()),
            write: false,
            use_mapping: true,
            tree: false,
        };

        let client = reqwest::Client::new();
        let metadata = fetch_pypi_metadata(&opts, &client).await.unwrap();
        let recipe = create_recipe(&opts, &metadata, &client).await.unwrap();

        assert_yaml_snapshot!(recipe);
    }
}
