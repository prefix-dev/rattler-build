use std::io::Read;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::{collections::HashMap, collections::HashSet};

#[cfg(feature = "cli")]
use clap::Parser;
use itertools::Itertools;
use miette::IntoDiagnostic;
use rattler_digest::compute_bytes_digest;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use url::Url;

use crate::serialize::{
    self, RTest, RTestInner, Requirement, Script, ScriptStep, ScriptTest, ScriptTestFiles,
    ScriptTestRequirements, Test, UrlSourceElement,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::write_recipe;
/// Package metadata returned by the R-universe/CRAN API.
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
    pub MD5sum: Option<String>,
    pub _user: String,
    pub _type: String,
    pub _file: String,
    pub _fileid: String,
    pub _filesize: i64,
    pub _created: String,
    pub _published: String,
    pub _upstream: String,
    /// Development repository detected by R-universe (e.g. the GitHub project),
    /// as opposed to `_upstream`, which is the CRAN mirror on GitHub.
    pub _devurl: Option<String>,
    pub _commit: Commit,
    pub _maintainer: Maintainer,
    pub _distro: String,
    pub _host: String,
    pub _status: String,
    pub _pkgdocs: Option<String>,
    /// URL of the package's pkgdown documentation site, if any.
    pub _pkgdown: Option<String>,
    pub _srconly: Option<String>,
    pub _winbinary: Option<String>,
    pub _macbinary: Option<String>,
    pub _wasmbinary: Option<String>,
    pub _buildurl: String,
    pub _registered: bool,
    pub _dependencies: Vec<Dependency>,
}

/// Packaging time and user information.
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Packaged {
    pub Date: String,
    pub User: String,
}

/// Options to control CRAN/R recipe generation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "cli", derive(Parser))]
pub struct CranOpts {
    /// The R Universe to fetch the package from (defaults to `cran`)
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub universe: Option<String>,

    /// Whether to create recipes for the whole dependency tree or not
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub tree: bool,

    /// Name of the package to generate
    pub package: String,

    /// Whether to write the recipe to a folder
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub write: bool,

    /// GitHub handle(s) to list under `extra.recipe-maintainers` (repeatable)
    #[cfg_attr(
        feature = "cli",
        arg(short, long = "maintainer", value_name = "GITHUB_ID")
    )]
    pub maintainers: Vec<String>,

    /// Append the package's DESCRIPTION file as a comment block at the end of
    /// the recipe, as conda-forge's staged-recipes reviewers ask for
    #[cfg_attr(feature = "cli", arg(long))]
    pub staged_recipes: bool,
}

/// Commit information from the R-universe/CRAN API.
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Commit {
    pub id: String,
    pub author: String,
    pub committer: String,
    pub message: String,
    pub time: i64,
}

/// Maintainer information from the R-universe/CRAN API.
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Maintainer {
    pub name: String,
    pub email: String,
    pub login: Option<String>,
}

/// Dependency specification for a CRAN package, including role and version.
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug)]
pub struct Dependency {
    pub package: String,
    pub version: Option<String>,
    pub role: String,
}

/// Prefix-relative directory that the `r-base` package installs the standard
/// license texts into. Combined with a license id (see [`r_bundled_license`])
/// this yields a [`LateBoundPath`]-style `${{ PREFIX }}` reference that
/// rattler-build resolves at packaging time.
///
/// [`LateBoundPath`]: rattler_build_types::late_bound_path::LateBoundPath
const R_LICENSE_DIR: &str = "${{ PREFIX }}/lib/R/share/licenses/";

/// Map an SPDX license id to the matching license file shipped by `r-base`
/// under `lib/R/share/licenses/`, if one exists.
///
/// R packages frequently only declare a standard license (e.g. `GPL-2`) without
/// bundling the license text in their sources. conda-forge handles this by
/// pointing `license_file` at the copy that `r-base` installs, which is what
/// this mapping reproduces. `*-or-later` expressions map to the base version
/// file, which is the one actually named by the license.
fn r_bundled_license(spdx: &str) -> Option<&'static str> {
    Some(match spdx {
        "GPL-2.0-only" | "GPL-2.0-or-later" => "GPL-2",
        "GPL-3.0-only" | "GPL-3.0-or-later" => "GPL-3",
        "LGPL-2.0-only" | "LGPL-2.0-or-later" => "LGPL-2",
        "LGPL-2.1-only" | "LGPL-2.1-or-later" => "LGPL-2.1",
        "LGPL-3.0-only" | "LGPL-3.0-or-later" => "LGPL-3",
        "AGPL-3.0-only" | "AGPL-3.0-or-later" => "AGPL-3",
        "Artistic-2.0" => "Artistic-2.0",
        "BSD-2-Clause" => "BSD_2_clause",
        "BSD-3-Clause" => "BSD_3_clause",
        "MIT" => "MIT",
        "MPL-2.0" => "MPL-2.0",
        "Apache-2.0" => "Apache-2.0",
        "CC0-1.0" => "CC0",
        _ => return None,
    })
}

/// Parse a CRAN `License:` field into an SPDX expression and the list of
/// license files to ship.
///
/// The returned license files are, in order, any standard licenses provided by
/// `r-base` (referenced via `${{ PREFIX }}`) followed by any package-local file
/// declared via `+ file LICENSE`.
fn map_license(license: &str) -> (Option<String>, Vec<String>) {
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
    let mut license_files = Vec::new();
    let mut package_license_file = None;

    for part in parts {
        if part.to_lowercase().contains("file") {
            // This part points at a license file shipped inside the package
            // sources (e.g. `MIT + file LICENSE`).
            package_license_file = part.split_whitespace().last().map(|s| s.to_string());
        } else {
            // This part is a license
            let mapped = license_replacements.get(part).map_or(part, |&s| s);
            // If `r-base` ships the text for this license, reference its copy so
            // the built package carries a license file even when the upstream
            // sources do not include one.
            if let Some(bundled) = r_bundled_license(mapped) {
                let path = format!("{R_LICENSE_DIR}{bundled}");
                if !license_files.contains(&path) {
                    license_files.push(path);
                }
            }
            final_licenses.push(mapped.to_string());
        }
    }

    if let Some(file) = package_license_file {
        license_files.push(file);
    }

    let final_license = if final_licenses.is_empty() {
        None
    } else {
        Some(final_licenses.join(" OR "))
    };

    (final_license, license_files)
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

/// Download the package tarball at `url`.
pub async fn fetch_package_tarball(url: &Url) -> Result<Vec<u8>, miette::Error> {
    let client = reqwest::Client::new();
    let response = client
        .get(url.clone())
        .send()
        .await
        .into_diagnostic()?
        // A 404 page must not end up hashed as if it were the tarball.
        .error_for_status()
        .into_diagnostic()?;
    let bytes = response.bytes().await.into_diagnostic()?;
    Ok(bytes.to_vec())
}

/// Extract the `DESCRIPTION` file (`<package>/DESCRIPTION`) from a CRAN
/// source tarball. Deeper `DESCRIPTION` files (e.g. test fixtures under
/// `inst/`) are ignored.
fn extract_description(tarball: &[u8]) -> miette::Result<Option<String>> {
    let tar = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(tar);
    for entry in archive.entries().into_diagnostic()? {
        let mut entry = entry.into_diagnostic()?;
        let is_top_level_description = {
            let path = entry.path().into_diagnostic()?;
            let depth = path
                .components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .count();
            depth == 2 && path.file_name().is_some_and(|name| name == "DESCRIPTION")
        };
        if is_top_level_description {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).into_diagnostic()?;
            return Ok(Some(contents));
        }
    }
    Ok(None)
}

/// Append the package's DESCRIPTION file to the rendered recipe as a comment
/// block, in the format `conda skeleton cran` used, so that reviewers can check
/// the recipe against the upstream metadata without opening the tarball.
fn append_description_comment(recipe: &str, description: &str) -> String {
    let mut out = recipe.trim_end_matches('\n').to_string();
    out.push_str("\n\n# The original CRAN metadata for this package was:\n\n");
    for line in description
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
    {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    out
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

/// The build command. `${{ R }}` resolves to the R binary of the host prefix
/// at build time.
const R_CMD_INSTALL: &str = "${{ R }} CMD INSTALL --build .";

/// Build script for compiled packages: [`R_CMD_INSTALL`] with `R_ARGS` passed
/// through (spelled per shell), so that recipe authors can inject e.g.
/// `--configure-args`. Pure-R packages have no configure step and use the
/// bare command.
fn compiled_build_script() -> Script {
    Script::Steps(vec![ScriptStep {
        condition: "win".to_string(),
        then: format!("{R_CMD_INSTALL} %R_ARGS%"),
        otherwise: Some(format!("{R_CMD_INSTALL} ${{R_ARGS}}")),
    }])
}

/// `cross-r-base` is required to cross-compile; `r_base` is the conda-forge
/// variant key pinning the R version.
fn cross_r_base_requirement() -> Requirement {
    Requirement::Conditional {
        condition: "build_platform != target_platform".to_string(),
        then: vec!["cross-r-base ${{ r_base }}".to_string()],
    }
}

/// Default value of the `cran_mirror` context variable. conda-forge provides
/// `cran_mirror` through its variant config; defining it in `context` keeps the
/// recipe buildable without one (a `context` variable shadows a variant key of
/// the same name).
const CRAN_MIRROR: &str = "https://cran.r-project.org";

/// Placeholder maintainer when none is given on the command line (the same
/// convention grayskull uses).
const DEFAULT_MAINTAINER: &str = "AddYourGitHubIdHere";

/// Convert an R `Depends: R (>= x.y.z)` version string into a rattler-build
/// `skip` expression. Only `>=` constraints are handled; returns `None` for
/// anything else.
fn r_dep_version_to_skip(version: &str) -> Option<String> {
    let num = version
        .trim()
        .trim_start_matches(">=")
        .trim_start_matches('>')
        .trim();
    let parts: Vec<&str> = num.splitn(3, '.').collect();
    match parts.as_slice() {
        [major, minor, ..] if !major.is_empty() => {
            Some(format!("match(r_base, \"<{}.{}\")", major, minor))
        }
        [major] if !major.is_empty() => Some(format!("match(r_base, \"<{}\")", major)),
        _ => None,
    }
}

/// Render the recipe, turning the `SUGGEST <spec>` entries of `run` into
/// comments (YAML comments cannot be expressed in the recipe model).
fn format_cran_recipe_with_suggests(recipe: &serialize::Recipe) -> String {
    let recipe_str = format!("{}", recipe);
    let mut final_recipe = String::new();
    for line in recipe_str.lines() {
        if let Some(spec) = line.trim_start().strip_prefix("- SUGGEST ") {
            // Suggested dependencies are kept as comments so that packagers
            // can promote the ones their tests actually need.
            let indent = &line[..line.len() - line.trim_start().len()];
            final_recipe.push_str(&format!("{indent}# - {spec}  # suggested\n"));
        } else {
            final_recipe.push_str(&format!("{}\n", line));
        }
    }
    final_recipe
}

/// Fetch the metadata of `package` from the R-universe API of `universe`.
pub async fn fetch_package_info(package: &str, universe: &str) -> miette::Result<PackageInfo> {
    reqwest::get(&format!(
        "https://{universe}.r-universe.dev/api/packages/{package}"
    ))
    .await
    .into_diagnostic()?
    .json::<PackageInfo>()
    .await
    .into_diagnostic()
}

/// Turn R-universe package metadata into a recipe.
///
/// `sha256` is the checksum of the CRAN tarball when it could be computed;
/// otherwise the MD5 reported by R-universe is used. `maintainers` fills
/// `extra.recipe-maintainers` (a placeholder is used when empty). Also returns
/// the R packages the recipe depends on, for `--tree`.
fn package_info_to_recipe(
    info: &PackageInfo,
    sha256: Option<String>,
    maintainers: &[String],
) -> (serialize::Recipe, HashSet<String>) {
    let mut recipe = serialize::Recipe::default();

    recipe
        .context
        .insert("version".to_string(), info.Version.clone());
    recipe
        .context
        .insert("cran_mirror".to_string(), CRAN_MIRROR.to_string());

    recipe.package.name = format_r_package(&info.Package, None);
    // CRAN allows `-` in versions (e.g. `0.7-5.1`), conda does not; conda-forge
    // maps it to `_`.
    recipe.package.version = if info.Version.contains('-') {
        "${{ version | replace(\"-\", \"_\") }}".to_string()
    } else {
        "${{ version }}".to_string()
    };

    // CRAN moves superseded versions to `Archive/<pkg>/`, so list that as a
    // fallback mirror.
    let tarball = format!("{}_${{{{ version }}}}.tar.gz", info.Package);
    recipe.source.push(
        UrlSourceElement {
            url: vec![
                format!("${{{{ cran_mirror }}}}/src/contrib/{tarball}"),
                format!(
                    "${{{{ cran_mirror }}}}/src/contrib/Archive/{}/{tarball}",
                    info.Package
                ),
            ],
            md5: if sha256.is_none() {
                info.MD5sum.clone()
            } else {
                None
            },
            sha256,
        }
        .into(),
    );

    recipe.build.number = "0".to_string();

    // Whether the package contains code that has to be compiled. Packages that
    // declare `LinkingTo` dependencies also compile against those headers, so
    // they need a compiler even if `NeedsCompilation` is not set to `yes`.
    let mut needs_compilation = info.NeedsCompilation == "yes";
    // Packages that suggest `testthat` ship their test suite in
    // `tests/testthat.R` by convention.
    let mut has_testthat = false;

    // `r-base` is always listed without a version pin; instead a minimum-R
    // constraint from `Depends: R (>= x.y.z)` is expressed as a `skip`
    // condition so variant selectors (r_base) control it at solve time.
    let r_base = "r-base".to_string();
    let mut host = Vec::new();
    let mut run = Vec::new();

    let mut remaining_deps = HashSet::new();
    for dep in info._dependencies.iter() {
        if dep.package == "R" {
            if let Some(ver) = &dep.version {
                recipe.build.skip = r_dep_version_to_skip(ver);
            }
            continue;
        }

        // skip builtins (these ship as part of `r-base`)
        if R_BUILTINS.contains(&dep.package.as_str()) {
            continue;
        }

        if dep.role == "LinkingTo" {
            // Headers needed at build time; pulls in a compiler.
            host.push(format_r_package(&dep.package, dep.version.as_ref()));
            needs_compilation = true;
            remaining_deps.insert(dep.package.clone());
        } else if dep.role == "Imports" || dep.role == "Depends" {
            let spec = format_r_package(&dep.package, dep.version.as_ref());
            host.push(spec.clone());
            run.push(spec);
            remaining_deps.insert(dep.package.clone());
        } else if dep.role == "Suggests" {
            has_testthat |= dep.package == "testthat";
            run.push(format!(
                "SUGGEST {}",
                format_r_package(&dep.package, dep.version.as_ref())
            ));
        }
    }

    recipe.requirements.host = std::iter::once(r_base.clone())
        .chain(host)
        .unique()
        .map(Requirement::from)
        .collect();
    recipe.requirements.run = std::iter::once(r_base)
        .chain(run)
        .unique()
        .map(Requirement::from)
        .collect();

    if needs_compilation {
        // Compiled packages need a toolchain, `cross-r-base` for cross builds,
        // and rpaths so the linker can find R's shared libraries.
        recipe.requirements.build = vec![
            cross_r_base_requirement(),
            "${{ compiler('c') }}".into(),
            "${{ compiler('cxx') }}".into(),
            "make".into(),
        ];
        recipe.build.dynamic_linking = Some(serialize::DynamicLinking {
            rpaths: vec!["lib/R/lib/".to_string(), "lib/".to_string()],
        });
        recipe.build.script = compiled_build_script();
    } else {
        // Pure-R packages are architecture independent.
        recipe.build.noarch = Some("generic".to_string());
        recipe.build.script = R_CMD_INSTALL.into();
    }

    if let Some(url) = &info.URL {
        let url = url
            .split_once(',')
            .map_or(url.as_str(), |(first, _)| first)
            .trim();
        recipe.about.homepage = Some(url.to_string());
    }

    recipe.about.summary = Some(info.Title.clone());
    // Trailing whitespace would force the description into a quoted scalar
    // instead of a readable `|-` block.
    recipe.about.description = Some(
        info.Description
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let (license, license_files) = map_license(&info.License);
    recipe.about.license = license;
    recipe.about.license_file = license_files;
    recipe.about.repository = Some(
        info._devurl
            .clone()
            .unwrap_or_else(|| info._upstream.clone()),
    );
    if let Some(docs) = info._pkgdown.as_ref().or(info._pkgdocs.as_ref())
        && Url::parse(docs).is_ok()
    {
        recipe.about.documentation = Some(docs.clone());
    }

    recipe.tests.push(Test::R(RTest {
        r: RTestInner {
            libraries: vec![info.Package.clone()],
        },
    }));
    if has_testthat {
        recipe.tests.push(Test::Script(ScriptTest {
            files: ScriptTestFiles {
                source: vec!["tests/".to_string()],
            },
            requirements: ScriptTestRequirements {
                run: vec!["r-testthat".to_string()],
            },
            script: vec![
                r#"Rscript -e "testthat::test_file('tests/testthat.R', stop_on_failure=TRUE)""#
                    .to_string(),
            ],
        }));
    }

    recipe.extra.recipe_maintainers = if maintainers.is_empty() {
        vec![DEFAULT_MAINTAINER.to_string()]
    } else {
        maintainers.to_vec()
    };

    (recipe, remaining_deps)
}

/// Fetch the package metadata, build the recipe and, when `with_description`
/// is set, also return the package's DESCRIPTION file.
async fn build_cran_recipe_and_deps(
    package: &str,
    universe: Option<&str>,
    maintainers: &[String],
    with_description: bool,
) -> miette::Result<(serialize::Recipe, HashSet<String>, Option<String>)> {
    let universe = universe.unwrap_or("cran");
    tracing::info!("Generating R recipe for {}", package);
    let package_info = fetch_package_info(package, universe).await?;

    // Download the tarball CRAN currently serves to hash it (and to read its
    // DESCRIPTION). This is best-effort: in restricted environments (e.g.
    // WASM/browser with no CORS on cran.r-project.org) we fall back to the MD5
    // the R-universe API gave us.
    let url = Url::parse(&format!("{CRAN_MIRROR}/src/contrib/{}", package_info._file))
        .expect("Failed to parse URL");
    let (sha256, description) = match fetch_package_tarball(&url).await {
        Ok(bytes) => {
            let sha256 = Some(hex::encode(compute_bytes_digest::<Sha256>(&bytes)));
            let description = if with_description {
                match extract_description(&bytes) {
                    Ok(Some(description)) => Some(description),
                    Ok(None) => {
                        tracing::warn!("No DESCRIPTION file found in {}", package_info._file);
                        None
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read DESCRIPTION from {}: {}",
                            package_info._file,
                            e
                        );
                        None
                    }
                }
            } else {
                None
            };
            (sha256, description)
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch {}: {} — falling back to MD5 from R-universe.",
                package_info._file,
                e
            );
            (None, None)
        }
    };

    let (recipe, remaining_deps) = package_info_to_recipe(&package_info, sha256, maintainers);
    Ok((recipe, remaining_deps, description))
}

/// Generate a CRAN recipe for `package` and return the YAML as a string.
pub async fn generate_r_recipe_string(
    package: &str,
    universe: Option<&str>,
) -> miette::Result<String> {
    let (recipe, _remaining_deps, _description) =
        build_cran_recipe_and_deps(package, universe, &[], false).await?;
    Ok(format_cran_recipe_with_suggests(&recipe))
}

#[cfg(not(target_arch = "wasm32"))]
#[async_recursion::async_recursion]
/// Generate a CRAN recipe using `CranOpts` and either print it or write it to disk.
///
/// If `opts.write` is true, the recipe is written to a folder named after the
/// package. Otherwise, the YAML is printed to stdout. When `tree` is enabled,
/// dependencies are recursively generated if they don't already exist locally.
pub async fn generate_r_recipe(opts: &CranOpts) -> miette::Result<()> {
    let (recipe, remaining_deps, description) = build_cran_recipe_and_deps(
        &opts.package,
        opts.universe.as_deref(),
        &opts.maintainers,
        opts.staged_recipes,
    )
    .await?;

    let mut final_recipe = format_cran_recipe_with_suggests(&recipe);
    if let Some(description) = description {
        final_recipe = append_description_comment(&final_recipe, &description);
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

    /// Verbatim responses of `https://cran.r-universe.dev/api/packages/<pkg>`.
    fn fixture(name: &str) -> PackageInfo {
        let json = match name {
            "tinkr" => include_str!("../test-data/cran/tinkr.json"),
            "gmp" => include_str!("../test-data/cran/gmp.json"),
            other => panic!("unknown fixture {other}"),
        };
        serde_json::from_str(json).expect("fixture must deserialize as PackageInfo")
    }

    /// Pure-R package: `noarch: generic`, single-line script, `r:` test plus a
    /// testthat test, dev repository and pkgdown site from R-universe.
    #[test]
    fn tinkr_noarch_with_testthat() {
        let info = fixture("tinkr");
        let (recipe, deps) = package_info_to_recipe(
            &info,
            Some("425bc04af76483b8cf713ad141bdebb963cf54dd363fbdbee3a709820ec4d23e".to_string()),
            &[],
        );
        assert!(deps.contains("commonmark"));
        assert!(deps.contains("R6"));
        assert!(!deps.contains("testthat"), "Suggests are not recursed into");
        insta::assert_snapshot!(format_cran_recipe_with_suggests(&recipe));
    }

    /// Compiled package with a `-` in its version: compilers, cross-r-base,
    /// rpaths, platform-split script, no testthat test, explicit maintainers.
    #[test]
    fn gmp_compiled_with_dash_version() {
        let info = fixture("gmp");
        let (recipe, deps) = package_info_to_recipe(
            &info,
            None,
            &["octocat".to_string(), "conda-forge/r".to_string()],
        );
        assert!(deps.is_empty(), "gmp only depends on base R packages");
        insta::assert_snapshot!(format_cran_recipe_with_suggests(&recipe));
    }

    /// The SUGGEST marker must only be recognised as a list item, not inside
    /// free text such as the description.
    #[test]
    fn only_suggested_dependency_lines_are_commented_out() {
        let mut info = fixture("tinkr");
        info.Description = "Does things as SUGGESTED by the SUGGEST list.".to_string();
        let (recipe, _) = package_info_to_recipe(&info, None, &[]);
        let yaml = format_cran_recipe_with_suggests(&recipe);
        assert!(
            yaml.contains("  description: Does things as SUGGESTED by the SUGGEST list.\n"),
            "{yaml}"
        );
        assert!(yaml.contains("    # - r-knitr  # suggested\n"), "{yaml}");
    }

    /// Build an in-memory `.tar.gz` with the given `(path, contents)` entries.
    fn tarball_with(files: &[(&str, &str)]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extracts_only_the_top_level_description() {
        let tarball = tarball_with(&[
            ("tinkr/R/tinkr.R", "NULL\n"),
            ("tinkr/inst/extdata/DESCRIPTION", "Package: not-this-one\n"),
            ("tinkr/DESCRIPTION", "Package: tinkr\nVersion: 0.3.1\n"),
        ]);
        assert_eq!(
            extract_description(&tarball).unwrap().as_deref(),
            Some("Package: tinkr\nVersion: 0.3.1\n")
        );

        let without = tarball_with(&[("tinkr/R/tinkr.R", "NULL\n")]);
        assert_eq!(extract_description(&without).unwrap(), None);
    }

    #[test]
    fn description_is_appended_as_a_comment_block() {
        let recipe = "package:\n  name: r-tinkr\n";
        let description =
            "Package: tinkr\nTitle: Cast '(R)Markdown' Files\n    to 'XML'  \n\nLicense: GPL-3\n";
        insta::assert_snapshot!(append_description_comment(recipe, description));
    }

    #[test]
    fn test_r_dep_version_to_skip() {
        assert_eq!(
            r_dep_version_to_skip(">= 4.1.0").as_deref(),
            Some("match(r_base, \"<4.1\")")
        );
        assert_eq!(
            r_dep_version_to_skip(">=3.5").as_deref(),
            Some("match(r_base, \"<3.5\")")
        );
        assert_eq!(
            r_dep_version_to_skip("> 4").as_deref(),
            Some("match(r_base, \"<4\")")
        );
        assert_eq!(r_dep_version_to_skip(""), None);
    }

    #[test]
    fn test_license_mapping() {
        // Helper to build the expected `${{ PREFIX }}`-relative path to a
        // license shipped by `r-base`.
        let bundled = |name: &str| format!("{R_LICENSE_DIR}{name}");

        let test_cases = vec![
            // Simple cases: standard licenses gain a reference to the copy that
            // `r-base` ships.
            ("GPL-3", "GPL-3.0-only", vec![bundled("GPL-3")]),
            ("MIT", "MIT", vec![bundled("MIT")]),
            (
                "Apache License 2.0",
                "Apache-2.0",
                vec![bundled("Apache-2.0")],
            ),
            // Cases with `file LICENSE`: the bundled license comes first,
            // followed by the package-local file.
            (
                "GPL-3 + file LICENSE",
                "GPL-3.0-only",
                vec![bundled("GPL-3"), "LICENSE".to_string()],
            ),
            (
                "MIT + file LICENCE",
                "MIT",
                vec![bundled("MIT"), "LICENCE".to_string()],
            ),
            (
                "MIT + file LICENSE",
                "MIT",
                vec![bundled("MIT"), "LICENSE".to_string()],
            ),
            // Compound licenses
            (
                "GPL-2 | MIT",
                "GPL-2.0-only OR MIT",
                vec![bundled("GPL-2"), bundled("MIT")],
            ),
            (
                "Apache License 2.0 | file LICENSE",
                "Apache-2.0",
                vec![bundled("Apache-2.0"), "LICENSE".to_string()],
            ),
            // Version ranges (`*-or-later` maps to the base version file)
            ("GPL (>= 2)", "GPL-2.0-or-later", vec![bundled("GPL-2")]),
            ("LGPL (>= 3)", "LGPL-3.0-or-later", vec![bundled("LGPL-3")]),
            // More complex cases
            (
                "GPL (>= 2) | BSD_3_clause + file LICENSE",
                "GPL-2.0-or-later OR BSD-3-Clause",
                vec![
                    bundled("GPL-2"),
                    bundled("BSD_3_clause"),
                    "LICENSE".to_string(),
                ],
            ),
            (
                "LGPL-2.1 | file LICENSE",
                "LGPL-2.1-only",
                vec![bundled("LGPL-2.1"), "LICENSE".to_string()],
            ),
            (
                "GPL (>= 2.0) | file LICENCE",
                "GPL-2.0-or-later",
                vec![bundled("GPL-2"), "LICENCE".to_string()],
            ),
            // Cases without a bundled license file
            ("Unlimited", "Unlimited", vec![]),
            ("GPL (>= 2.15.1)", "GPL (>= 2.15.1)", vec![]),
            // Creative Commons licenses
            ("CC BY-SA 4.0", "CC-BY-SA-4.0", vec![]),
            ("CC BY-NC-ND 3.0 US", "CC BY-NC-ND 3.0 US", vec![]), // This one doesn't have a direct SPDX mapping
            // Multiple licenses with file
            (
                "GPL-2 | GPL-3 | MIT + file LICENSE",
                "GPL-2.0-only OR GPL-3.0-only OR MIT",
                vec![
                    bundled("GPL-2"),
                    bundled("GPL-3"),
                    bundled("MIT"),
                    "LICENSE".to_string(),
                ],
            ),
        ];

        for (input, expected_license, expected_files) in test_cases {
            let (mapped_license, license_files) = map_license(input);
            assert_eq!(
                mapped_license.as_deref(),
                Some(expected_license),
                "Failed for input: {}",
                input
            );
            assert_eq!(license_files, expected_files, "Failed for input: {}", input);
        }
    }
}
