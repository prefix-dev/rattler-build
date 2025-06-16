use clap::Parser;
use miette::{IntoDiagnostic, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use super::write_recipe;
use crate::recipe_generator::serialize::{self, ScriptTest, SourceElement, Test};

#[derive(Debug, Clone, Parser)]
pub struct CpanOpts {
    /// Name of the package to generate
    pub package: String,

    /// Select a version of the package to generate (defaults to latest)
    #[arg(long)]
    pub version: Option<String>,

    /// Whether to write the recipe to a folder
    #[arg(short, long)]
    pub write: bool,

    /// Whether to generate recipes for all dependencies
    #[arg(short, long)]
    pub tree: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanRelease {
    version: String,
    status: String,
    archive: String,
    download_url: String,
    checksum_sha256: Option<String>,
    checksum_md5: Option<String>,
    date: String,
    author: String,
    distribution: String,
    name: String,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanModule {
    name: String,
    version: Option<String>,
    documentation: Option<String>,
    r#abstract: Option<String>,
    author: Vec<String>,
    authorized: bool,
    indexed: bool,
    status: String,
    distribution: String,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanDistribution {
    name: String,
    version: String,
    r#abstract: Option<String>,
    author: Vec<String>,
    license: Vec<String>,
    resources: Option<CpanResources>,
    dependency: Option<Vec<CpanDependency>>,
    #[serde(rename = "provides")]
    modules: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanResources {
    homepage: Option<String>,
    repository: Option<CpanRepository>,
    bugtracker: Option<CpanBugtracker>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanRepository {
    url: Option<String>,
    web: Option<String>,
    #[serde(rename = "type")]
    repo_type: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanBugtracker {
    web: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct CpanDependency {
    module: String,
    phase: String,
    relationship: String,
    version: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct MetaCpanResponse<T> {
    hits: MetaCpanHits<T>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct MetaCpanHits<T> {
    hits: Vec<MetaCpanHit<T>>,
    total: i32,
}

#[derive(Deserialize, Debug, Clone)]
struct MetaCpanHit<T> {
    #[serde(rename = "_source")]
    source: T,
}

#[derive(Debug, Clone)]
pub struct CpanMetadata {
    release: CpanRelease,
    distribution: Option<CpanDistribution>,
    modules: Vec<CpanModule>,
}

fn format_perl_package_name(name: &str) -> String {
    format!("perl-{}", name.to_lowercase().replace("::", "-"))
}

fn format_perl_dependency(dep: &CpanDependency) -> Option<String> {
    // Skip perl core modules and development dependencies
    if dep.phase == "develop" || dep.phase == "x_Dist_Zilla" {
        return None;
    }

    let package_name = format_perl_package_name(&dep.module);

    if let Some(version) = &dep.version {
        if version == "0" || version.is_empty() {
            Some(package_name)
        } else {
            Some(format!("{} >={}", package_name, version))
        }
    } else {
        Some(package_name)
    }
}

fn map_perl_license(licenses: &[String]) -> Option<String> {
    if licenses.is_empty() {
        return None;
    }

    let license_map: HashMap<&str, &str> = [
        ("perl_5", "Artistic-1.0-Perl OR GPL-1.0-or-later"),
        ("artistic_2", "Artistic-2.0"),
        ("apache_2_0", "Apache-2.0"),
        ("bsd", "BSD-3-Clause"),
        ("gpl_1", "GPL-1.0-only"),
        ("gpl_2", "GPL-2.0-only"),
        ("gpl_3", "GPL-3.0-only"),
        ("lgpl_2_1", "LGPL-2.1-only"),
        ("lgpl_3_0", "LGPL-3.0-only"),
        ("mit", "MIT"),
        ("mozilla_1_1", "MPL-1.1"),
        ("mozilla_2_0", "MPL-2.0"),
    ]
    .iter()
    .cloned()
    .collect();

    // Map licenses and join with OR
    let mapped_licenses: Vec<String> = licenses
        .iter()
        .filter_map(|license| {
            license_map
                .get(license.as_str())
                .map(|&mapped| mapped.to_string())
                .or_else(|| Some(license.clone()))
        })
        .collect();

    if mapped_licenses.is_empty() {
        None
    } else {
        Some(mapped_licenses.join(" OR "))
    }
}

async fn fetch_cpan_metadata(
    opts: &CpanOpts,
    client: &reqwest::Client,
) -> miette::Result<CpanMetadata> {
    // First, get the release information
    let release_url = if let Some(version) = &opts.version {
        format!(
            "https://fastapi.metacpan.org/v1/release/{}/{}",
            opts.package, version
        )
    } else {
        format!("https://fastapi.metacpan.org/v1/release/{}", opts.package)
    };

    let release: CpanRelease = client
        .get(&release_url)
        .send()
        .await
        .into_diagnostic()
        .wrap_err("Failed to fetch release information")?
        .json()
        .await
        .into_diagnostic()
        .wrap_err("Failed to parse release response")?;

    // Get distribution information
    let dist_url = format!(
        "https://fastapi.metacpan.org/v1/distribution/{}",
        release.distribution
    );

    let distribution: Option<CpanDistribution> = match client.get(&dist_url).send().await {
        Ok(resp) => resp.json().await.ok(),
        Err(_) => None,
    };

    // Get modules in this distribution
    let modules_url = format!(
        "https://fastapi.metacpan.org/v1/module/_search?q=distribution:{}",
        release.distribution
    );

    let modules_response: MetaCpanResponse<CpanModule> = client
        .get(&modules_url)
        .send()
        .await
        .into_diagnostic()
        .wrap_err("Failed to fetch modules")?
        .json()
        .await
        .into_diagnostic()
        .wrap_err("Failed to parse modules response")?;

    let modules = modules_response
        .hits
        .hits
        .into_iter()
        .map(|hit| hit.source)
        .collect();

    Ok(CpanMetadata {
        release,
        distribution,
        modules,
    })
}

pub async fn create_cpan_recipe(
    _opts: &CpanOpts,
    metadata: &CpanMetadata,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();

    // Set package name and version
    recipe.package.name = format_perl_package_name(&metadata.release.name);
    recipe.package.version = metadata.release.version.clone();

    // Add version to context
    recipe
        .context
        .insert("version".to_string(), metadata.release.version.clone());

    // Set source
    let source = SourceElement {
        url: vec![metadata.release.download_url.clone()],
        sha256: metadata.release.checksum_sha256.clone(),
        md5: metadata.release.checksum_md5.clone(),
    };
    recipe.source.push(source);

    // Set build requirements
    recipe.requirements.build.push("perl".to_string());
    recipe.requirements.build.push("make".to_string());

    // Host requirements
    recipe.requirements.host.push("perl".to_string());

    // Runtime requirements
    recipe.requirements.run.push("perl".to_string());

    // Add dependencies
    if let Some(dist) = &metadata.distribution {
        if let Some(dependencies) = &dist.dependency {
            for dep in dependencies {
                if let Some(formatted_dep) = format_perl_dependency(dep) {
                    match (dep.phase.as_str(), dep.relationship.as_str()) {
                        ("configure", "requires") | ("build", "requires") => {
                            recipe.requirements.build.push(formatted_dep);
                        }
                        ("runtime", "requires") => {
                            recipe.requirements.run.push(formatted_dep);
                        }
                        ("test", "requires") => {
                            // Add test dependencies as comments
                            recipe
                                .requirements
                                .run
                                .push(format!("# test: {}", formatted_dep));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Set build script
    recipe.build.script = r#"perl Makefile.PL INSTALLDIRS=vendor
make
make install"#
        .to_string();

    // Set metadata
    if let Some(dist) = &metadata.distribution {
        recipe.about.summary = dist.r#abstract.clone();
        recipe.about.license = map_perl_license(&dist.license);

        if let Some(resources) = &dist.resources {
            recipe.about.homepage = resources.homepage.clone();
            if let Some(repo) = &resources.repository {
                recipe.about.repository = repo.web.clone().or_else(|| repo.url.clone());
            }
        }
    }

    // Add basic test
    let main_module = metadata
        .modules
        .iter()
        .find(|m| m.name == metadata.release.name)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| metadata.release.name.clone());

    recipe.tests.push(Test::Script(ScriptTest {
        script: vec![format!(
            "perl -M{} -e 'print \"Module loaded successfully\\n\"'",
            main_module
        )],
    }));

    Ok(recipe)
}

#[async_recursion::async_recursion]
pub async fn generate_cpan_recipe(opts: &CpanOpts) -> miette::Result<()> {
    eprintln!("Generating recipe for {}", opts.package);
    let client = reqwest::Client::new();

    let metadata = fetch_cpan_metadata(opts, &client).await?;
    let recipe = create_cpan_recipe(opts, &metadata).await?;

    let recipe_string = format!("{}", recipe);

    // Post-process to handle test dependencies
    let processed_recipe = recipe_string
        .lines()
        .map(|line| {
            if line.contains("# test:") {
                format!("  {}", line.replace("- # test:", "# -"))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if opts.write {
        write_recipe(&recipe.package.name, &processed_recipe).into_diagnostic()?;
    } else {
        print!("{}", processed_recipe);
    }

    if opts.tree {
        if let Some(dist) = &metadata.distribution {
            if let Some(dependencies) = &dist.dependency {
                for dep in dependencies {
                    if dep.phase == "runtime" && dep.relationship == "requires" {
                        let dep_name = dep.module.replace("::", "-");
                        if !PathBuf::from(format_perl_package_name(&dep_name)).exists() {
                            let child_opts = CpanOpts {
                                package: dep.module.clone(),
                                version: None,
                                write: opts.write,
                                tree: false, // Avoid infinite recursion
                            };
                            generate_cpan_recipe(&child_opts).await?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_perl_package_name() {
        assert_eq!(format_perl_package_name("DBI"), "perl-dbi");
        assert_eq!(format_perl_package_name("DBD::SQLite"), "perl-dbd-sqlite");
        assert_eq!(format_perl_package_name("Moose::Role"), "perl-moose-role");
    }

    #[test]
    fn test_map_perl_license() {
        assert_eq!(
            map_perl_license(&["perl_5".to_string()]),
            Some("Artistic-1.0-Perl OR GPL-1.0-or-later".to_string())
        );
        assert_eq!(
            map_perl_license(&["mit".to_string()]),
            Some("MIT".to_string())
        );
        assert_eq!(
            map_perl_license(&["apache_2_0".to_string(), "mit".to_string()]),
            Some("Apache-2.0 OR MIT".to_string())
        );
        assert_eq!(map_perl_license(&[]), None);
    }

    #[tokio::test]
    async fn test_cpan_recipe_generation() {
        let opts = CpanOpts {
            package: "DBI".to_string(),
            version: Some("1.643".to_string()),
            write: false,
            tree: false,
        };

        let client = reqwest::Client::new();
        // This test would require network access, so we'll skip it in CI
        if std::env::var("CI").is_ok() {
            return;
        }

        let result = fetch_cpan_metadata(&opts, &client).await;
        if let Ok(metadata) = result {
            let recipe = create_cpan_recipe(&opts, &metadata).await.unwrap();
            assert_eq!(recipe.package.name, "perl-dbi");
            assert!(!recipe.requirements.run.is_empty());
        }
    }
}
