use clap::Parser;
use miette::{IntoDiagnostic, WrapErr};
use serde::Deserialize;
use serde_with::{OneOrMany, serde_as};
use std::{
    collections::{HashMap, HashSet},
    process::Command,
    sync::OnceLock,
};

use super::write_recipe;
use crate::recipe_generator::serialize::{self, ScriptTest, Test, UrlSourceElement};

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
struct CpanDependency {
    module: String,
    phase: String,
    relationship: String,
    version: Option<String>,
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
    r#abstract: Option<String>,
    license: Option<Vec<String>>,
    metadata: Option<CpanReleaseMetadata>,
    resources: Option<CpanResources>,
    dependency: Option<Vec<CpanDependency>>,
}

#[derive(Deserialize, Debug, Clone)]
struct CpanReleaseMetadata {
    resources: Option<CpanResources>,
    author: Option<Vec<String>>,
}

#[serde_as]
#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct CpanModule {
    name: String,
    version: Option<String>,
    documentation: Option<String>,
    r#abstract: Option<String>,
    #[serde_as(as = "OneOrMany<_>")]
    author: Vec<String>,
    authorized: bool,
    indexed: bool,
    status: String,
    distribution: String,
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
    #[allow(dead_code)]
    modules: Vec<CpanModule>,
}

static CORE_MODULES: OnceLock<HashSet<String>> = OnceLock::new();

fn get_core_modules_from_perl() -> Result<HashSet<String>, std::io::Error> {
    let output = Command::new("perl")
        .arg("-e")
        .arg(
            "use Module::CoreList; \
             my @modules = grep {Module::CoreList::is_core($_)} Module::CoreList->find_modules(qr/.*/); \
             print join \"\\n\", @modules;"
        )
        .output()?;

    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Perl command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    let modules_text = String::from_utf8_lossy(&output.stdout);
    let modules: HashSet<String> = modules_text
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(modules)
}

fn get_fallback_core_modules() -> HashSet<String> {
    // Fallback list of common Perl core modules
    const CORE_MODULES_FALLBACK: &[&str] = &[
        "strict",
        "warnings",
        "Carp",
        "Exporter",
        "File::Spec",
        "File::Path",
        "File::Copy",
        "File::Find",
        "Data::Dumper",
        "Scalar::Util",
        "List::Util",
        "FindBin",
        "lib",
        "base",
        "constant",
        "vars",
        "utf8",
        "Encode",
        "POSIX",
        "Fcntl",
        "Socket",
        "IO::Handle",
        "IO::File",
        "Time::Local",
        "Getopt::Long",
        "Getopt::Std",
        "Pod::Usage",
        "Test::More",
        "Test::Simple",
        "File::Basename",
        "File::Temp",
        "Digest::MD5",
        "MIME::Base64",
        "Storable",
        "Sys::Hostname",
        "Text::ParseWords",
        "Text::Tabs",
        "Text::Wrap",
        "Time::HiRes",
        "AutoLoader",
        "Benchmark",
        "Config",
        "Errno",
        "ExtUtils::MakeMaker",
        "Fcntl",
        "Getopt::Std",
        "IO",
        "IPC::Open2",
        "IPC::Open3",
        "Math::BigFloat",
        "Math::BigInt",
        "Net::Ping",
        "Symbol",
        "Tie::Array",
        "Tie::Hash",
        "XSLoader",
        // Add more as needed based on common core modules
    ];

    CORE_MODULES_FALLBACK
        .iter()
        .map(|&s| s.to_string())
        .collect()
}

fn initialize_core_modules() -> &'static HashSet<String> {
    CORE_MODULES.get_or_init(|| match get_core_modules_from_perl() {
        Ok(modules) => {
            tracing::info!(
                "Successfully loaded {} core modules from system Perl",
                modules.len()
            );
            modules
        }
        Err(e) => {
            tracing::warn!(
                "Warning: Failed to get core modules from system Perl: {}",
                e
            );
            tracing::warn!("Falling back to hardcoded core module list.");
            tracing::warn!("Consider installing Perl with: pixi global install perl");
            get_fallback_core_modules()
        }
    })
}

fn is_core_module(module: &str) -> bool {
    let core_modules = initialize_core_modules();
    core_modules.contains(module)
}

fn process_dependencies(dependencies: &[CpanDependency]) -> (Vec<String>, Vec<String>) {
    let mut host_deps = Vec::new();
    let mut run_deps = Vec::new();

    for dep in dependencies {
        // Skip develop dependencies
        if dep.phase == "develop" {
            continue;
        }

        // Only process "requires" relationships
        if dep.relationship != "requires" {
            continue;
        }

        // Skip core modules
        if is_core_module(&dep.module) {
            continue;
        }

        let conda_name = format_perl_package_name(&dep.module);

        // Add version constraint if specified
        let dep_spec = if let Some(version) = &dep.version {
            if version != "0" && !version.is_empty() && version != "undef" {
                format!("{} >={}", conda_name, version)
            } else {
                conda_name
            }
        } else {
            conda_name
        };

        match dep.phase.as_str() {
            "build" | "configure" => {
                if !host_deps.contains(&dep_spec) {
                    host_deps.push(dep_spec);
                }
            }
            "runtime" => {
                if !run_deps.contains(&dep_spec) {
                    run_deps.push(dep_spec);
                }
            }
            _ => {
                // Default to runtime for unknown phases
                if !host_deps.contains(&dep_spec) {
                    host_deps.push(dep_spec.clone());
                }
                if !run_deps.contains(&dep_spec) {
                    run_deps.push(dep_spec);
                }
            }
        }
    }

    (host_deps, run_deps)
}

fn format_perl_package_name(name: &str) -> String {
    if name == "perl" {
        return "perl".to_string();
    }

    format!("perl-{}", name.to_lowercase().replace("::", "-"))
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
    // First, try to get the release information assuming it's a distribution name
    let release_url = if let Some(version) = &opts.version {
        format!(
            "https://fastapi.metacpan.org/v1/release/{}/{}",
            opts.package, version
        )
    } else {
        format!("https://fastapi.metacpan.org/v1/release/{}", opts.package)
    };

    let release_response = client.get(&release_url).send().await.into_diagnostic()?;

    let release: CpanRelease = if release_response.status().is_success() {
        release_response
            .json()
            .await
            .into_diagnostic()
            .wrap_err("Failed to parse release response")?
    } else {
        // If release lookup failed, it might be a module name instead of distribution name
        // Try to find the distribution that contains this module
        let module_url = format!("https://fastapi.metacpan.org/v1/module/{}", opts.package);
        let module_response = client
            .get(&module_url)
            .send()
            .await
            .into_diagnostic()
            .wrap_err("Failed to fetch module information")?;

        if !module_response.status().is_success() {
            return Err(miette::miette!(
                "Cannot find distribution or module named '{}'. Check if the name is correct.",
                opts.package
            ));
        }

        // Parse the module response to get the distribution name
        let module_data: serde_json::Value = module_response
            .json()
            .await
            .into_diagnostic()
            .wrap_err("Failed to parse module response")?;

        let distribution_name = module_data
            .get("distribution")
            .and_then(|d| d.as_str())
            .ok_or_else(|| miette::miette!("Module response missing distribution field"))?;

        // Now fetch the release using the correct distribution name
        let dist_release_url = if let Some(version) = &opts.version {
            format!(
                "https://fastapi.metacpan.org/v1/release/{}/{}",
                distribution_name, version
            )
        } else {
            format!(
                "https://fastapi.metacpan.org/v1/release/{}",
                distribution_name
            )
        };

        client
            .get(&dist_release_url)
            .send()
            .await
            .into_diagnostic()
            .wrap_err("Failed to fetch release information for distribution")?
            .json()
            .await
            .into_diagnostic()
            .wrap_err("Failed to parse distribution release response")?
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

    Ok(CpanMetadata { release, modules })
}

pub async fn create_cpan_recipe(
    _opts: &CpanOpts,
    metadata: &CpanMetadata,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();

    // Set package name and version
    recipe.package.name = format_perl_package_name(&metadata.release.distribution);
    recipe.package.version = metadata.release.version.clone();

    // Add version to context
    recipe
        .context
        .insert("version".to_string(), metadata.release.version.clone());

    // Set source
    let source = UrlSourceElement {
        url: vec![metadata.release.download_url.clone()],
        sha256: metadata.release.checksum_sha256.clone(),
        md5: metadata.release.checksum_md5.clone(),
    };
    recipe.source.push(source.into());

    // Set build requirements
    recipe.requirements.build.push("make".to_string());

    // Host requirements
    recipe.requirements.host.push("perl".to_string());

    // Runtime requirements
    recipe.requirements.run.push("perl".to_string());

    // Process dependencies
    if let Some(dependencies) = &metadata.release.dependency {
        let (host_deps, run_deps) = process_dependencies(dependencies);

        // Add dependencies to appropriate sections
        recipe.requirements.host.extend(host_deps);
        recipe.requirements.run.extend(run_deps);
    }

    // Set build script
    recipe.build.script = r#"perl Makefile.PL INSTALLDIRS=vendor
make
make install"#
        .to_string();

    // Detect if package should be noarch
    // Most Perl packages are noarch unless they contain XS (C extensions)
    // Heuristics: check if package name contains common XS indicators
    let package_name = &metadata.release.distribution;
    let is_likely_xs = package_name.contains("XS")
        || package_name.contains("Fast")
        || package_name.contains("DB")
        || package_name.contains("Crypt");

    if !is_likely_xs {
        recipe.build.noarch = Some("generic".to_string());
    }

    // Set metadata from release
    if let Some(abstract_text) = &metadata.release.r#abstract {
        recipe.about.summary = Some(abstract_text.clone());
    }

    if let Some(licenses) = &metadata.release.license {
        recipe.about.license = map_perl_license(licenses);
    }

    // Get resources from release metadata or top-level
    let resources = metadata.release.resources.as_ref().or_else(|| {
        metadata
            .release
            .metadata
            .as_ref()
            .and_then(|m| m.resources.as_ref())
    });

    if let Some(resources) = resources {
        recipe.about.homepage = resources.homepage.clone();
        if let Some(repo) = &resources.repository {
            recipe.about.repository = repo.web.clone().or_else(|| repo.url.clone());
        }

        // Add bugtracker URL if available
        if let Some(bugtracker) = &resources.bugtracker {
            if recipe.about.repository.is_none() {
                recipe.about.repository = bugtracker.web.clone();
            }
        }
    }

    // Add author information
    if let Some(metadata) = &metadata.release.metadata {
        if let Some(authors) = &metadata.author {
            if !authors.is_empty() {
                let authors_str = authors.join(", ");
                recipe.about.description = Some(format!("By {}", authors_str));
            }
        }
    }

    // Add additional metadata from modules if not already set
    if !metadata.modules.is_empty() {
        let main_module = &metadata.modules[0];
        if recipe.about.summary.is_none() && main_module.r#abstract.is_some() {
            recipe.about.summary = main_module.r#abstract.clone();
        }

        if let Some(doc) = &main_module.documentation {
            // Convert module documentation to MetaCPAN URL
            recipe.about.documentation = Some(format!(
                "https://metacpan.org/pod/{}",
                doc.replace("::", "%3A%3A")
            ));
        }
    }

    // Set homepage to MetaCPAN page if not already set
    if recipe.about.homepage.is_none() {
        recipe.about.homepage = Some(format!(
            "https://metacpan.org/release/{}/{}",
            metadata.release.author, metadata.release.name
        ));
    }

    // Add basic test - for simplicity, just use the distribution name converted to module format
    // This works for most cases: DBI -> DBI, Perl-Tidy -> Perl::Tidy, etc.
    let main_module = metadata.release.distribution.replace("-", "::");

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
    tracing::info!("Generating recipe for {}", opts.package);
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
        // TODO: Implement dependency tree generation
        eprintln!("Warning: --tree option not yet implemented for CPAN packages");
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
