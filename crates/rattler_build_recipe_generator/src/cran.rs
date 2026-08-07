#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::{collections::HashMap, collections::HashSet};

#[cfg(feature = "cli")]
use clap::Parser;
use itertools::Itertools;
use miette::IntoDiagnostic;
#[cfg(not(target_arch = "wasm32"))]
use rattler_conda_types::{Channel, ChannelConfig, PackageName, Platform};
use rattler_digest::{Sha256Hash, compute_bytes_digest};
#[cfg(not(target_arch = "wasm32"))]
use rattler_repodata_gateway::{Gateway, GatewayError, RepoData};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use url::Url;

use crate::serialize::{self, ScriptTest, Test, UrlSourceElement};
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

    /// Whether to recursively generate recipes for dependencies. By default,
    /// only dependencies missing from conda-forge are recursed into (mirrors
    /// grayskull's `--recursive`); pass `--full` as well to recurse into the
    /// whole dependency tree regardless of conda-forge status.
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub tree: bool,

    /// Modifies `--tree` to recurse into every dependency, including ones
    /// already available on conda-forge. Has no effect unless `--tree` is
    /// also set.
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub full: bool,

    /// Name of the package to generate
    pub package: String,

    /// Whether to write the recipe to a folder
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub write: bool,
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

/// Download the package at `url` and compute its SHA256 checksum.
pub async fn fetch_package_sha256sum(url: &Url) -> Result<Sha256Hash, miette::Error> {
    let client = reqwest::Client::new();
    let response = client.get(url.clone()).send().await.into_diagnostic()?;
    let bytes = response.bytes().await.into_diagnostic()?;
    Ok(compute_bytes_digest::<Sha256>(&bytes))
}

/// The default "conda-forge" channel, resolved against the standard
/// `https://conda.anaconda.org` alias.
#[cfg(not(target_arch = "wasm32"))]
fn conda_forge_channel() -> Channel {
    let channel_config = ChannelConfig::default_with_root_dir(
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/")),
    );
    Channel::from_name("conda-forge", &channel_config)
}

/// Query `channel` via the repodata gateway for which of `package_names`
/// are present there, for the host platform or `noarch`. Queries every name
/// in a single batched request rather than one at a time, since the
/// gateway can fetch multiple packages concurrently.
#[cfg(not(target_arch = "wasm32"))]
async fn query_conda_forge_channel(
    gateway: &Gateway,
    channel: Channel,
    package_names: Vec<PackageName>,
) -> Result<HashSet<PackageName>, GatewayError> {
    let output = gateway
        .query(
            vec![channel],
            [Platform::current(), Platform::NoArch],
            package_names,
        )
        .recursive(false)
        .await?;
    Ok(output
        .iter()
        .flat_map(RepoData::iter)
        .map(|record| record.package_record.name.clone())
        .collect())
}

/// Check `deps` against `channel`, returning those that aren't available
/// there (as the original, non-prefixed dependency names). Checks all of
/// `deps` in a single batched gateway query. Split out from
/// [`find_missing_conda_forge_deps`] so tests can exercise the
/// network-error branch deterministically by pointing it at an unreachable
/// channel.
///
/// Network errors (and unparsable package names) are treated as "exists"
/// so connectivity issues don't produce spurious warnings.
#[cfg(not(target_arch = "wasm32"))]
async fn find_missing_conda_forge_deps_in(channel: Channel, deps: &HashSet<String>) -> Vec<String> {
    if deps.is_empty() {
        return Vec::new();
    }

    let mut package_name_to_dep = HashMap::new();
    for dep in deps {
        let conda_name = format_r_package(dep, None);
        match conda_name.parse::<PackageName>() {
            Ok(package_name) => {
                package_name_to_dep.insert(package_name, dep.clone());
            }
            Err(e) => {
                // Fail open for this dep: an unparsable name can't be
                // confirmed missing, so don't warn about it.
                tracing::debug!("Invalid conda package name '{}': {}", conda_name, e);
            }
        }
    }

    let gateway = Gateway::new();
    let package_names = package_name_to_dep.keys().cloned().collect();
    let found = match query_conda_forge_channel(&gateway, channel, package_names).await {
        Ok(found) => found,
        Err(e) => {
            // Fail open: a query-wide failure shouldn't produce spurious
            // warnings for every dependency.
            tracing::debug!("Failed to check conda-forge: {}", e);
            return Vec::new();
        }
    };

    package_name_to_dep
        .into_iter()
        .filter(|(name, _)| !found.contains(name))
        .map(|(_, dep)| dep)
        .collect()
}

/// Check `deps` against conda-forge, returning those that aren't available
/// there yet (as the original, non-prefixed dependency names).
///
/// Unlike PyPI, CRAN package names map to conda-forge by a fixed convention
/// (`foo` -> `r-foo`, see [`format_r_package`]), so no name-mapping lookup is
/// needed here, only an existence check via the repodata gateway.
#[cfg(not(target_arch = "wasm32"))]
async fn find_missing_conda_forge_deps(deps: &HashSet<String>) -> Vec<String> {
    find_missing_conda_forge_deps_in(conda_forge_channel(), deps).await
}

/// Warn about each dependency in `missing`, so the user knows upfront which
/// recipes they need to package first.
#[cfg(not(target_arch = "wasm32"))]
fn warn_about_missing_conda_forge_deps(missing: &[String]) {
    for dep in missing {
        let conda_name = format_r_package(dep, None);
        tracing::warn!(
            "Dependency '{}' does not appear to be available on conda-forge as '{}'. You may need to create and publish a recipe for it first.",
            dep,
            conda_name
        );
    }
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

/// Placeholder pushed into `requirements.build` for compiled packages. It is
/// expanded by [`format_cran_recipe_with_suggests`] into a `build_platform !=
/// target_platform` selector that pulls in `cross-r-base` when cross-compiling.
///
/// A plain identifier is used so that `serde_yaml` emits it unquoted, which
/// keeps the post-processing match simple.
const CROSS_R_BASE_MARKER: &str = "CROSS_R_BASE_PLACEHOLDER";

/// Placeholder stored in `build.script`. Expanded by
/// [`format_cran_recipe_with_suggests`] into a platform-conditional list that
/// uses `${R_ARGS}` on Unix and `%R_ARGS%` on Windows.
const CRAN_R_SCRIPT_MARKER: &str = "CRAN_R_SCRIPT_PLACEHOLDER";

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

fn format_cran_recipe_with_suggests(recipe: &serialize::Recipe) -> String {
    let recipe_str = format!("{}", recipe);
    let mut final_recipe = String::new();
    for line in recipe_str.lines() {
        if let Some(indent) = line
            .strip_suffix(CROSS_R_BASE_MARKER)
            .and_then(|prefix| prefix.strip_suffix("- "))
        {
            // Expand the placeholder into a cross-compilation selector. `r_base`
            // is the conda-forge variant key pinning the R version.
            final_recipe.push_str(&format!(
                "{indent}- if: build_platform != target_platform\n\
                 {indent}  then:\n\
                 {indent}    - cross-r-base ${{{{ r_base }}}}\n"
            ));
        } else if line
            .trim_start()
            .starts_with(&format!("script: {CRAN_R_SCRIPT_MARKER}"))
        {
            // Expand into a platform-conditional script list.
            let indent_len = line.len() - line.trim_start().len();
            let indent = &line[..indent_len];
            final_recipe.push_str(&format!(
                "{indent}script:\n\
                 {indent}  - if: win\n\
                 {indent}    then: R CMD INSTALL --build . %R_ARGS%\n\
                 {indent}    else: R CMD INSTALL --build . ${{R_ARGS}}\n"
            ));
        } else if line.contains("SUGGEST") {
            final_recipe.push_str(&format!(
                "{}  # suggested\n",
                line.replace(" - SUGGEST", " # - ")
            ));
        } else {
            final_recipe.push_str(&format!("{}\n", line));
        }
    }
    final_recipe
}

async fn build_cran_recipe_and_deps(
    package: &str,
    universe: Option<&str>,
) -> miette::Result<(serialize::Recipe, HashSet<String>)> {
    let universe = universe.unwrap_or("cran");
    tracing::info!("Generating R recipe for {}", package);
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

    recipe
        .context
        .insert("build_number".to_string(), "0".to_string());

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

    // Fetching the tarball to hash it is best-effort: in restricted environments
    // (e.g. WASM/browser with no CORS on cran.r-project.org) we fall back to the
    // MD5 the R-universe API already gave us.
    let (sha256, md5) = match fetch_package_sha256sum(&url).await {
        Ok(hash) => (Some(hex::encode(hash)), None),
        Err(e) => {
            tracing::warn!(
                "Failed to fetch SHA256 for {}: {} — falling back to MD5 from R-universe.",
                package_info._file,
                e
            );
            (None, package_info.MD5sum.clone())
        }
    };

    let source = UrlSourceElement {
        url: vec![url.to_string(), url_archive.to_string()],
        md5,
        sha256,
    };
    recipe.source.push(source.into());

    recipe.build.number = "${{ build_number }}".to_string();
    // Expanded by `format_cran_recipe_with_suggests` into a platform-specific
    // list (${R_ARGS} on Unix, %R_ARGS% on Windows).
    recipe.build.script = CRAN_R_SCRIPT_MARKER.to_string();

    let build_tools = vec![
        "${{ compiler('c') }}".to_string(),
        "${{ compiler('cxx') }}".to_string(),
        "make".to_string(),
    ];

    // Whether the package contains code that has to be compiled. Packages that
    // declare `LinkingTo` dependencies also compile against those headers, so
    // they need a compiler even if `NeedsCompilation` is not set to `yes`.
    let mut needs_compilation = package_info.NeedsCompilation == "yes";

    // `r-base` is always listed without a version pin; instead a minimum-R
    // constraint from `Depends: R (>= x.y.z)` is expressed as a `skip`
    // condition so variant selectors (r_base) control it at solve time.
    let r_base = "r-base".to_string();
    let mut host = Vec::new();
    let mut run = Vec::new();

    let mut remaining_deps = HashSet::new();
    for dep in package_info._dependencies.iter() {
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
            run.push(format!(
                "SUGGEST {}",
                format_r_package(&dep.package, dep.version.as_ref())
            ));
        }
    }

    recipe.requirements.host = std::iter::once(r_base.clone())
        .chain(host)
        .unique()
        .collect();
    recipe.requirements.run = std::iter::once(r_base).chain(run).unique().collect();

    if needs_compilation {
        // Compiled packages need a toolchain, `cross-r-base` for cross builds,
        // and rpaths so the linker can find R's shared libraries.
        let mut build = vec![CROSS_R_BASE_MARKER.to_string()];
        build.extend(build_tools);
        recipe.requirements.build = build.into_iter().unique().collect();
        recipe.build.dynamic_linking = Some(serialize::DynamicLinking {
            rpaths: vec!["lib/R/lib/".to_string(), "lib/".to_string()],
        });
    } else {
        // Pure-R packages are architecture independent.
        recipe.build.noarch = Some("generic".to_string());
    }

    if let Some(url) = package_info.URL.clone() {
        let url = url.split_once(',').unwrap_or((url.as_str(), "")).0;
        recipe.about.homepage = Some(url.to_string());
    }

    recipe.about.summary = Some(package_info.Title.clone());
    recipe.about.description = Some(package_info.Description.clone());
    let (license, license_files) = map_license(&package_info.License);
    recipe.about.license = license;
    recipe.about.license_file = license_files;
    recipe.about.repository = Some(package_info._upstream.clone());
    if let Some(pkgdocs) = &package_info._pkgdocs
        && url::Url::parse(pkgdocs).is_ok()
    {
        recipe.about.documentation = Some(pkgdocs.clone());
    }

    recipe.tests.push(Test::Script(ScriptTest {
        script: vec![format!(
            "Rscript -e 'library(\"{}\")'",
            package_info.Package
        )],
    }));

    Ok((recipe, remaining_deps))
}

/// Generate a CRAN recipe for `package` and return the YAML as a string.
pub async fn generate_r_recipe_string(
    package: &str,
    universe: Option<&str>,
) -> miette::Result<String> {
    let (recipe, _remaining_deps) = build_cran_recipe_and_deps(package, universe).await?;
    Ok(format_cran_recipe_with_suggests(&recipe))
}

#[cfg(not(target_arch = "wasm32"))]
#[async_recursion::async_recursion]
/// Generate a CRAN recipe using `CranOpts` and either print it or write it to disk.
///
/// If `opts.write` is true, the recipe is written to a folder named after the
/// package. Otherwise, the YAML is printed to stdout. When `tree` is enabled,
/// dependencies that don't already exist locally are recursively generated:
/// by default only those missing from conda-forge, or every dependency if
/// `full` is also set.
pub async fn generate_r_recipe(opts: &CranOpts) -> miette::Result<()> {
    let (recipe, remaining_deps) =
        build_cran_recipe_and_deps(&opts.package, opts.universe.as_deref()).await?;

    let final_recipe = format_cran_recipe_with_suggests(&recipe);

    if opts.write {
        write_recipe(&recipe.package.name, &final_recipe).into_diagnostic()?;
    } else {
        print!("{}", final_recipe);
    }

    let missing_deps = find_missing_conda_forge_deps(&remaining_deps).await;
    warn_about_missing_conda_forge_deps(&missing_deps);

    generate_recipes_for_deps(
        select_deps_to_recurse(remaining_deps, missing_deps, opts),
        opts,
    )
    .await?;

    Ok(())
}

/// Decide which dependencies to recursively generate recipes for:
/// - without `tree`, none;
/// - with `tree` alone, only those missing from conda-forge (generating
///   recipes for ones already available there isn't useful, since those
///   boilerplate recipes would need further changes to be valid anyway);
/// - with `tree` and `full`, every dependency, regardless of conda-forge
///   status.
#[cfg(not(target_arch = "wasm32"))]
fn select_deps_to_recurse(
    remaining_deps: HashSet<String>,
    missing_deps: Vec<String>,
    opts: &CranOpts,
) -> Vec<String> {
    if !opts.tree {
        return Vec::new();
    }

    if opts.full {
        remaining_deps.into_iter().collect()
    } else {
        missing_deps
    }
}

/// Generate a recipe for each of `deps` that doesn't already have a local
/// folder, reusing `opts` (except for the package name) for each one.
#[cfg(not(target_arch = "wasm32"))]
async fn generate_recipes_for_deps(
    deps: impl IntoIterator<Item = String>,
    opts: &CranOpts,
) -> miette::Result<()> {
    for dep in deps {
        let r_package = format_r_package(&dep, None);

        if !PathBuf::from(r_package).exists() {
            let opts = CranOpts {
                package: dep,
                ..opts.clone()
            };
            generate_r_recipe(&opts).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

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

    /// A channel whose host is guaranteed to never resolve, so queries
    /// against it deterministically exercise the network-error branch
    /// regardless of the test environment's actual connectivity. `.invalid`
    /// is an IANA-reserved TLD for exactly this purpose (RFC 2606).
    fn unreachable_channel() -> Channel {
        Channel {
            base_url: Url::parse("https://conda-forge.invalid/conda-forge/")
                .unwrap()
                .into(),
            name: Some("conda-forge".to_string()),
            platforms: None,
        }
    }

    #[tokio::test]
    #[traced_test]
    async fn test_query_conda_forge_channel_finds_real_package() {
        let gateway = Gateway::new();
        let package_name = "r-jsonlite".parse().unwrap();

        let found = query_conda_forge_channel(&gateway, conda_forge_channel(), vec![package_name])
            .await
            .unwrap();

        assert!(!found.is_empty());
    }

    #[tokio::test]
    #[traced_test]
    async fn test_query_conda_forge_channel_excludes_fake_package() {
        let gateway = Gateway::new();
        let package_name: PackageName = "r-this-package-does-not-exist-xyz123".parse().unwrap();

        let found =
            query_conda_forge_channel(&gateway, conda_forge_channel(), vec![package_name.clone()])
                .await
                .unwrap();

        assert!(!found.contains(&package_name));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_find_missing_conda_forge_deps_in_fails_open_on_network_error() {
        let mut deps = HashSet::new();
        deps.insert("jsonlite".to_string());

        // `.invalid` is an IANA-reserved TLD (RFC 2606) guaranteed never to
        // resolve, so this deterministically exercises the network-error
        // branch regardless of the test environment's actual connectivity.
        let missing = find_missing_conda_forge_deps_in(unreachable_channel(), &deps).await;

        // Fails open: a query-wide failure reports nothing as missing,
        // rather than treating every dependency as missing.
        assert!(missing.is_empty());
        assert!(logs_contain("Failed to check conda-forge"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_find_missing_conda_forge_deps_includes_fake_package() {
        let dep = "this_package_does_not_exist_xyz123";
        let mut deps = HashSet::new();
        deps.insert(dep.to_string());

        let missing = find_missing_conda_forge_deps(&deps).await;

        if !missing.contains(&dep.to_string()) {
            // The dependency is only excluded from the missing list if the
            // conda-forge check itself failed open due to a network error.
            // Confirm that's what happened, rather than a false positive.
            let gateway = Gateway::new();
            let package_name = format!("r-{dep}").parse().unwrap();
            assert!(
                query_conda_forge_channel(&gateway, conda_forge_channel(), vec![package_name])
                    .await
                    .is_err(),
                "fake dependency unexpectedly reported as existing on conda-forge"
            );
        }
    }

    #[tokio::test]
    #[traced_test]
    async fn test_find_missing_conda_forge_deps_excludes_real_package() {
        let mut deps = HashSet::new();
        deps.insert("jsonlite".to_string());

        let missing = find_missing_conda_forge_deps(&deps).await;

        assert!(missing.is_empty());
    }

    #[test]
    #[traced_test]
    fn test_warn_about_missing_conda_forge_deps_logs_warning() {
        let missing = vec!["this_package_does_not_exist_xyz123".to_string()];

        warn_about_missing_conda_forge_deps(&missing);

        assert!(logs_contain(
            "Dependency 'this_package_does_not_exist_xyz123' does not appear to be available on conda-forge as 'r-this_package_does_not_exist_xyz123'"
        ));
    }

    #[test]
    #[traced_test]
    fn test_warn_about_missing_conda_forge_deps_silent_when_empty() {
        warn_about_missing_conda_forge_deps(&[]);

        assert!(!logs_contain(
            "does not appear to be available on conda-forge"
        ));
    }

    fn dummy_cran_opts() -> CranOpts {
        CranOpts {
            universe: None,
            tree: false,
            full: false,
            package: "unused".to_string(),
            write: false,
        }
    }

    // The recursion tests fabricate the `missing_deps` set directly rather than
    // querying conda-forge, so these two names are just placeholders whose
    // conda-forge "status" is decided purely by which set a test puts them in.
    // Neither is a real package on conda-forge or CRAN -- the absence from CRAN
    // is what makes a recursion into either fail fast instead of downloading
    // anything.

    /// Placeholder the recursion tests treat as present on conda-forge, by
    /// keeping it out of the fabricated `missing_deps`.
    const DEP_ON_CONDA_FORGE: &str = "dep_on_conda_forge_xyz123";
    /// Placeholder the recursion tests treat as missing from conda-forge, by
    /// putting it in the fabricated `missing_deps`.
    const DEP_MISSING_FROM_CONDA_FORGE: &str = "dep_missing_from_conda_forge_xyz123";

    /// Build the `remaining_deps`/`missing_deps` pair that `generate_r_recipe`
    /// would hand to `select_deps_to_recurse`.
    fn deps_split() -> (HashSet<String>, Vec<String>) {
        let remaining = HashSet::from([
            DEP_ON_CONDA_FORGE.to_string(),
            DEP_MISSING_FROM_CONDA_FORGE.to_string(),
        ]);
        let missing = vec![DEP_MISSING_FROM_CONDA_FORGE.to_string()];
        (remaining, missing)
    }

    fn opts_with_flags(tree: bool, full: bool) -> CranOpts {
        CranOpts {
            tree,
            full,
            ..dummy_cran_opts()
        }
    }

    #[test]
    fn test_select_deps_to_recurse_without_tree_recurses_into_nothing() {
        let (remaining, missing) = deps_split();

        let selected = select_deps_to_recurse(remaining, missing, &opts_with_flags(false, false));

        assert!(selected.is_empty());
    }

    #[test]
    fn test_select_deps_to_recurse_full_without_tree_has_no_effect() {
        let (remaining, missing) = deps_split();

        // `--full` only modifies `--tree`'s behavior, so without `--tree` it
        // still recurses into nothing.
        let selected = select_deps_to_recurse(remaining, missing, &opts_with_flags(false, true));

        assert!(selected.is_empty());
    }

    #[test]
    fn test_select_deps_to_recurse_tree_only_recurses_into_missing() {
        let (remaining, missing) = deps_split();

        let selected = select_deps_to_recurse(remaining, missing, &opts_with_flags(true, false));

        // `--tree` alone recurses only into dependencies missing from
        // conda-forge.
        assert_eq!(selected, vec![DEP_MISSING_FROM_CONDA_FORGE.to_string()]);
    }

    #[test]
    fn test_select_deps_to_recurse_tree_full_recurses_into_everything() {
        let (remaining, missing) = deps_split();

        let selected = select_deps_to_recurse(remaining, missing, &opts_with_flags(true, true));

        // `--tree --full` recurses into every dependency, on conda-forge or
        // not.
        assert_eq!(
            selected.into_iter().collect::<HashSet<_>>(),
            HashSet::from([
                DEP_ON_CONDA_FORGE.to_string(),
                DEP_MISSING_FROM_CONDA_FORGE.to_string()
            ])
        );
    }

    /// Mirror the tail of `generate_r_recipe` for a single fabricated
    /// dependency: warn about it if it's flagged missing from conda-forge,
    /// then recurse into it if `select_deps_to_recurse` picks it for these
    /// flags. Only `dep` is in `remaining_deps` (so recursion never
    /// short-circuits on a sibling), and `missing_deps` contains it only when
    /// `missing_from_conda_forge` is set. `dep` isn't a real CRAN package, so a
    /// selected recursion logs "Generating R recipe for <dep>" and then fails
    /// fast -- the log is what the callers assert on.
    async fn warn_and_recurse(dep: &str, missing_from_conda_forge: bool, opts: &CranOpts) {
        let remaining = HashSet::from([dep.to_string()]);
        let missing = if missing_from_conda_forge {
            vec![dep.to_string()]
        } else {
            Vec::new()
        };

        warn_about_missing_conda_forge_deps(&missing);
        let selected = select_deps_to_recurse(remaining, missing, opts);
        let _ = generate_recipes_for_deps(selected, opts).await;
    }

    #[tokio::test]
    #[traced_test]
    async fn test_recursion_without_tree_warns_but_never_recurses() {
        let opts = opts_with_flags(false, false);

        warn_and_recurse(DEP_MISSING_FROM_CONDA_FORGE, true, &opts).await;
        // The missing dependency is warned about, but without `--tree` it is
        // not recursed into.
        assert!(logs_contain(&format!(
            "Dependency '{DEP_MISSING_FROM_CONDA_FORGE}' does not appear to be available on conda-forge"
        )));
        assert!(!logs_contain(&format!(
            "Generating R recipe for {DEP_MISSING_FROM_CONDA_FORGE}"
        )));

        warn_and_recurse(DEP_ON_CONDA_FORGE, false, &opts).await;
        assert!(!logs_contain(&format!(
            "Generating R recipe for {DEP_ON_CONDA_FORGE}"
        )));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_recursion_full_without_tree_has_no_effect() {
        let opts = opts_with_flags(false, true);

        // `--full` only modifies `--tree`'s behavior, so without `--tree`
        // nothing is recursed into, even a dependency missing from
        // conda-forge.
        warn_and_recurse(DEP_MISSING_FROM_CONDA_FORGE, true, &opts).await;
        assert!(!logs_contain(&format!(
            "Generating R recipe for {DEP_MISSING_FROM_CONDA_FORGE}"
        )));

        warn_and_recurse(DEP_ON_CONDA_FORGE, false, &opts).await;
        assert!(!logs_contain(&format!(
            "Generating R recipe for {DEP_ON_CONDA_FORGE}"
        )));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_recursion_tree_only_recurses_into_missing() {
        let opts = opts_with_flags(true, false);

        warn_and_recurse(DEP_MISSING_FROM_CONDA_FORGE, true, &opts).await;
        assert!(logs_contain(&format!(
            "Dependency '{DEP_MISSING_FROM_CONDA_FORGE}' does not appear to be available on conda-forge"
        )));
        assert!(logs_contain(&format!(
            "Generating R recipe for {DEP_MISSING_FROM_CONDA_FORGE}"
        )));

        // `--tree` alone only recurses into dependencies missing from
        // conda-forge, so an already-available dependency is left alone.
        warn_and_recurse(DEP_ON_CONDA_FORGE, false, &opts).await;
        assert!(!logs_contain(&format!(
            "Generating R recipe for {DEP_ON_CONDA_FORGE}"
        )));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_recursion_tree_full_recurses_into_everything() {
        let opts = opts_with_flags(true, true);

        warn_and_recurse(DEP_MISSING_FROM_CONDA_FORGE, true, &opts).await;
        assert!(logs_contain(&format!(
            "Dependency '{DEP_MISSING_FROM_CONDA_FORGE}' does not appear to be available on conda-forge"
        )));
        assert!(logs_contain(&format!(
            "Generating R recipe for {DEP_MISSING_FROM_CONDA_FORGE}"
        )));

        // `--tree --full` recurses into every dependency, including ones
        // already on conda-forge.
        warn_and_recurse(DEP_ON_CONDA_FORGE, false, &opts).await;
        assert!(logs_contain(&format!(
            "Generating R recipe for {DEP_ON_CONDA_FORGE}"
        )));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_generate_recipes_for_deps_skips_dependency_with_existing_local_folder() {
        let dep = "test-recurse-missing-fixture-existing";
        let folder = format_r_package(dep, None);
        fs_err::create_dir_all(&folder).unwrap();

        let result = generate_recipes_for_deps([dep.to_string()], &dummy_cran_opts()).await;

        fs_err::remove_dir_all(&folder).unwrap();

        // A pre-existing local folder means the dependency is skipped
        // entirely, so no network call is attempted and this can't fail.
        assert!(
            result.is_ok(),
            "should skip recursion once a local folder for the dependency already exists"
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_generate_recipes_for_deps_recurses_into_dependency_without_local_folder() {
        let dep = "this-package-definitely-does-not-exist-anywhere-xyz123";
        let folder = format_r_package(dep, None);
        assert!(
            !PathBuf::from(&folder).exists(),
            "test fixture assumption violated: {folder} should not exist locally"
        );

        let result = generate_recipes_for_deps([dep.to_string()], &dummy_cran_opts()).await;

        // No local folder means `generate_recipes_for_deps` recurses into
        // `generate_r_recipe`, which looks the (nonexistent) package up on
        // CRAN and fails -- proving recursion was actually attempted rather
        // than silently skipped.
        assert!(result.is_err());
    }
}
