use itertools::Itertools;
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};

use crate::recipe_generator::serialize::{self, SourceElement};
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
    pub URL: String,
    pub NeedsCompilation: String,
    pub Packaged: Packaged,
    pub Repository: String,
    #[serde(rename = "Date/Publication")]
    pub DatePublication: String,
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
    pub _pkgdocs: String,
    pub _srconly: String,
    pub _winbinary: String,
    pub _macbinary: String,
    pub _wasmbinary: String,
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
    pub login: String,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Dependency {
    pub package: String,
    pub version: Option<String>,
    pub role: String,
}

fn map_license(license: &str) -> String {
    // replace `|` with ` OR `
    // map GPL-3 to GPL-3.0-only
    // map GPL-2 to GPL-2.0-only
    let license_replacements = [
        ("|", " OR "),
        ("GPL-3", "GPL-3.0-only"),
        ("GPL-2", "GPL-2.0-only"),
        ("GPL (>= 3)", "GPL-3.0-or-later"),
        ("GPL (>= 2)", "GPL-2.0-or-later"),
    ];

    let mut res = license.to_string();
    for (from, to) in license_replacements.iter() {
        res = res.replace(from, to);
    }
    res
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

pub async fn generate_r_recipe(package: &str) -> miette::Result<()> {
    eprintln!("Generating R recipe for {}", package);
    let package_info = reqwest::get(&format!(
        "https://cran.r-universe.dev/api/packages/{}",
        package
    ))
    .await
    .into_diagnostic()?
    .json::<PackageInfo>()
    .await
    .into_diagnostic()?;

    let mut recipe = serialize::Recipe::default();

    recipe.package.name = format_r_package(&package_info.Package.to_lowercase(), None);
    recipe.package.version = package_info.Version.clone();
    let source = SourceElement {
        url: format!(
            "https://cran.r-project.org/src/contrib/{}",
            package_info._file
        ),
        md5: Some(package_info.MD5sum.clone()),
        sha256: None,
    };
    recipe.source.push(source);

    recipe.build.script = "R CMD INSTALL --build .".to_string();

    for dep in package_info._dependencies.iter() {
        if dep.package == "R" {
            // get r-base
            let rbase = format_r_package("base", dep.version.as_ref());
            // recipe.requirements.build.push(rbase);
            recipe.requirements.host.push(rbase);
        } else if dep.role == "Depends" {
        } else if dep.role == "LinkingTo" {
            recipe
                .requirements
                .host
                .push(format_r_package(&dep.package, dep.version.as_ref()));
        } else if dep.role == "Imports" {
            recipe
                .requirements
                .run
                .push(format_r_package(&dep.package, dep.version.as_ref()));
        }
        if dep.role == "LinkingTo" {
            recipe
                .requirements
                .build
                .push("${{ compiler('c') }}".to_string());
            recipe
                .requirements
                .build
                .push("${{ compiler('cxx') }}".to_string());
            recipe.requirements.build.push("make".to_string());
        }
        if dep.role == "Suggests" {
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

    recipe.about.homepage = Some(package_info.URL.clone());
    recipe.about.summary = Some(package_info.Title.clone());
    recipe.about.description = Some(package_info.Description.clone());
    recipe.about.license = Some(map_license(&package_info.License));
    recipe.about.repository = Some(package_info._upstream.clone());
    if url::Url::parse(&package_info._pkgdocs).is_ok() {
        recipe.about.documentation = Some(package_info._pkgdocs.clone());
    }

    // ??
    // recipe.about.license_file = Some("LICENSE".to_string());

    let recipe = format!("{}", recipe);

    let mut final_recipe = String::new();
    for line in recipe.lines() {
        if line.contains("SUGGEST") {
            final_recipe.push_str(&format!(
                "{}  # suggested\n",
                line.replace(" - SUGGEST", " # - ")
            ));
        } else {
            final_recipe.push_str(&format!("{}\n", line));
        }
    }

    print!("{}", final_recipe);

    Ok(())
}
