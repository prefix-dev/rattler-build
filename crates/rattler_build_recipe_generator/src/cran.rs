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
use crate::tarball;
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

    /// Shape the recipe for a conda-forge staged-recipes submission: download
    /// through conda-forge's `cran_mirror` variant, leave out the `skip` for a
    /// minimum R version, require the C standard library next to the
    /// compilers and append the package's DESCRIPTION file for reviewers
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

/// What the CRAN source tarball tells us beyond the R-universe metadata.
#[derive(Debug, Default)]
struct CranTarball {
    /// SHA256 of the downloaded tarball; only recorded when the archive could
    /// be read and came from the host the recipe's source URLs reference (the
    /// CRAN mirror).
    sha256: Option<String>,
    /// The package's `DESCRIPTION` file (`<package>/DESCRIPTION`).
    description: Option<String>,
    /// Whether the package ships the testthat runner, `tests/testthat.R`.
    has_testthat_runner: bool,
}

impl CranTarball {
    fn parse(bytes: &[u8]) -> miette::Result<Self> {
        // One decompression pass for everything the tarball can tell us. A
        // failure here means the bytes are not a source archive at all, so
        // nothing about them — the checksum included — can be trusted.
        let mut files = tarball::find_archive_files(bytes, &["DESCRIPTION", "tests/testthat.R"])?;
        Ok(Self {
            sha256: Some(hex::encode(compute_bytes_digest::<Sha256>(bytes))),
            description: files.remove("DESCRIPTION"),
            has_testthat_runner: files.contains_key("tests/testthat.R"),
        })
    }
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
        out.push_str(&serialize::comment_text(line));
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

/// The build command of pure-R (`noarch: generic`) packages, as conda-forge
/// writes it. `${{ R }}` is replaced by the path of the host prefix's R. Such
/// a package is never cross-compiled, so that R can always run.
const NOARCH_R_CMD_INSTALL: &str = "${{ R }} CMD INSTALL --build .";

/// The build command of compiled packages. `R` is left for the shell to find
/// at build time: when cross-compiling, the activation of `cross-r-base` puts
/// the R of the build platform first. `${{ R }}` is replaced before any
/// activation happens, and the R of the host prefix cannot run there.
const COMPILED_R_CMD_INSTALL: &str = "R CMD INSTALL --build .";

/// Build script for compiled packages: [`COMPILED_R_CMD_INSTALL`] with
/// `R_ARGS` passed through (spelled per shell), so that recipe authors can
/// inject e.g. `--configure-args`. Pure-R packages have no configure step.
fn compiled_build_script() -> Script {
    Script::Steps(vec![ScriptStep {
        condition: "win".to_string(),
        then: format!("{COMPILED_R_CMD_INSTALL} %R_ARGS%"),
        otherwise: Some(format!("{COMPILED_R_CMD_INSTALL} ${{R_ARGS}}")),
    }])
}

/// `cross-r-base` is required to cross-compile; `r_base` is the conda-forge
/// variant key pinning the R version. The variant names a `major.minor`
/// series (e.g. `4.5`), so `.*` is needed to match its patch releases.
fn cross_r_base_requirement() -> Requirement {
    Requirement::Conditional {
        condition: "build_platform != target_platform".to_string(),
        then: vec!["cross-r-base ${{ r_base }}.*".to_string()],
    }
}

/// The CRAN mirror the generated recipes download from: the CDN that
/// conda-forge also pins its `cran_mirror` variant to.
const CRAN_MIRROR: &str = "https://cloud.r-project.org";

/// Choices the caller makes about the shape of the generated recipe.
#[derive(Debug, Default)]
pub struct RecipeOptions {
    /// GitHub handles to list under `extra.recipe-maintainers`.
    pub maintainers: Vec<String>,
    /// Follow the conventions of a conda-forge staged-recipes submission:
    /// download through conda-forge's `cran_mirror` variant rather than from a
    /// mirror named in the recipe. rattler-build itself knows no `cran_mirror`,
    /// so such a recipe needs a variant config that defines one. Also leaves
    /// out the `skip` for a minimum R version, which the R versions conda-forge
    /// builds for make unnecessary, requires `${{ stdlib("c") }}` next to the
    /// compilers of a compiled package, as conda-forge's linter demands, and
    /// appends the package's DESCRIPTION file to the recipe as a comment block.
    pub staged_recipes: bool,
}

/// Placeholder maintainer when none is given on the command line (the same
/// convention grayskull uses).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const DEFAULT_MAINTAINER: &str = "AddYourGitHubIdHere";

/// The maintainers to list for a recipe generated from the command line:
/// the ones given, or a placeholder reminding the author to fill them in.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn cli_maintainers(given: &[String]) -> Vec<String> {
    if given.is_empty() {
        vec![DEFAULT_MAINTAINER.to_string()]
    } else {
        given.to_vec()
    }
}

/// The R version that a `Depends: R (>= x.y.z)` constraint asks for at least,
/// as `(major, minor)`. Only lower bounds (`>=` and `>`) with numeric
/// components are understood; anything else gives `None`. R separates the
/// components of a version with `.` or `-`.
fn r_minimum_version(constraint: &str) -> Option<(u64, Option<u64>)> {
    let constraint = constraint.trim();
    let version = constraint
        .strip_prefix(">=")
        .or_else(|| constraint.strip_prefix('>'))?;
    let mut components = version.trim().split(['.', '-']);
    let major = components.next()?.parse().ok()?;
    let minor = components.next().map(str::parse).transpose().ok()?;
    Some((major, minor))
}

/// The rattler-build `skip` expression for an R older than `major.minor`.
fn r_skip_condition((major, minor): (u64, Option<u64>)) -> String {
    match minor {
        Some(minor) => format!("match(r_base, \"<{major}.{minor}\")"),
        None => format!("match(r_base, \"<{major}\")"),
    }
}

/// Fetch the metadata of `package` from the R-universe API of `universe`.
async fn fetch_package_info(
    client: &reqwest::Client,
    package: &str,
    universe: &str,
) -> miette::Result<PackageInfo> {
    client
        .get(format!(
            "https://{universe}.r-universe.dev/api/packages/{package}"
        ))
        .send()
        .await
        .into_diagnostic()?
        .error_for_status()
        .into_diagnostic()?
        .json::<PackageInfo>()
        .await
        .into_diagnostic()
}

/// Where the CRAN mirror `mirror` serves the tarball named `file`: CRAN moves
/// superseded versions to `Archive/<package>/`, so that is the fallback.
fn cran_source_urls(mirror: &str, package: &str, file: &str) -> Vec<String> {
    vec![
        format!("{mirror}/src/contrib/{file}"),
        format!("{mirror}/src/contrib/Archive/{package}/{file}"),
    ]
}

/// The URLs that may serve the tarball named `file`, in the order to try.
/// CRAN hosts its own files; every other universe serves them through
/// r-universe.dev.
fn tarball_urls(universe: &str, package: &str, file: &str) -> Vec<String> {
    if universe == "cran" {
        cran_source_urls(CRAN_MIRROR, package, file)
    } else {
        vec![format!(
            "https://{universe}.r-universe.dev/src/contrib/{file}"
        )]
    }
}

/// Read the package's source tarball from the first of `urls` that serves a
/// readable archive.
async fn fetch_tarball(client: &reqwest::Client, urls: &[String]) -> Option<CranTarball> {
    for url in urls {
        match tarball::download(client, url.as_str()).await {
            Ok(bytes) => match CranTarball::parse(&bytes) {
                Ok(tarball) => return Some(tarball),
                Err(e) => tracing::warn!("{url} is not a readable source archive: {e}"),
            },
            Err(e) => tracing::debug!("Could not download {url}: {e}"),
        }
    }
    None
}

/// Fetch the metadata of `package` and, best-effort, its tarball: `None` when
/// it could not be downloaded, e.g. in restricted environments (WASM/browser
/// with no CORS on cran.r-project.org).
async fn fetch_package(
    client: &reqwest::Client,
    package: &str,
    universe: &str,
) -> miette::Result<(PackageInfo, Option<CranTarball>)> {
    tracing::info!("Generating R recipe for {}", package);
    let info = fetch_package_info(client, package, universe).await?;

    let urls = tarball_urls(universe, &info.Package, &info._file);
    let mut tarball = fetch_tarball(client, &urls).await;
    match tarball.as_mut() {
        None => tracing::warn!(
            "Could not read {} from {} — the recipe will not contain a checksum; add the sha256 by hand.",
            info._file,
            urls.join(" or ")
        ),
        // The recipe's source URLs point at the CRAN mirror, and r-universe
        // rebuilds tarballs, so its hash would not match what the recipe
        // downloads.
        Some(tarball) if universe != "cran" => {
            tarball.sha256 = None;
            tracing::warn!(
                "The package comes from the {universe} universe: the recipe's source URLs and \
                 checksum need manual attention."
            );
        }
        Some(_) => {}
    }

    Ok((info, tarball))
}

/// Turn R-universe package metadata (plus what the tarball told us, when it
/// could be downloaded) into a recipe.
///
/// `options.maintainers` fills `extra.recipe-maintainers` (omitted when empty).
/// Also returns the R packages the recipe depends on, for `--tree`.
fn package_info_to_recipe(
    info: &PackageInfo,
    tarball: Option<&CranTarball>,
    options: &RecipeOptions,
) -> (serialize::Recipe, HashSet<String>) {
    let mut recipe = serialize::Recipe::default();

    recipe
        .context
        .insert("version".to_string(), info.Version.clone());

    recipe.package.name = format_r_package(&info.Package, None);
    // CRAN allows `-` in versions (e.g. `0.7-5.1`), conda does not; conda-forge
    // maps it to `_`.
    recipe.package.version = if info.Version.contains('-') {
        "${{ version | replace(\"-\", \"_\") }}".to_string()
    } else {
        "${{ version }}".to_string()
    };

    // A staged-recipes submission downloads through conda-forge's pinned
    // `cran_mirror`; anywhere else the recipe has to name a mirror itself,
    // because rattler-build defines no such variable.
    let mirror = if options.staged_recipes {
        // Reading the variant would otherwise make it part of the package's
        // variant, so that the mirror a package came from would change its
        // build string.
        recipe.build.variant.ignore_keys = vec!["cran_mirror".to_string()];
        "${{ cran_mirror }}"
    } else {
        CRAN_MIRROR
    };
    let file_name = format!("{}_${{{{ version }}}}.tar.gz", info.Package);
    recipe.source.push(
        UrlSourceElement {
            url: cran_source_urls(mirror, &info.Package, &file_name),
            sha256: tarball.and_then(|tarball| tarball.sha256.clone()),
            md5: None,
        }
        .into(),
    );

    recipe.build.number = "0".to_string();

    // Whether the package contains code that has to be compiled. Packages that
    // declare `LinkingTo` dependencies also compile against those headers, so
    // they need a compiler even if `NeedsCompilation` is not set to `yes`.
    let mut needs_compilation = info.NeedsCompilation == "yes";

    // `r-base` is always listed without a version pin; instead a minimum-R
    // constraint from `Depends: R (>= x.y.z)` is expressed as a `skip`
    // condition so variant selectors (r_base) control it at solve time. Such
    // recipes therefore need an `r_base` variant: `match()` on an undefined
    // variable is true, so without one the output is skipped.
    let r_base = "r-base".to_string();
    let mut host = Vec::new();
    let mut run = Vec::new();
    let mut suggested = Vec::new();
    // The testthat test dependency keeps whatever constraint upstream declares
    // in its Suggests entry.
    let mut testthat_requirement = "r-testthat".to_string();

    let mut remaining_deps = HashSet::new();
    for dep in info._dependencies.iter() {
        if dep.package == "R" {
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
            if dep.package == "testthat" {
                testthat_requirement = format_r_package(&dep.package, dep.version.as_ref());
            }
            suggested.push(format_r_package(&dep.package, dep.version.as_ref()));
        }
    }

    if !options.staged_recipes {
        // Several entries may bound R; the strictest lower bound decides.
        recipe.build.skip = info
            ._dependencies
            .iter()
            .filter(|dep| dep.package == "R")
            .filter_map(|dep| r_minimum_version(dep.version.as_deref()?))
            .max()
            .map(r_skip_condition);
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
        // Suggested dependencies follow the real ones, as comments.
        .chain(suggested.into_iter().map(Requirement::suggested))
        .collect();

    if needs_compilation {
        // Compiled packages need a toolchain, `cross-r-base` for cross builds,
        // and rpaths so the linker can find R's shared libraries.
        let mut build = vec![
            cross_r_base_requirement(),
            "${{ compiler(\"c\") }}".into(),
            "${{ compiler(\"cxx\") }}".into(),
        ];
        if options.staged_recipes {
            // conda-forge's linter requires the C standard library next to a
            // compiler. rattler-build itself knows no `c_stdlib`, so a plain
            // build could not render it.
            build.push("${{ stdlib(\"c\") }}".into());
        }
        build.push("make".into());
        recipe.requirements.build = build;
        recipe.build.dynamic_linking = Some(serialize::DynamicLinking {
            rpaths: vec!["lib/R/lib/".to_string(), "lib/".to_string()],
        });
        recipe.build.script = compiled_build_script();
    } else {
        // Pure-R packages are architecture independent.
        recipe.build.noarch = Some("generic".to_string());
        recipe.build.script = NOARCH_R_CMD_INSTALL.into();
    }

    // `URL:` lists one or more URLs separated by commas and/or whitespace; the
    // first one is the package's home page by convention.
    if let Some(url) = info.URL.as_deref().and_then(|urls| {
        urls.split(|c: char| c == ',' || c.is_whitespace())
            .find(|url| !url.is_empty())
    }) {
        recipe.about.homepage = Some(url.to_string());
    }

    recipe.about.summary = Some(info.Title.clone());
    // Trailing whitespace would force the description into a quoted scalar
    // instead of a readable `|-` block.
    recipe.about.description = Some(info.Description.lines().map(str::trim_end).join("\n"));
    let (license, license_files) = map_license(&info.License);
    recipe.about.license = license;
    recipe.about.license_file = license_files;
    // `_devurl` is the development repository R-universe detected, if any;
    // `_upstream` is the CRAN mirror on GitHub and is always usable.
    recipe.about.repository = Some(
        info._devurl
            .as_deref()
            .filter(|url| Url::parse(url).is_ok())
            .unwrap_or(&info._upstream)
            .to_string(),
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
    // Run the package's own test suite when it ships the testthat runner
    // (suggesting testthat alone does not guarantee `tests/testthat.R` exists).
    if tarball.is_some_and(|tarball| tarball.has_testthat_runner) {
        recipe.tests.push(Test::Script(ScriptTest {
            files: ScriptTestFiles {
                source: vec!["tests/".to_string()],
            },
            requirements: ScriptTestRequirements {
                run: vec![testthat_requirement],
            },
            script: vec![
                r#"Rscript -e "testthat::test_file('tests/testthat.R', stop_on_failure=TRUE)""#
                    .to_string(),
            ],
        }));
    }

    recipe.extra.recipe_maintainers = options.maintainers.clone();

    (recipe, remaining_deps)
}

/// A rendered recipe plus what the recursive `--tree` mode and the command
/// line need alongside it.
struct GeneratedRecipe {
    /// Only `yaml` is read on wasm32, where the command-line front end that
    /// consumes the other fields is not built.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    package_name: String,
    yaml: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    remaining_deps: HashSet<String>,
}

/// Fetch `package` from `universe` (CRAN when `None`) and render its recipe.
async fn fetch_and_render(
    client: &reqwest::Client,
    package: &str,
    universe: Option<&str>,
    options: &RecipeOptions,
) -> miette::Result<GeneratedRecipe> {
    let (info, tarball) = fetch_package(client, package, universe.unwrap_or("cran")).await?;
    let (recipe, remaining_deps) = package_info_to_recipe(&info, tarball.as_ref(), options);
    let mut yaml = recipe.to_string();
    if options.staged_recipes {
        match tarball.and_then(|tarball| tarball.description) {
            Some(description) => yaml = append_description_comment(&yaml, &description),
            None => tracing::warn!("No DESCRIPTION file available to append to the recipe"),
        }
    }
    Ok(GeneratedRecipe {
        package_name: recipe.package.name,
        yaml,
        remaining_deps,
    })
}

/// Generate a CRAN recipe for `package` and return the YAML as a string.
pub async fn generate_r_recipe_string(
    package: &str,
    universe: Option<&str>,
) -> miette::Result<String> {
    let client = reqwest::Client::new();
    Ok(
        fetch_and_render(&client, package, universe, &RecipeOptions::default())
            .await?
            .yaml,
    )
}

/// Generate a CRAN recipe using `CranOpts` and either print it or write it to disk.
///
/// If `opts.write` is true, the recipe is written to a folder named after the
/// package. Otherwise, the YAML is printed to stdout. When `tree` is enabled,
/// dependencies are recursively generated if they don't already exist locally.
#[cfg(not(target_arch = "wasm32"))]
pub async fn generate_r_recipe(opts: &CranOpts) -> miette::Result<()> {
    // One client for the whole (possibly recursive) run, so that `--tree`
    // reuses its connections.
    let client = reqwest::Client::new();
    generate_r_recipe_with_client(&client, opts).await
}

#[cfg(not(target_arch = "wasm32"))]
#[async_recursion::async_recursion]
async fn generate_r_recipe_with_client(
    client: &reqwest::Client,
    opts: &CranOpts,
) -> miette::Result<()> {
    let generated = fetch_and_render(
        client,
        &opts.package,
        opts.universe.as_deref(),
        &RecipeOptions {
            maintainers: cli_maintainers(&opts.maintainers),
            staged_recipes: opts.staged_recipes,
        },
    )
    .await?;

    if opts.write {
        write_recipe(&generated.package_name, &generated.yaml).into_diagnostic()?;
    } else {
        print!("{}", generated.yaml);
    }

    if opts.tree {
        for dep in generated.remaining_deps {
            let r_package = format_r_package(&dep, None);

            if !PathBuf::from(r_package).exists() {
                let opts = CranOpts {
                    package: dep,
                    ..opts.clone()
                };
                generate_r_recipe_with_client(client, &opts).await?;
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
        let tarball = CranTarball {
            sha256: Some(
                "425bc04af76483b8cf713ad141bdebb963cf54dd363fbdbee3a709820ec4d23e".to_string(),
            ),
            has_testthat_runner: true,
            ..Default::default()
        };
        let options = RecipeOptions {
            maintainers: cli_maintainers(&[]),
            ..Default::default()
        };
        let (recipe, deps) = package_info_to_recipe(&info, Some(&tarball), &options);
        assert!(deps.contains("commonmark"));
        assert!(deps.contains("R6"));
        assert!(!deps.contains("testthat"), "Suggests are not recursed into");
        insta::assert_snapshot!(recipe.to_string());
    }

    /// Compiled package with a `-` in its version: compilers, cross-r-base,
    /// rpaths, platform-split script, no testthat test, explicit maintainers.
    #[test]
    fn gmp_compiled_with_dash_version() {
        let info = fixture("gmp");
        let options = RecipeOptions {
            maintainers: vec!["octocat".to_string(), "conda-forge/r".to_string()],
            ..Default::default()
        };
        let (recipe, deps) = package_info_to_recipe(&info, None, &options);
        assert!(deps.is_empty(), "gmp only depends on base R packages");
        insta::assert_snapshot!(recipe.to_string());
    }

    /// A compiled package may be cross-compiled, so its script must leave the
    /// choice of R to the shell (see [`COMPILED_R_CMD_INSTALL`]). The snapshot
    /// pins the same text; this guards against accepting a changed one.
    #[test]
    fn compiled_build_scripts_do_not_render_the_path_of_r() {
        let script = serde_yaml::to_string(&compiled_build_script()).unwrap();
        assert!(script.contains("R CMD INSTALL --build ."), "{script}");
        assert!(!script.contains("${{"), "{script}");
    }

    /// rattler-build knows no `cran_mirror`, so only a staged-recipes
    /// submission — which conda-forge's pinning covers — may refer to it.
    #[test]
    fn only_staged_recipes_refer_to_the_cran_mirror_variant() {
        let info = fixture("tinkr");

        let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
        let yaml = recipe.to_string();
        assert!(
            yaml.contains(
                "    - https://cloud.r-project.org/src/contrib/tinkr_${{ version }}.tar.gz\n"
            ),
            "{yaml}"
        );
        assert!(!yaml.contains("cran_mirror"), "{yaml}");

        let staged = RecipeOptions {
            staged_recipes: true,
            ..Default::default()
        };
        let (recipe, _) = package_info_to_recipe(&info, None, &staged);
        let yaml = recipe.to_string();
        assert!(
            yaml.contains("    - ${{ cran_mirror }}/src/contrib/tinkr_${{ version }}.tar.gz\n"),
            "{yaml}"
        );
        assert!(
            yaml.contains(
                "    - ${{ cran_mirror }}/src/contrib/Archive/tinkr/tinkr_${{ version }}.tar.gz\n"
            ),
            "{yaml}"
        );
        // Referring to the variant must not put the mirror in the build hash.
        assert!(yaml.contains("      - cran_mirror\n"), "{yaml}");
        // The mirror comes from the variant config, not from `context`.
        assert!(!yaml.contains("  cran_mirror:"), "{yaml}");
    }

    /// tinkr depends on `R (>= 4.1.0)`; conda-forge only builds for R versions
    /// that satisfy such a bound, so a staged-recipes submission has no `skip`.
    #[test]
    fn only_staged_recipes_leave_out_the_minimum_r_skip() {
        let info = fixture("tinkr");

        let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
        assert_eq!(
            recipe.build.skip.as_deref(),
            Some("match(r_base, \"<4.1\")")
        );

        let staged = RecipeOptions {
            staged_recipes: true,
            ..Default::default()
        };
        let (recipe, _) = package_info_to_recipe(&info, None, &staged);
        assert_eq!(recipe.build.skip, None);
    }

    /// conda-forge's linter wants `stdlib("c")` next to a compiler; plain
    /// rattler-build cannot render it without a `c_stdlib` variant, so only a
    /// staged-recipes submission lists it, and only for a compiled package.
    #[test]
    fn only_staged_recipes_of_compiled_packages_require_the_c_stdlib() {
        let build_of = |name: &str, staged_recipes: bool| {
            let options = RecipeOptions {
                staged_recipes,
                ..Default::default()
            };
            package_info_to_recipe(&fixture(name), None, &options)
                .0
                .requirements
                .build
        };
        let stdlib = || Requirement::from("${{ stdlib(\"c\") }}");

        assert!(!build_of("gmp", false).contains(&stdlib()));
        assert_eq!(
            build_of("gmp", true),
            vec![
                cross_r_base_requirement(),
                "${{ compiler(\"c\") }}".into(),
                "${{ compiler(\"cxx\") }}".into(),
                stdlib(),
                "make".into(),
            ]
        );
        // Pure-R packages have no compiler, so nothing to pair it with.
        assert!(build_of("tinkr", true).is_empty());
    }

    /// A package may bound R more than once (`ppgm` does); the skip follows
    /// the strictest lower bound in whatever order they come, and ignores what
    /// is not a lower bound.
    #[test]
    fn the_strictest_minimum_r_decides_the_skip() {
        let skip_for = |constraints: &[&str]| {
            let mut info = fixture("tinkr");
            info._dependencies.retain(|dep| dep.package != "R");
            info._dependencies
                .extend(constraints.iter().map(|constraint| Dependency {
                    package: "R".to_string(),
                    version: Some(constraint.to_string()),
                    role: "Depends".to_string(),
                }));
            package_info_to_recipe(&info, None, &RecipeOptions::default())
                .0
                .build
                .skip
        };
        let skip = |version: &str| Some(format!("match(r_base, \"<{version}\")"));

        assert_eq!(skip_for(&[">= 4.4.0", ">= 2.10"]), skip("4.4"));
        assert_eq!(skip_for(&[">= 2.10", ">= 4.4.0"]), skip("4.4"));
        // Compared as numbers: 2.10 is newer than 2.9.
        assert_eq!(skip_for(&[">= 2.9", ">= 2.10"]), skip("2.10"));
        assert_eq!(skip_for(&["<= 4.5", ">= 4.1.0"]), skip("4.1"));
        assert_eq!(skip_for(&["> 4"]), skip("4"));
        assert_eq!(skip_for(&["< 5"]), None);
        assert_eq!(skip_for(&[]), None);
    }

    /// R-universe reports an unusable `_devurl` for some packages; the CRAN
    /// mirror in `_upstream` is always there to fall back on.
    #[test]
    fn repository_falls_back_to_upstream_for_an_unusable_devurl() {
        let mut info = fixture("tinkr");
        for devurl in [None, Some(String::new()), Some("not a url".to_string())] {
            info._devurl = devurl.clone();
            let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
            assert_eq!(
                recipe.about.repository.as_deref(),
                Some(info._upstream.as_str()),
                "{devurl:?}"
            );
        }
        info._devurl = Some("https://github.com/ropensci/tinkr".to_string());
        let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
        assert_eq!(
            recipe.about.repository.as_deref(),
            Some("https://github.com/ropensci/tinkr")
        );
    }

    /// CRAN's `URL:` field may separate several URLs with commas, spaces or
    /// newlines; the first one is the home page.
    #[test]
    fn homepage_is_the_first_url_of_the_url_field() {
        let mut info = fixture("tinkr");
        for urls in [
            "https://a.example, https://b.example",
            "https://a.example https://b.example",
            "https://a.example,\r\nhttps://b.example",
            "https://a.example\r\nhttps://b.example",
            "https://a.example,\nhttps://b.example",
            "  https://a.example",
        ] {
            info.URL = Some(urls.to_string());
            let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
            assert_eq!(
                recipe.about.homepage.as_deref(),
                Some("https://a.example"),
                "{urls:?}"
            );
        }
        info.URL = None;
        let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
        assert_eq!(recipe.about.homepage, None);
    }

    /// Library callers (py-rattler-build, the playground) get no `extra:` block
    /// unless they name maintainers; the placeholder is a CLI convenience.
    #[test]
    fn maintainers_are_only_defaulted_on_the_command_line() {
        let info = fixture("tinkr");
        let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
        assert!(recipe.extra.recipe_maintainers.is_empty());
        assert!(!recipe.to_string().contains("extra:"));

        assert_eq!(cli_maintainers(&[]), vec![DEFAULT_MAINTAINER.to_string()]);
        assert_eq!(
            cli_maintainers(&["octocat".to_string()]),
            vec!["octocat".to_string()]
        );
    }

    /// A package may suggest testthat without shipping `tests/testthat.R`
    /// (or the tarball may not have been downloaded at all).
    #[test]
    fn no_testthat_test_without_the_runner() {
        let info = fixture("tinkr");
        let without_runner = CranTarball::default();
        for tarball in [None, Some(&without_runner)] {
            let (recipe, _) = package_info_to_recipe(&info, tarball, &RecipeOptions::default());
            assert_eq!(recipe.tests.len(), 1, "only the `r:` test is expected");
            assert!(matches!(recipe.tests[0], Test::R(_)));
        }
    }

    /// Regression test for the former text-based SUGGEST marker: free text
    /// such as the description stays untouched, even when a line has the
    /// exact shape of a suggested-dependency entry.
    #[test]
    fn only_suggested_dependency_lines_are_commented_out() {
        let mut info = fixture("tinkr");
        info.Description =
            "Does things as SUGGESTED by:\n- SUGGEST mode for reviewers\n- other modes".to_string();
        let (recipe, _) = package_info_to_recipe(&info, None, &RecipeOptions::default());
        let yaml = recipe.to_string();
        assert!(
            yaml.contains("    - SUGGEST mode for reviewers\n"),
            "description bullet must stay untouched: {yaml}"
        );
        assert!(yaml.contains("    # - r-knitr  # suggested\n"), "{yaml}");
    }

    #[test]
    fn description_is_appended_as_a_comment_block() {
        let recipe = "package:\n  name: r-tinkr\n";
        let description =
            "Package: tinkr\nTitle: Cast '(R)Markdown' Files\n    to 'XML'  \n\nLicense: GPL-3\n";
        insta::assert_snapshot!(append_description_comment(recipe, description));
    }

    /// DESCRIPTION is free text; nothing in it may end its comment line and
    /// become part of the recipe.
    #[test]
    fn the_description_comment_adds_nothing_to_the_recipe() {
        let recipe = "package:\n  name: r-tinkr\n";
        let description = "\u{feff}Package: tinkr\nAuthor: A\u{2028}injected: 1\rother: 2\n";
        let commented = append_description_comment(recipe, description);
        assert_eq!(
            serde_yaml::from_str::<serde_yaml::Value>(&commented).unwrap(),
            serde_yaml::from_str::<serde_yaml::Value>(recipe).unwrap(),
            "{commented}"
        );
    }

    #[test]
    fn tarballs_come_from_the_requested_universe() {
        assert_eq!(
            tarball_urls("cran", "tinkr", "tinkr_0.3.1.tar.gz"),
            [
                "https://cloud.r-project.org/src/contrib/tinkr_0.3.1.tar.gz",
                "https://cloud.r-project.org/src/contrib/Archive/tinkr/tinkr_0.3.1.tar.gz",
            ]
        );
        assert_eq!(
            tarball_urls("bioconductor", "Biobase", "Biobase_2.62.0.tar.gz"),
            ["https://bioconductor.r-universe.dev/src/contrib/Biobase_2.62.0.tar.gz"]
        );
    }

    #[test]
    fn only_lower_bounds_give_a_minimum_r_version() {
        assert_eq!(r_minimum_version(">= 4.1.0"), Some((4, Some(1))));
        assert_eq!(r_minimum_version(">=3.5"), Some((3, Some(5))));
        assert_eq!(r_minimum_version("> 4"), Some((4, None)));
        assert_eq!(r_minimum_version(">= 3.1-0"), Some((3, Some(1))));
        for constraint in [
            "", "4.1", "<= 4.5", "< 5", "< 4.5.0", "== 4.1", "!= 4.2", ">= x.y", ">= 4.x",
        ] {
            assert_eq!(r_minimum_version(constraint), None, "{constraint:?}");
        }
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
