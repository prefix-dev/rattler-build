use async_once_cell::OnceCell;
#[cfg(feature = "cli")]
use clap::Parser;
use miette::{IntoDiagnostic, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Cursor, Read as _};
use std::path::PathBuf;
use zip::ZipArchive;

use super::write_recipe;
use crate::serialize::{self, PythonTest, PythonTestInner, Test, UrlSourceElement};

#[derive(Deserialize)]
struct CondaPyPiNameMapping {
    conda_name: String,
    pypi_name: String,
}

/// Options for generating a Python (PyPI) recipe.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "cli", derive(Parser))]
pub struct PyPIOpts {
    /// Name of the package to generate
    pub package: String,

    /// Select a version of the package to generate (defaults to latest)
    #[cfg_attr(feature = "cli", arg(long))]
    pub version: Option<String>,

    /// Whether to write the recipe to a folder
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub write: bool,

    /// Whether to use the conda-forge PyPI name mapping
    #[cfg_attr(feature = "cli", arg(short, long, default_value = "true"))]
    pub use_mapping: bool,

    /// Whether to generate recipes for all dependencies
    #[cfg_attr(feature = "cli", arg(short, long))]
    pub tree: bool,

    /// Specify the PyPI index URL(s) to use for recipe generation
    #[cfg_attr(
        feature = "cli",
        arg(
            long = "pypi-index-url",
            env = "RATTLER_BUILD_PYPI_INDEX_URL",
            default_value = "https://pypi.org/pypi",
            value_delimiter = ',',
            help = "Specify the PyPI index URL(s) to use for recipe generation"
        )
    )]
    pub pypi_index_urls: Vec<String>,
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
    license_expression: Option<String>,
    classifiers: Option<Vec<String>>,
    requires_dist: Option<Vec<String>>,
    project_urls: Option<HashMap<String, String>>,
    requires_python: Option<String>,
}

/// Information extracted from a wheel archive.
#[derive(Default, Debug)]
struct WheelInfo {
    /// Console script entry points (e.g. `"cmd = package.module:func"`).
    entry_points: Vec<String>,
    /// License file paths from `License-File` METADATA headers.
    license_files: Vec<String>,
}

async fn extract_wheel_info(url: &str, client: &reqwest::Client) -> miette::Result<WheelInfo> {
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

    let mut info = WheelInfo::default();

    // Extract License-File entries from METADATA
    let metadata_file = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .map(|file| file.name().ends_with(".dist-info/METADATA"))
            .unwrap_or(false)
    });

    if let Some(index) = metadata_file {
        let mut file = archive.by_index(index).into_diagnostic()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).into_diagnostic()?;

        info.license_files = contents
            .lines()
            .filter_map(|l| l.strip_prefix("License-File: "))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    // Extract entry_points.txt
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
        info.entry_points = contents
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
    }

    Ok(info)
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

/// Metadata about a PyPI release used to construct the recipe.
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
            let toml: toml::Table = contents.parse().into_diagnostic()?;

            // Try different build system specs
            return Ok(match toml.get("build-system") {
                Some(build) => build
                    .get("requires")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                None => Vec::new(),
            });
        }
    }

    Ok(Vec::new())
}

/// Fetch and cache the conda-forge mapping from conda package names to PyPI names.
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

pub(crate) fn format_requirement(req: &str) -> String {
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
    // Try each PyPI index URL in sequence until one works
    let mut errors = Vec::new();

    for base_url in &opts.pypi_index_urls {
        let base_url = if base_url.ends_with('/') {
            base_url.to_string()
        } else {
            format!("{}/", base_url)
        };

        let result: Result<(PyPiInfo, Vec<PyPiRelease>), miette::Error> = async {
            if let Some(version) = &opts.version {
                let url = format!("{}{}/{}/json", base_url, opts.package, version);
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .into_diagnostic()
                    .context(format!("Failed to fetch from {}", url))?;

                if !response.status().is_success() {
                    return Err(miette::miette!(
                        "Server returned status code: {}",
                        response.status()
                    ));
                }

                let release: PyPrReleaseResponse = response.json().await.into_diagnostic()?;
                Ok((release.info, release.urls))
            } else {
                let url = format!("{}{}/json", base_url, opts.package);
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .into_diagnostic()
                    .context(format!("Failed to fetch from {}", url))?;

                if !response.status().is_success() {
                    return Err(miette::miette!(
                        "Server returned status code: {}",
                        response.status()
                    ));
                }

                let response: PyPiResponse = response.json().await.into_diagnostic()?;

                // Get the latest release
                let urls = response
                    .releases
                    .get(&response.info.version)
                    .ok_or_else(|| miette::miette!("No source distribution found"))?;
                Ok((response.info, urls.clone()))
            }
        }
        .await;

        match result {
            Ok((info, urls)) => {
                tracing::info!("Successfully fetched metadata from {}", base_url);

                let release = urls
                    .iter()
                    .find(|r| r.filename.ends_with(".tar.gz"))
                    .ok_or_else(|| miette::miette!("No source distribution found in {}", base_url))?
                    .clone();

                let wheel_url = urls
                    .iter()
                    .find(|r| r.filename.ends_with(".whl"))
                    .map(|r| r.url.clone());

                return Ok(PyPiMetadata {
                    info,
                    urls,
                    release,
                    wheel_url,
                });
            }
            Err(err) => {
                tracing::warn!("Failed to fetch from {}: {}", base_url, err);
                errors.push(format!("{}: {}", base_url, err));
            }
        }
    }

    // If we get here, all URLs failed
    let error_message = format!(
        "Failed to fetch metadata from all provided PyPI URLs:\n- {}",
        errors.join("\n- ")
    );
    Err(miette::miette!(error_message))
}

pub(crate) async fn map_requirement(
    req: &str,
    mapping: &HashMap<String, String>,
    use_mapping: bool,
) -> String {
    if !use_mapping {
        return req.to_string();
    }
    // Get base package name without markers/version
    if let Some(base_name) = req.split([' ', ';']).next()
        && let Some(mapped_name) = mapping.get(base_name)
    {
        // Replace the package name but keep version and markers
        return req.replacen(base_name, mapped_name, 1).to_string();
    }
    req.to_string()
}

/// Map a PyPI license classifier name (the part after `"License :: OSI Approved :: "`)
/// to an SPDX license identifier.
///
/// Many classifiers already carry the SPDX id in parentheses —
/// e.g. `"Boost Software License 1.0 (BSL-1.0)"` — and we extract it from there.
/// For the remaining ones we maintain a hand-curated lookup table derived from
/// the [PEP 639 appendix](https://peps.python.org/pep-0639/appendix-mapping-classifiers/).
///
/// Returns `(spdx_id, ambiguous)` where `ambiguous` is `true` for classifiers
/// that PEP 639 flags as not specifying a particular version/variant.
fn classifier_to_spdx(classifier_name: &str) -> Option<(&'static str, bool)> {
    // Classifiers that PEP 639 considers ambiguous — they don't specify the
    // particular version or variant.  We still map them to the most common
    // SPDX id, but flag them so callers can warn.
    // See: https://peps.python.org/pep-0639/appendix-mapping-classifiers/
    static AMBIGUOUS: &[&str] = &[
        "Academic Free License (AFL)",
        "Apache Software License",
        "Apple Public Source License",
        "Artistic License",
        "BSD License",
        "GNU Affero General Public License v3",
        "GNU Free Documentation License (FDL)",
        "GNU General Public License (GPL)",
        "GNU General Public License v2 (GPLv2)",
        "GNU General Public License v3 (GPLv3)",
        "GNU Lesser General Public License v2 (LGPLv2)",
        "GNU Lesser General Public License v2 or later (LGPLv2+)",
        "GNU Lesser General Public License v3 (LGPLv3)",
        "GNU Library or Lesser General Public License (LGPL)",
    ];

    // Static lookup for classifiers that do NOT carry the SPDX id in parens,
    // or where the parenthesised form needs correction.
    static MAP: &[(&str, &str)] = &[
        ("Academic Free License (AFL)", "AFL-3.0"),
        ("Apache Software License", "Apache-2.0"),
        ("Artistic License", "Artistic-2.0"),
        ("BSD License", "BSD-3-Clause"),
        ("Boost Software License 1.0 (BSL-1.0)", "BSL-1.0"),
        (
            "CEA CNRS Inria Logiciel Libre License, version 2.1 (CeCILL-2.1)",
            "CECILL-2.1",
        ),
        ("CMU License (MIT-CMU)", "MIT-CMU"),
        (
            "Common Development and Distribution License 1.0 (CDDL-1.0)",
            "CDDL-1.0",
        ),
        ("Common Public License", "CPL-1.0"),
        ("Eclipse Public License 1.0 (EPL-1.0)", "EPL-1.0"),
        ("Eclipse Public License 2.0 (EPL-2.0)", "EPL-2.0"),
        (
            "Educational Community License, Version 2.0 (ECL-2.0)",
            "ECL-2.0",
        ),
        ("Eiffel Forum License", "EFL-2.0"),
        ("European Union Public Licence 1.0 (EUPL 1.0)", "EUPL-1.0"),
        ("European Union Public Licence 1.1 (EUPL 1.1)", "EUPL-1.1"),
        ("European Union Public Licence 1.2 (EUPL 1.2)", "EUPL-1.2"),
        ("GNU Affero General Public License v3", "AGPL-3.0-only"),
        (
            "GNU Affero General Public License v3 or later (AGPLv3+)",
            "AGPL-3.0-or-later",
        ),
        ("GNU Free Documentation License (FDL)", "GFDL-1.3-only"),
        ("GNU General Public License (GPL)", "GPL-2.0-or-later"),
        ("GNU General Public License v2 (GPLv2)", "GPL-2.0-only"),
        (
            "GNU General Public License v2 or later (GPLv2+)",
            "GPL-2.0-or-later",
        ),
        ("GNU General Public License v3 (GPLv3)", "GPL-3.0-only"),
        (
            "GNU General Public License v3 or later (GPLv3+)",
            "GPL-3.0-or-later",
        ),
        (
            "GNU Lesser General Public License v2 (LGPLv2)",
            "LGPL-2.0-only",
        ),
        (
            "GNU Lesser General Public License v2 or later (LGPLv2+)",
            "LGPL-2.0-or-later",
        ),
        (
            "GNU Lesser General Public License v3 (LGPLv3)",
            "LGPL-3.0-only",
        ),
        (
            "GNU Lesser General Public License v3 or later (LGPLv3+)",
            "LGPL-3.0-or-later",
        ),
        (
            "GNU Library or Lesser General Public License (LGPL)",
            "LGPL-2.0-or-later",
        ),
        ("Historical Permission Notice and Disclaimer (HPND)", "HPND"),
        ("IBM Public License", "IPL-1.0"),
        ("ISC License (ISCL)", "ISC"),
        ("MIT License", "MIT"),
        ("MIT No Attribution License (MIT-0)", "MIT-0"),
        ("MirOS License (MirOS)", "MirOS"),
        ("Motosoto License", "Motosoto"),
        ("Mozilla Public License 1.0 (MPL)", "MPL-1.0"),
        ("Mozilla Public License 1.1 (MPL 1.1)", "MPL-1.1"),
        ("Mozilla Public License 2.0 (MPL 2.0)", "MPL-2.0"),
        (
            "Mulan Permissive Software License v2 (MulanPSL-2.0)",
            "MulanPSL-2.0",
        ),
        ("NASA Open Source Agreement v1.3 (NASA-1.3)", "NASA-1.3"),
        ("Nethack General Public License", "NGPL"),
        ("Nokia Open Source License", "Nokia"),
        ("Open Group Test Suite License", "OGTSL"),
        ("Open Software License 3.0 (OSL-3.0)", "OSL-3.0"),
        ("PostgreSQL License", "PostgreSQL"),
        ("Python License (CNRI Python License)", "CNRI-Python"),
        ("Python Software Foundation License", "PSF-2.0"),
        ("Qt Public License (QPL)", "QPL-1.0"),
        ("Ricoh Source Code Public License", "RSCPL"),
        ("SIL Open Font License 1.1 (OFL-1.1)", "OFL-1.1"),
        ("Sleepycat License", "Sleepycat"),
        ("Sun Public License", "SPL-1.0"),
        ("The Unlicense (Unlicense)", "Unlicense"),
        ("Universal Permissive License (UPL)", "UPL-1.0"),
        ("University of Illinois/NCSA Open Source License", "NCSA"),
        ("Vovida Software License 1.0", "VSL-1.0"),
        ("W3C License", "W3C"),
        ("Zero-Clause BSD (0BSD)", "0BSD"),
        ("zlib/libpng License", "Zlib"),
        ("Zope Public License", "ZPL-2.1"),
        // Non-OSI classifiers
        ("Apple Public Source License", "APSL-2.0"),
        ("Blue Oak Model License (BlueOak-1.0.0)", "BlueOak-1.0.0"),
    ];

    MAP.iter()
        .find(|(name, _)| *name == classifier_name)
        .map(|(_, spdx)| {
            let ambiguous = AMBIGUOUS.contains(&classifier_name);
            (*spdx, ambiguous)
        })
}

/// Try to convert a legacy `license` field value to an SPDX identifier.
///
/// Many packages set the `license` field to human-readable strings like
/// `"MIT"` or `"BSD"` rather than a proper SPDX expression. We map the most
/// common values.
/// Returns `(spdx_id, ambiguous)` where `ambiguous` is `true` for values that
/// don't clearly specify a particular version/variant.
fn legacy_license_to_spdx(license: &str) -> Option<(&'static str, bool)> {
    // Values that are ambiguous — they don't specify the particular version or
    // variant, so our mapping is a best-effort guess.
    static AMBIGUOUS: &[&str] = &[
        "BSD",
        "BSD License",
        "GPL",
        "LGPL",
        "Apache",
        "Apache Software License",
        "Artistic",
        "Artistic License",
    ];

    static MAP: &[(&str, &str)] = &[
        ("MIT", "MIT"),
        ("MIT License", "MIT"),
        ("MIT license", "MIT"),
        ("BSD", "BSD-3-Clause"),
        ("BSD License", "BSD-3-Clause"),
        ("BSD license", "BSD-3-Clause"),
        ("BSD-3-Clause", "BSD-3-Clause"),
        ("BSD 3-Clause", "BSD-3-Clause"),
        ("BSD 3-Clause License", "BSD-3-Clause"),
        ("BSD-3-Clause License", "BSD-3-Clause"),
        ("BSD 3-clause", "BSD-3-Clause"),
        ("BSD-2-Clause", "BSD-2-Clause"),
        ("BSD 2-Clause License", "BSD-2-Clause"),
        ("BSD-2-Clause License", "BSD-2-Clause"),
        ("0BSD", "0BSD"),
        ("Apache 2.0", "Apache-2.0"),
        ("Apache-2.0", "Apache-2.0"),
        ("Apache License 2.0", "Apache-2.0"),
        ("Apache License, Version 2.0", "Apache-2.0"),
        ("Apache Software License", "Apache-2.0"),
        ("Apache", "Apache-2.0"),
        ("GPL", "GPL-2.0-or-later"),
        ("GPLv2", "GPL-2.0-only"),
        ("GPLv2+", "GPL-2.0-or-later"),
        ("GPLv3", "GPL-3.0-only"),
        ("GPLv3+", "GPL-3.0-or-later"),
        ("GPL-2.0", "GPL-2.0-only"),
        ("GPL-2.0-only", "GPL-2.0-only"),
        ("GPL-2.0-or-later", "GPL-2.0-or-later"),
        ("GPL-3.0", "GPL-3.0-only"),
        ("GPL-3.0-only", "GPL-3.0-only"),
        ("GPL-3.0-or-later", "GPL-3.0-or-later"),
        ("LGPL", "LGPL-2.0-or-later"),
        ("LGPLv2", "LGPL-2.0-only"),
        ("LGPLv2+", "LGPL-2.0-or-later"),
        ("LGPLv3", "LGPL-3.0-only"),
        ("LGPLv3+", "LGPL-3.0-or-later"),
        ("LGPL-2.1", "LGPL-2.1-only"),
        ("LGPL-2.1-only", "LGPL-2.1-only"),
        ("LGPL-2.1-or-later", "LGPL-2.1-or-later"),
        ("LGPL-3.0", "LGPL-3.0-only"),
        ("LGPL-3.0-only", "LGPL-3.0-only"),
        ("LGPL-3.0-or-later", "LGPL-3.0-or-later"),
        ("ISC", "ISC"),
        ("ISC License", "ISC"),
        ("ISC license (ISCL)", "ISC"),
        ("MPL-2.0", "MPL-2.0"),
        ("MPL 2.0", "MPL-2.0"),
        ("Mozilla Public License 2.0", "MPL-2.0"),
        ("PSF", "PSF-2.0"),
        ("PSF-2.0", "PSF-2.0"),
        ("Python Software Foundation License", "PSF-2.0"),
        ("Unlicense", "Unlicense"),
        ("The Unlicense", "Unlicense"),
        ("Public Domain", "LicenseRef-Public-Domain"),
        ("Artistic", "Artistic-2.0"),
        ("Artistic License", "Artistic-2.0"),
        ("Artistic-2.0", "Artistic-2.0"),
        ("Zlib", "Zlib"),
        ("zlib", "Zlib"),
        ("WTFPL", "WTFPL"),
        ("CC0", "CC0-1.0"),
        ("CC0-1.0", "CC0-1.0"),
        ("EUPL-1.2", "EUPL-1.2"),
        ("ECL-2.0", "ECL-2.0"),
    ];

    MAP.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(license))
        .map(|(_, spdx)| {
            let ambiguous = AMBIGUOUS.iter().any(|a| a.eq_ignore_ascii_case(license));
            (*spdx, ambiguous)
        })
}

/// Result of license extraction: the SPDX expression and an optional warning.
struct ExtractedLicense {
    /// The SPDX license expression.
    spdx: String,
    /// If set, a warning that the license mapping was ambiguous.
    warning: Option<String>,
}

/// Extract the license string from PyPI metadata and convert it to SPDX.
///
/// Checks the following fields in order of preference:
/// 1. `license_expression` — the PEP 639 field, already in SPDX format
/// 2. `license` — the legacy free-text license field (skipped if it looks like
///    a full license text rather than a short identifier)
/// 3. License classifiers — e.g. `"License :: OSI Approved :: MIT License"`
///
/// Values from sources 2 and 3 are mapped to SPDX identifiers using the
/// [PEP 639 classifier mapping](https://peps.python.org/pep-0639/appendix-mapping-classifiers/).
fn extract_license(info: &PyPiInfo) -> Option<ExtractedLicense> {
    // Prefer license_expression (PEP 639) — already SPDX
    if let Some(expr) = &info.license_expression {
        let expr = expr.trim();
        if !expr.is_empty() {
            return Some(ExtractedLicense {
                spdx: expr.to_string(),
                warning: None,
            });
        }
    }

    // Fall back to legacy license field, but only if it looks like a short
    // identifier rather than a full license text.
    if let Some(license) = &info.license {
        let license = license.trim();
        if !license.is_empty() && license.len() < 100 && !license.contains('\n') {
            if let Some((spdx, ambiguous)) = legacy_license_to_spdx(license) {
                let warning = if ambiguous {
                    Some(format!(
                        "WARNING: The PyPI license field value \"{license}\" is ambiguous\n\
                         and may not map to the correct SPDX license (mapped to \"{spdx}\"). Please verify."
                    ))
                } else {
                    None
                };
                return Some(ExtractedLicense {
                    spdx: spdx.to_string(),
                    warning,
                });
            }
            // If we can't map it, return as-is and let the recipe parser validate.
            return Some(ExtractedLicense {
                spdx: license.to_string(),
                warning: Some(format!(
                    "WARNING: The PyPI license field value \"{license}\" could not be mapped\n\
                     to a known SPDX license expression. Please verify and correct."
                )),
            });
        }
    }

    // Fall back to classifiers
    if let Some(classifiers) = &info.classifiers {
        let mapped: Vec<(&str, &str, bool)> = classifiers
            .iter()
            .filter_map(|c| c.strip_prefix("License :: OSI Approved :: "))
            .filter_map(|name| classifier_to_spdx(name).map(|(spdx, amb)| (name, spdx, amb)))
            .collect();

        if !mapped.is_empty() {
            let spdx_ids: Vec<&str> = mapped.iter().map(|(_, spdx, _)| *spdx).collect();
            let ambiguous: Vec<&str> = mapped
                .iter()
                .filter(|(_, _, amb)| *amb)
                .map(|(name, _, _)| *name)
                .collect();

            let warning = if !ambiguous.is_empty() {
                let classifiers_str = ambiguous
                    .iter()
                    .map(|c| format!("  - {c}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(format!(
                    "WARNING: The following PyPI classifier(s) are ambiguous (per PEP 639)\n\
                     and may not map to the correct SPDX license. Please verify:\n\
                     {classifiers_str}"
                ))
            } else {
                None
            };

            return Some(ExtractedLicense {
                spdx: spdx_ids.join(" OR "),
                warning,
            });
        }
    }

    None
}

/// Create a `serialize::Recipe` from the provided options and PyPI metadata.
pub async fn create_recipe(
    opts: &PyPIOpts,
    metadata: &PyPiMetadata,
    client: &reqwest::Client,
) -> miette::Result<serialize::Recipe> {
    let mut recipe = serialize::Recipe::default();
    recipe
        .context
        .insert("version".to_string(), metadata.info.version.clone());
    recipe
        .context
        .insert("build_number".to_string(), "0".to_string());
    recipe.package.name = metadata.info.name.to_lowercase();
    recipe.package.version = "${{ version }}".to_string();
    recipe.build.number = "${{ build_number }}".to_string();

    // Check if we're using the standard PyPI
    let is_default_pypi = opts
        .pypi_index_urls
        .iter()
        .any(|url| url.starts_with("https://pypi.org"));

    // replace URL with the shorter version that does not contain the hash if using the standard PyPI
    let release_url = if is_default_pypi
        && metadata
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
        let wheel_info = extract_wheel_info(wheel_url, client).await?;
        if !wheel_info.entry_points.is_empty() {
            recipe.build.python.entry_points = wheel_info.entry_points;
        }
        if !wheel_info.license_files.is_empty() {
            recipe.about.license_file = wheel_info.license_files;
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
            imports: vec![metadata.info.name.replace('-', "_")],
            pip_check: true,
        },
    }));

    // Set metadata
    recipe.about.summary = metadata.info.summary.clone();
    recipe.about.description = metadata.info.description.clone();
    recipe.about.homepage = metadata.info.home_page.clone();
    if let Some(extracted) = extract_license(&metadata.info) {
        if let Some(warning) = &extracted.warning {
            tracing::warn!(
                "Ambiguous PyPI license classifier for '{}': mapped to '{}' — please verify.\n{}",
                metadata.info.name,
                extracted.spdx,
                warning
            );
        }
        recipe.about.license_warning = extracted.warning;
        recipe.about.license = Some(extracted.spdx);
    }

    if let Some(urls) = &metadata.info.project_urls {
        recipe.about.repository = urls.get("Source Code").cloned();
        recipe.about.documentation = urls.get("Documentation").cloned();
    }

    Ok(recipe)
}

/// Generate a recipe YAML string for a PyPI package.
pub async fn generate_pypi_recipe_string(opts: &PyPIOpts) -> miette::Result<String> {
    let client = reqwest::Client::new();
    let metadata = fetch_pypi_metadata(opts, &client).await?;
    let recipe = create_recipe(opts, &metadata, &client).await?;
    let string = format!("{}", recipe);
    Ok(post_process_markers(string))
}

/// Generate a recipe for a PyPI package and either write it to disk or print it.
///
/// If `opts.tree` is set, recursively generates recipes for runtime dependencies.
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
            pypi_index_urls: vec!["https://pypi.org/pypi".to_string()],
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
            pypi_index_urls: vec!["https://pypi.org/pypi".to_string()],
        };

        let client = reqwest::Client::new();
        let metadata = fetch_pypi_metadata(&opts, &client).await.unwrap();
        let recipe = create_recipe(&opts, &metadata, &client).await.unwrap();

        assert_yaml_snapshot!(recipe);
    }

    /// Helper: extract just the SPDX string from `extract_license`.
    fn extract_spdx(info: &PyPiInfo) -> Option<String> {
        extract_license(info).map(|e| e.spdx)
    }

    /// Helper: extract the warning from `extract_license`.
    fn extract_warning(info: &PyPiInfo) -> Option<String> {
        extract_license(info).and_then(|e| e.warning)
    }

    #[test]
    fn test_extract_license_prefers_license_expression() {
        let info = PyPiInfo {
            license_expression: Some("GPL-2.0-or-later".into()),
            license: Some("GPL".into()),
            classifiers: Some(vec![
                "License :: OSI Approved :: GNU General Public License v2 or later (GPLv2+)".into(),
            ]),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("GPL-2.0-or-later".into()));
        assert_eq!(extract_warning(&info), None);
    }

    #[test]
    fn test_extract_license_falls_back_to_license() {
        let info = PyPiInfo {
            license: Some("MIT".into()),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("MIT".into()));
    }

    #[test]
    fn test_extract_license_skips_long_license_text() {
        let info = PyPiInfo {
            license: Some("A very long license text that is clearly not a short SPDX identifier but rather the full contents of a license file which we should not use".into()),
            classifiers: Some(vec![
                "License :: OSI Approved :: MIT License".into(),
            ]),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("MIT".into()));
        // MIT License is not ambiguous
        assert_eq!(extract_warning(&info), None);
    }

    #[test]
    fn test_extract_license_falls_back_to_classifiers() {
        let info = PyPiInfo {
            classifiers: Some(vec![
                "License :: OSI Approved :: BSD License".into(),
                "Programming Language :: Python :: 3".into(),
            ]),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("BSD-3-Clause".into()));
        // BSD License is ambiguous per PEP 639
        assert!(extract_warning(&info).is_some());
    }

    #[test]
    fn test_extract_license_maps_legacy_field_to_spdx() {
        let info = PyPiInfo {
            license: Some("MIT License".into()),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("MIT".into()));
    }

    #[test]
    fn test_extract_license_multiple_classifiers() {
        let info = PyPiInfo {
            classifiers: Some(vec![
                "License :: OSI Approved :: MIT License".into(),
                "License :: OSI Approved :: Apache Software License".into(),
            ]),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("MIT OR Apache-2.0".into()));
        // Apache Software License is ambiguous
        let warning = extract_warning(&info).unwrap();
        assert!(warning.contains("Apache Software License"));
        // MIT License is not ambiguous, so it shouldn't be in the warning
        assert!(!warning.contains("MIT License"));
    }

    #[test]
    fn test_classifier_to_spdx_coverage() {
        // Verify a selection of common mappings
        assert_eq!(classifier_to_spdx("MIT License"), Some(("MIT", false)));
        assert_eq!(
            classifier_to_spdx("BSD License"),
            Some(("BSD-3-Clause", true))
        );
        assert_eq!(
            classifier_to_spdx("Apache Software License"),
            Some(("Apache-2.0", true))
        );
        assert_eq!(
            classifier_to_spdx("GNU General Public License v3 (GPLv3)"),
            Some(("GPL-3.0-only", true))
        );
        assert_eq!(
            classifier_to_spdx("Mozilla Public License 2.0 (MPL 2.0)"),
            Some(("MPL-2.0", false))
        );
        assert_eq!(
            classifier_to_spdx("The Unlicense (Unlicense)"),
            Some(("Unlicense", false))
        );
        assert_eq!(
            classifier_to_spdx("Zero-Clause BSD (0BSD)"),
            Some(("0BSD", false))
        );
        // Unknown classifier returns None
        assert_eq!(classifier_to_spdx("Some Unknown License"), None);
    }

    #[test]
    fn test_extract_license_maps_bsd_3clause_license() {
        // This is the format used by ipywidgets and many Jupyter packages.
        let info = PyPiInfo {
            license: Some("BSD 3-Clause License".into()),
            classifiers: Some(vec!["License :: OSI Approved :: BSD License".into()]),
            ..Default::default()
        };
        // Should map via legacy field (higher priority than classifiers)
        assert_eq!(extract_spdx(&info), Some("BSD-3-Clause".into()));
        // "BSD 3-Clause License" is NOT ambiguous — it's specific
        assert_eq!(extract_warning(&info), None);
    }

    #[test]
    fn test_extract_license_legacy_ambiguous_bsd() {
        let info = PyPiInfo {
            license: Some("BSD".into()),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("BSD-3-Clause".into()));
        let warning = extract_warning(&info).unwrap();
        assert!(warning.contains("BSD"));
        assert!(warning.contains("ambiguous"));
    }

    #[test]
    fn test_extract_license_legacy_unmapped_warns() {
        let info = PyPiInfo {
            license: Some("Custom License v42".into()),
            ..Default::default()
        };
        assert_eq!(extract_spdx(&info), Some("Custom License v42".into()));
        let warning = extract_warning(&info).unwrap();
        assert!(warning.contains("Custom License v42"));
        assert!(warning.contains("could not be mapped"));
    }

    #[test]
    fn test_extract_license_none_when_empty() {
        let info = PyPiInfo::default();
        assert!(extract_license(&info).is_none());
    }

    #[tokio::test]
    async fn test_hyphenated_imports_are_sanitized() {
        let opts = PyPIOpts {
            package: "file-read-backwards".into(),
            version: Some("3.2.0".into()),
            write: false,
            use_mapping: true,
            tree: false,
            pypi_index_urls: vec!["https://pypi.org/pypi".to_string()],
        };

        let client = reqwest::Client::new();
        let metadata = fetch_pypi_metadata(&opts, &client).await.unwrap();
        let recipe = create_recipe(&opts, &metadata, &client).await.unwrap();

        let python_test = recipe
            .tests
            .iter()
            .find_map(|t| {
                if let Test::Python(pt) = t {
                    Some(pt)
                } else {
                    None
                }
            })
            .expect("expected a Python test");
        assert_eq!(python_test.python.imports, vec!["file_read_backwards"]);
    }
}
