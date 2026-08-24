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

/// What the CRAN source tarball tells us beyond the R-universe metadata.
#[derive(Debug, Default)]
struct CranTarball {
    /// SHA256 of the downloaded tarball; only recorded when it came from the
    /// host the recipe's source URLs reference (the CRAN mirror).
    sha256: Option<String>,
    /// The package's `DESCRIPTION` file (`<package>/DESCRIPTION`).
    description: Option<String>,
    /// Whether the package ships the testthat runner, `tests/testthat.R`.
    has_testthat_runner: bool,
}

impl CranTarball {
    fn parse(bytes: &[u8]) -> Self {
        // One decompression pass for everything the tarball can tell us.
        let mut files = tarball::find_files(
            bytes,
            |path| {
                tarball::is_in_top_level_dir(path, "DESCRIPTION")
                    || tarball::is_in_top_level_dir(path, "tests/testthat.R")
            },
            2,
        )
        .inspect_err(|e| tracing::warn!("Failed to read the tarball: {e}"))
        .unwrap_or_default();
        let mut take = |relative: &str| {
            files
                .iter()
                .position(|(path, _)| tarball::is_in_top_level_dir(path, relative))
                .map(|index| files.remove(index).1)
        };
        Self {
            sha256: Some(hex::encode(compute_bytes_digest::<Sha256>(bytes))),
            description: take("DESCRIPTION"),
            has_testthat_runner: take("tests/testthat.R").is_some(),
        }
    }
}

/// A package's R-universe metadata plus what could be learned from its
/// tarball.
struct FetchedPackage {
    info: PackageInfo,
    /// `None` when the tarball could not be downloaded, e.g. in restricted
    /// environments (WASM/browser with no CORS on cran.r-project.org).
    tarball: Option<CranTarball>,
}

/// Append the package's DESCRIPTION file to the rendered recipe as a comment
/// block, in the format `conda skeleton cran` used, so that reviewers can check
/// the recipe against the upstream metadata without opening the tarball.
#[cfg(not(target_arch = "wasm32"))]
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
/// at build time, and is quoted because that path may contain spaces (on
/// Windows it lives under the user's profile directory).
const R_CMD_INSTALL: &str = "\"${{ R }}\" CMD INSTALL --build .";

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

/// The CRAN mirror used when no `cran_mirror` variant is configured (conda-forge
/// provides one through its pinning).
const CRAN_MIRROR: &str = "https://cran.r-project.org";

/// The `cran_mirror` context entry: the variant value when one is configured,
/// [`CRAN_MIRROR`] otherwise. A `context` variable shadows a variant key of the
/// same name, so the entry has to refer to the variant explicitly.
fn cran_mirror_context() -> String {
    format!("${{{{ cran_mirror | default(\"{CRAN_MIRROR}\") }}}}")
}

/// Placeholder maintainer when none is given on the command line (the same
/// convention grayskull uses).
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_MAINTAINER: &str = "AddYourGitHubIdHere";

/// The maintainers to list for a recipe generated from the command line:
/// the ones given, or a placeholder reminding the author to fill them in.
#[cfg(not(target_arch = "wasm32"))]
fn cli_maintainers(given: &[String]) -> Vec<String> {
    if given.is_empty() {
        vec![DEFAULT_MAINTAINER.to_string()]
    } else {
        given.to_vec()
    }
}

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
    // Top-level keys sit at column zero and block-scalar bodies never do, so
    // tracking the current section tells requirement entries apart from
    // look-alike text in e.g. the description.
    let mut in_requirements = false;
    for line in recipe_str.lines() {
        if line.chars().next().is_some_and(|first| first != ' ') {
            in_requirements = line.starts_with("requirements:");
        }
        if in_requirements && let Some(spec) = line.trim_start().strip_prefix("- SUGGEST ") {
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

/// Where `universe` serves the tarball named `file`. CRAN hosts its own
/// files; every other universe serves them through r-universe.dev.
fn tarball_url(universe: &str, file: &str) -> String {
    if universe == "cran" {
        format!("{CRAN_MIRROR}/src/contrib/{file}")
    } else {
        format!("https://{universe}.r-universe.dev/src/contrib/{file}")
    }
}

/// Fetch the metadata of `package` and, best-effort, its tarball.
async fn fetch_package(
    client: &reqwest::Client,
    package: &str,
    universe: &str,
) -> miette::Result<FetchedPackage> {
    tracing::info!("Generating R recipe for {}", package);
    let info = fetch_package_info(client, package, universe).await?;

    let tarball = match tarball::download(client, tarball_url(universe, &info._file)).await {
        Ok(bytes) => {
            let mut tarball = CranTarball::parse(&bytes);
            if universe != "cran" {
                // The recipe's source URLs point at the CRAN mirror, and
                // r-universe rebuilds tarballs, so this hash would not match
                // what the recipe downloads.
                tarball.sha256 = None;
                tracing::warn!(
                    "The package comes from the {universe} universe: the recipe's source URLs \
                     and checksum need manual attention."
                );
            }
            Some(tarball)
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch {}: {} — the recipe will not contain a checksum; add the sha256 by hand.",
                info._file,
                e
            );
            None
        }
    };

    Ok(FetchedPackage { info, tarball })
}

/// Turn R-universe package metadata (plus what the tarball told us, when it
/// could be downloaded) into a recipe.
///
/// `maintainers` fills `extra.recipe-maintainers` (omitted when empty). Also
/// returns the R packages the recipe depends on, for `--tree`.
fn package_info_to_recipe(
    info: &PackageInfo,
    tarball: Option<&CranTarball>,
    maintainers: &[String],
) -> (serialize::Recipe, HashSet<String>) {
    let mut recipe = serialize::Recipe::default();

    recipe
        .context
        .insert("version".to_string(), info.Version.clone());
    recipe
        .context
        .insert("cran_mirror".to_string(), cran_mirror_context());
    // Reading the `cran_mirror` variant makes it part of the package's variant
    // unless it is ignored explicitly, which would make the build string depend
    // on the mirror a package happened to be downloaded from.
    recipe.build.variant.ignore_keys = vec!["cran_mirror".to_string()];

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
    let file_name = format!("{}_${{{{ version }}}}.tar.gz", info.Package);
    recipe.source.push(
        UrlSourceElement {
            url: vec![
                format!("${{{{ cran_mirror }}}}/src/contrib/{file_name}"),
                format!(
                    "${{{{ cran_mirror }}}}/src/contrib/Archive/{}/{file_name}",
                    info.Package
                ),
            ],
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
    // The testthat test dependency keeps whatever constraint upstream declares
    // in its Suggests entry.
    let mut testthat_requirement = "r-testthat".to_string();

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
            if dep.package == "testthat" {
                testthat_requirement = format_r_package(&dep.package, dep.version.as_ref());
            }
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

    // `URL:` lists one or more URLs separated by commas and/or whitespace; the
    // first one is the package's home page by convention.
    if let Some(url) = info.URL.as_deref().and_then(|urls| {
        urls.split([',', ' ', '\n', '\t'])
            .find(|url| !url.is_empty())
    }) {
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

    recipe.extra.recipe_maintainers = maintainers.to_vec();

    (recipe, remaining_deps)
}

/// A rendered recipe plus what the recursive `--tree` mode and the command
/// line need alongside it.
struct GeneratedRecipe {
    /// Only `yaml` is read on wasm32, where the command-line front end that
    /// consumes the other fields is not built.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    recipe: serialize::Recipe,
    yaml: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    remaining_deps: HashSet<String>,
    /// The tarball's DESCRIPTION file, appended to the recipe by
    /// `--staged-recipes`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    description: Option<String>,
}

/// Fetch `package` from `universe` (CRAN when `None`) and render its recipe.
async fn fetch_and_render(
    client: &reqwest::Client,
    package: &str,
    universe: Option<&str>,
    maintainers: &[String],
) -> miette::Result<GeneratedRecipe> {
    let package = fetch_package(client, package, universe.unwrap_or("cran")).await?;
    let (recipe, remaining_deps) =
        package_info_to_recipe(&package.info, package.tarball.as_ref(), maintainers);
    let yaml = format_cran_recipe_with_suggests(&recipe);
    let description = package.tarball.and_then(|tarball| tarball.description);
    Ok(GeneratedRecipe {
        recipe,
        yaml,
        remaining_deps,
        description,
    })
}

/// Generate a CRAN recipe for `package` and return the YAML as a string.
pub async fn generate_r_recipe_string(
    package: &str,
    universe: Option<&str>,
) -> miette::Result<String> {
    let client = reqwest::Client::new();
    Ok(fetch_and_render(&client, package, universe, &[])
        .await?
        .yaml)
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
        &cli_maintainers(&opts.maintainers),
    )
    .await?;

    let mut final_recipe = generated.yaml;
    if opts.staged_recipes {
        match &generated.description {
            Some(description) => {
                final_recipe = append_description_comment(&final_recipe, description);
            }
            None => tracing::warn!("No DESCRIPTION file available to append to the recipe"),
        }
    }

    if opts.write {
        write_recipe(&generated.recipe.package.name, &final_recipe).into_diagnostic()?;
    } else {
        print!("{}", final_recipe);
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
        let (recipe, deps) = package_info_to_recipe(&info, Some(&tarball), &cli_maintainers(&[]));
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

    /// CRAN's `URL:` field may separate several URLs with commas, spaces or
    /// newlines; the first one is the home page.
    #[test]
    fn homepage_is_the_first_url_of_the_url_field() {
        let mut info = fixture("tinkr");
        for urls in [
            "https://a.example, https://b.example",
            "https://a.example https://b.example",
            "https://a.example,\nhttps://b.example",
            "  https://a.example",
        ] {
            info.URL = Some(urls.to_string());
            let (recipe, _) = package_info_to_recipe(&info, None, &[]);
            assert_eq!(
                recipe.about.homepage.as_deref(),
                Some("https://a.example"),
                "{urls:?}"
            );
        }
        info.URL = None;
        let (recipe, _) = package_info_to_recipe(&info, None, &[]);
        assert_eq!(recipe.about.homepage, None);
    }

    /// Library callers (py-rattler-build, the playground) get no `extra:` block
    /// unless they name maintainers; the placeholder is a CLI convenience.
    #[test]
    fn maintainers_are_only_defaulted_on_the_command_line() {
        let info = fixture("tinkr");
        let (recipe, _) = package_info_to_recipe(&info, None, &[]);
        assert!(recipe.extra.recipe_maintainers.is_empty());
        assert!(!format_cran_recipe_with_suggests(&recipe).contains("extra:"));

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
            let (recipe, _) = package_info_to_recipe(&info, tarball, &[]);
            assert_eq!(recipe.tests.len(), 1, "only the `r:` test is expected");
            assert!(matches!(recipe.tests[0], Test::R(_)));
        }
    }

    /// The SUGGEST marker must only be recognised inside the requirements
    /// section, not in free text such as the description — not even when a
    /// description line has the exact shape of a suggested-dependency entry.
    #[test]
    fn only_suggested_dependency_lines_are_commented_out() {
        let mut info = fixture("tinkr");
        info.Description =
            "Does things as SUGGESTED by:\n- SUGGEST mode for reviewers\n- other modes".to_string();
        let (recipe, _) = package_info_to_recipe(&info, None, &[]);
        let yaml = format_cran_recipe_with_suggests(&recipe);
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

    #[test]
    fn tarballs_come_from_the_requested_universe() {
        assert_eq!(
            tarball_url("cran", "tinkr_0.3.1.tar.gz"),
            "https://cran.r-project.org/src/contrib/tinkr_0.3.1.tar.gz"
        );
        assert_eq!(
            tarball_url("bioconductor", "Biobase_2.62.0.tar.gz"),
            "https://bioconductor.r-universe.dev/src/contrib/Biobase_2.62.0.tar.gz"
        );
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
