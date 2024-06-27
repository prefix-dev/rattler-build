use itertools::Itertools;
use miette::IntoDiagnostic;
use rattler_digest::{compute_bytes_digest, Sha256Hash};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use url::Url;

use crate::recipe_generator::{
    serialize::{self, ScriptTest, SourceElement, Test},
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
    // replace `|` with ` OR `
    // map GPL-3 to GPL-3.0-only
    // map GPL-2 to GPL-2.0-only

    // split at `+`
    let (license, file) = license.rsplit_once('+').unwrap_or((license, ""));

    let license_replacements = [
        ("|", " OR "),
        ("GPL-3", "GPL-3.0-only"),
        ("GPL-2", "GPL-2.0-only"),
        ("GPL (>= 3)", "GPL-3.0-or-later"),
        ("GPL (>= 2)", "GPL-2.0-or-later"),
        ("BSD_3_Clause", "BSD-3-Clause"),
    ];

    let mut res = license.to_string();
    for (from, to) in license_replacements.iter() {
        res = res.replace(from, to);
    }

    if file.trim().starts_with("file") {
        let file = file.split_whitespace().last().unwrap();
        (Some(res), Some(file.to_string()))
    } else {
        (Some(res), None)
    }
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

// According to https://stat.ethz.ch/R-manual/R-devel/doc/html/packages.html
const R_BUILTINS : [&'static str; 29] = [
    "base",
    "boot",
    "class",
    "cluster",
    "codetools",
    "compiler",
    "datasets",
    "foreign",
    "graphics",
    "grDevices",
    "grid",
    "KernSmooth",
    "lattice",
    "MASS",
    "Matrix",
    "methods",
    "mgcv",
    "nlme",
    "nnet",
    "parallel",
    "rpart",
    "spatial",
    "splines",
    "stats",
    "stats4",
    "survival",
    "tcltk",
    "tools",
    "utils",
];


pub async fn generate_r_recipe(package: &str, write: bool) -> miette::Result<()> {
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

    let url = Url::parse(&format!(
        "https://cran.r-project.org/src/contrib/{}",
        package_info._file
    ))
    .expect("Failed to parse URL");

    let sha256 = fetch_package_sha256sum(&url).await?;

    let source = SourceElement {
        url: url.to_string(),
        md5: None,
        sha256: Some(format!("{:x}", sha256)),
    };
    recipe.source.push(source);

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

    for dep in package_info._dependencies.iter() {
        // skip builtins
        if R_BUILTINS.contains(&dep.package.as_str()) {
            continue;
        }

        if dep.package == "R" {
            // get r-base
            let rbase = format_r_package("base", dep.version.as_ref());
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
            recipe
                .requirements
                .host
                .push(format_r_package(&dep.package, dep.version.as_ref()));
        }
        if dep.role == "LinkingTo" {
            recipe.requirements.build.extend(build_requirements.clone());
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

    recipe.about.homepage = package_info.URL.clone();
    recipe.about.summary = Some(package_info.Title.clone());
    recipe.about.description = Some(package_info.Description.clone());
    (recipe.about.license, recipe.about.license_file) = map_license(&package_info.License);
    recipe.about.repository = Some(package_info._upstream.clone());
    if url::Url::parse(&package_info._pkgdocs).is_ok() {
        recipe.about.documentation = Some(package_info._pkgdocs.clone());
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

    if write {
        write_recipe(&recipe.package.name, &final_recipe).into_diagnostic()?;
    } else {
        print!("{}", final_recipe);
    }

    Ok(())
}
