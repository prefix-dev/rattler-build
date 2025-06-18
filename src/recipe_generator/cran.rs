use clap::Parser;
use std::{collections::HashMap, collections::HashSet, path::PathBuf};

use itertools::Itertools;
use miette::IntoDiagnostic;
use rattler_digest::{Sha256Hash, compute_bytes_digest};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use url::Url;

use crate::recipe_generator::{
    serialize::{self, ScriptTest, Test, UrlSourceElement},
    write_recipe,
};
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct PackageInfo {
    pub Package: String,
    pub Title: String,
    pub Description: String,
    pub Version: String,
    pub Author: String,
    pub Maintainer: String,
    pub License: String,
    pub URL: Option<String>,
    pub NeedsCompilation: String,
    pub Packaged: Packaged,
    pub Repository: String,
    #[serde(rename = "Date/Publication")]
    pub DatePublication: Option<String>,
    pub MD5sum: String,
    pub _user: String,
    pub _type: String,
    pub _file: String,
    pub _fileid: String,
    pub _filesize: i64,
    pub _created: String,
    pub _published: String,
    pub _upstream: String,
    pub _commit: Commit,
    pub _maintainer: Maintainer,
    pub _distro: String,
    pub _host: String,
    pub _status: String,
    pub _pkgdocs: Option<String>,
    pub _srconly: Option<String>,
    pub _winbinary: Option<String>,
    pub _macbinary: Option<String>,
    pub _wasmbinary: Option<String>,
    pub _buildurl: String,
    pub _registered: bool,
    pub _dependencies: Vec<Dependency>,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Packaged {
    pub Date: String,
    pub User: String,
}

#[derive(Debug, Clone, Parser)]
pub struct CranOpts {
    /// The R Universe to fetch the package from (defaults to `cran`)
    #[arg(short, long)]
    universe: Option<String>,

    /// Whether to create recipes for the whole dependency tree or not
    #[arg(short, long)]
    tree: bool,

    /// Name of the package to generate
    pub package: String,

    /// Whether to write the recipe to a folder
    #[arg(short, long)]
    pub write: bool,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Commit {
    pub id: String,
    pub author: String,
    pub committer: String,
    pub message: String,
    pub time: i64,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Maintainer {
    pub name: String,
    pub email: String,
    pub login: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Dependency {
    pub package: String,
    pub version: Option<String>,
    pub role: String,
}

fn map_license(license: &str) -> (Option<String>, Option<String>) {
    let license_replacements: HashMap<&str, &str> = [
        ("GPL-3", "GPL-3.0-only"),
        ("GPL-2", "GPL-2.0-only"),
        ("GPL (>= 3)", "GPL-3.0-or-later"),
        ("GPL (>= 3.0)", "GPL-3.0-or-later"),
        ("GPL (>= 2)", "GPL-2.0-or-later"),
        ("GPL (>= 2.0)", "GPL-2.0-or-later"),
        ("GPL (== 3)", "GPL-3.0-only"),
        ("GPL (== 2)", "GPL-2.0-only"),
        ("LGPL-3", "LGPL-3.0-only"),
        ("LGPL-2", "LGPL-2.0-only"),
        ("LGPL-2.1", "LGPL-2.1-only"),
        ("LGPL (>= 3)", "LGPL-3.0-or-later"),
        ("LGPL (>= 2)", "LGPL-2.0-or-later"),
        ("LGPL (>= 2.1)", "LGPL-2.1-or-later"),
        ("BSD_3_clause", "BSD-3-Clause"),
        ("BSD_2_clause", "BSD-2-Clause"),
        ("Apache License (== 2.0)", "Apache-2.0"),
        ("Apache License 2.0", "Apache-2.0"),
        ("MIT License", "MIT"),
        ("CC0", "CC0-1.0"),
        ("CC BY 4.0", "CC-BY-4.0"),
        ("CC BY-NC 4.0", "CC-BY-NC-4.0"),
        ("CC BY-SA 4.0", "CC-BY-SA-4.0"),
        ("AGPL-3", "AGPL-3.0-only"),
        ("AGPL (>= 3)", "AGPL-3.0-or-later"),
        ("EPL", "EPL-1.0"),
        ("EUPL", "EUPL-1.1"),
        ("Mozilla Public License 1.0", "MPL-1.0"),
        ("Mozilla Public License 2.0", "MPL-2.0"),
    ]
    .iter()
    .cloned()
    .collect();

    // Split the license string at '|' to separate licenses
    let parts: Vec<&str> = license.split(&['|', '+']).map(str::trim).collect();

    let mut final_licenses = Vec::new();
    let mut license_file = None;

    for part in parts {
        if part.to_lowercase().contains("file") {
            // This part contains the file specification
            license_file = part.split_whitespace().last().map(|s| s.to_string());
        } else {
            // This part is a license
            let mapped = license_replacements.get(part).map_or(part, |&s| s);
            final_licenses.push(mapped.to_string());
        }
    }

    let final_license = if final_licenses.is_empty() {
        None
    } else {
        Some(final_licenses.join(" OR "))
    };

    (final_license, license_file)
}

fn format_r_package(package: &str, version: Option<&String>) -> String {
    let mut res = format!("r-{}", package.to_lowercase());
    if let Some(version) = version {
        // filter all whitespace
        let version = version.split_whitespace().collect::<String>();
        res.push_str(&format!(" {}", version));
    }
    res
}

pub async fn fetch_package_sha256sum(url: &Url) -> Result<Sha256Hash, miette::Error> {
    let client = reqwest::Client::new();
    let response = client.get(url.clone()).send().await.into_diagnostic()?;
    let bytes = response.bytes().await.into_diagnostic()?;
    Ok(compute_bytes_digest::<Sha256>(&bytes))
}

// Found when running `installed.packages()` in an `r-base` environment
// Updated for `R 4.4.1`
const R_BUILTINS: &[&str] = &[
    "base",
    "compiler",
    "datasets",
    "graphics",
    "grDevices",
    "grid",
    "methods",
    "parallel",
    "splines",
    "stats",
    "stats4",
    "tcltk",
    "tools",
    "utils",
];

#[async_recursion::async_recursion]
pub async fn generate_r_recipe(opts: &CranOpts) -> miette::Result<()> {
    let package = &opts.package;
    tracing::info!("Generating R recipe for {}", package);
    let universe = opts.universe.as_deref().unwrap_or("cran");
    let package_info = reqwest::get(&format!(
        "https://{universe}.r-universe.dev/api/packages/{}",
        package
    ))
    .await
    .into_diagnostic()?
    .json::<PackageInfo>()
    .await
    .into_diagnostic()?;

    let mut recipe = serialize::Recipe::default();

    recipe.package.name = format_r_package(&package_info.Package.to_lowercase(), None);
    // some versions have a `-` in them (i think that's like a build number in debian)
    // we just replace it with a `.`
    recipe.package.version = package_info.Version.replace('-', ".").clone();

    let url = Url::parse(&format!(
        "https://cran.r-project.org/src/contrib/{}",
        package_info._file
    ))
    .expect("Failed to parse URL");

    // It looks like CRAN moves the package to the archive for old versions
    // so let's add that as a fallback mirror
    let url_archive = Url::parse(&format!(
        "https://cran.r-project.org/src/contrib/Archive/{}",
        package_info._file
    ))
    .expect("Failed to parse URL");

    let sha256 = fetch_package_sha256sum(&url).await?;

    let source = UrlSourceElement {
        url: vec![url.to_string(), url_archive.to_string()],
        md5: None,
        sha256: Some(format!("{:x}", sha256)),
    };
    recipe.source.push(source.into());

    recipe.build.script = "R CMD INSTALL --build .".to_string();

    let build_requirements = vec![
        "${{ compiler('c') }}".to_string(),
        "${{ compiler('cxx') }}".to_string(),
        "make".to_string(),
    ];

    if package_info.NeedsCompilation == "yes" {
        recipe.requirements.build.extend(build_requirements.clone());
    }

    recipe.requirements.host = vec!["r-base".to_string()];
    recipe.requirements.run = vec!["r-base".to_string()];

    let mut remaining_deps = HashSet::new();
    for dep in package_info._dependencies.iter() {
        // skip builtins
        if R_BUILTINS.contains(&dep.package.as_str()) {
            continue;
        }

        if dep.package == "R" {
            // get r-base
            let rbase = format_r_package("base", dep.version.as_ref());
            recipe.requirements.host.push(rbase);
        } else if dep.role == "LinkingTo" {
            recipe
                .requirements
                .host
                .push(format_r_package(&dep.package, dep.version.as_ref()));
            recipe.requirements.build.extend(build_requirements.clone());
            remaining_deps.insert(dep.package.clone());
        } else if dep.role == "Imports" || dep.role == "Depends" {
            recipe
                .requirements
                .run
                .push(format_r_package(&dep.package, dep.version.as_ref()));
            recipe
                .requirements
                .host
                .push(format_r_package(&dep.package, dep.version.as_ref()));
            remaining_deps.insert(dep.package.clone());
        } else if dep.role == "Suggests" {
            recipe.requirements.run.push(format!(
                "SUGGEST {}",
                format_r_package(&dep.package, dep.version.as_ref())
            ));
        }
    }

    // make requirements unique
    recipe.requirements.host = recipe.requirements.host.into_iter().unique().collect();
    recipe.requirements.build = recipe.requirements.build.into_iter().unique().collect();
    recipe.requirements.run = recipe.requirements.run.into_iter().unique().collect();

    if let Some(url) = package_info.URL.clone() {
        let url = url.split_once(',').unwrap_or((url.as_str(), "")).0;
        recipe.about.homepage = Some(url.to_string());
    }

    recipe.about.summary = Some(package_info.Title.clone());
    recipe.about.description = Some(package_info.Description.clone());
    (recipe.about.license, recipe.about.license_file) = map_license(&package_info.License);
    recipe.about.repository = Some(package_info._upstream.clone());
    if let Some(pkgdocs) = &package_info._pkgdocs {
        if url::Url::parse(pkgdocs).is_ok() {
            recipe.about.documentation = Some(pkgdocs.clone());
        }
    }

    recipe.tests.push(Test::Script(ScriptTest {
        script: vec![format!(
            "Rscript -e 'library(\"{}\")'",
            package_info.Package
        )],
    }));

    let recipe_str = format!("{}", recipe);

    let mut final_recipe = String::new();
    for line in recipe_str.lines() {
        if line.contains("SUGGEST") {
            final_recipe.push_str(&format!(
                "{}  # suggested\n",
                line.replace(" - SUGGEST", " # - ")
            ));
        } else {
            final_recipe.push_str(&format!("{}\n", line));
        }
    }

    if opts.write {
        write_recipe(&recipe.package.name, &final_recipe).into_diagnostic()?;
    } else {
        print!("{}", final_recipe);
    }

    if opts.tree {
        for dep in remaining_deps {
            let r_package = format_r_package(&dep, None);

            if !PathBuf::from(r_package).exists() {
                let opts = CranOpts {
                    package: dep,
                    ..opts.clone()
                };
                generate_r_recipe(&opts).await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_mapping() {
        let test_cases = vec![
            // Simple cases
            ("GPL-3", "GPL-3.0-only", None),
            ("MIT", "MIT", None),
            ("Apache License 2.0", "Apache-2.0", None),
            // Cases with file LICENSE
            ("GPL-3 + file LICENSE", "GPL-3.0-only", Some("LICENSE")),
            ("MIT + file LICENCE", "MIT", Some("LICENCE")),
            ("MIT + file LICENSE", "MIT", Some("LICENSE")),
            // Compound licenses
            ("GPL-2 | MIT", "GPL-2.0-only OR MIT", None),
            (
                "Apache License 2.0 | file LICENSE",
                "Apache-2.0",
                Some("LICENSE"),
            ),
            // Version ranges
            ("GPL (>= 2)", "GPL-2.0-or-later", None),
            ("LGPL (>= 3)", "LGPL-3.0-or-later", None),
            // More complex cases
            (
                "GPL (>= 2) | BSD_3_clause + file LICENSE",
                "GPL-2.0-or-later OR BSD-3-Clause",
                Some("LICENSE"),
            ),
            ("LGPL-2.1 | file LICENSE", "LGPL-2.1-only", Some("LICENSE")),
            (
                "GPL (>= 2.0) | file LICENCE",
                "GPL-2.0-or-later",
                Some("LICENCE"),
            ),
            // Cases that should remain unchanged
            ("Unlimited", "Unlimited", None),
            ("GPL (>= 2.15.1)", "GPL (>= 2.15.1)", None),
            // Creative Commons licenses
            ("CC BY-SA 4.0", "CC-BY-SA-4.0", None),
            ("CC BY-NC-ND 3.0 US", "CC BY-NC-ND 3.0 US", None), // This one doesn't have a direct SPDX mapping
            // Multiple licenses with file
            (
                "GPL-2 | GPL-3 | MIT + file LICENSE",
                "GPL-2.0-only OR GPL-3.0-only OR MIT",
                Some("LICENSE"),
            ),
        ];

        for (input, expected_license, expected_file) in test_cases {
            let (mapped_license, license_file) = map_license(input);
            assert_eq!(
                mapped_license.as_deref(),
                Some(expected_license),
                "Failed for input: {}",
                input
            );
            assert_eq!(
                license_file.as_deref(),
                expected_file,
                "Failed for input: {}",
                input
            );
        }
    }
}
