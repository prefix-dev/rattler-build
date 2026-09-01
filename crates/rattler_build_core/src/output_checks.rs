//! Cross-output consistency checks that run after all outputs of a build are packaged.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
};

use fs_err as fs;

use crate::{packaging::file_mapper::filter_file, staging::StagingCacheMetadata, types::Output};

/// How many offending files to list per finding before truncating.
const MAX_LISTED_FILES: usize = 10;

fn format_file_list(files: &BTreeSet<PathBuf>) -> String {
    let listed = files
        .iter()
        .take(MAX_LISTED_FILES)
        .map(|p| format!("  - {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    if files.len() > MAX_LISTED_FILES {
        format!(
            "{}\n  … and {} more",
            listed,
            files.len() - MAX_LISTED_FILES
        )
    } else {
        listed
    }
}

fn report(findings: Vec<String>, error: bool, flag: &str) -> miette::Result<()> {
    if findings.is_empty() {
        return Ok(());
    }
    if error {
        Err(miette::miette!("{}", findings.join("\n\n")))
    } else {
        for finding in &findings {
            tracing::warn!("{}", finding);
        }
        tracing::warn!("Pass `{}` to turn this warning into an error.", flag);
        Ok(())
    }
}

/// Warn (or error) when two outputs of the same recipe package the same file.
///
/// Installing both packages into one environment would clobber the file, and with
/// staging caches this usually means an output forgot to narrow its `files` selection.
/// Compared on prefix-relative paths as selected for packaging (before noarch path
/// remapping), so a noarch and an arch output that ship the same file are caught too.
/// Outputs with the same package name are never compared: they cannot be co-installed.
pub fn check_overlapping_files(outputs: &[Output], error: bool) -> miette::Result<()> {
    let mut by_recipe: HashMap<&std::path::Path, Vec<&Output>> = HashMap::new();
    for output in outputs {
        by_recipe
            .entry(output.build_configuration.directories.recipe_path.as_path())
            .or_default()
            .push(output);
    }

    let mut findings = Vec::new();
    for outputs in by_recipe.values() {
        // Variants of the same pair would repeat the same finding, so keep one per name pair.
        let mut seen_pairs = HashSet::new();
        for (i, a) in outputs.iter().enumerate() {
            let Some(files_a) = a.packaged_prefix_files() else {
                continue;
            };
            for b in &outputs[i + 1..] {
                if a.name() == b.name() {
                    continue;
                }
                let Some(files_b) = b.packaged_prefix_files() else {
                    continue;
                };
                let overlap: BTreeSet<PathBuf> = files_a.intersection(&files_b).cloned().collect();
                if overlap.is_empty() || !seen_pairs.insert((a.name().clone(), b.name().clone())) {
                    continue;
                }
                b.record_warning(&format!(
                    "packages {} file(s) that are also packaged by output '{}'",
                    overlap.len(),
                    a.name().as_normalized(),
                ));
                findings.push(format!(
                    "Outputs '{}' and '{}' both package {} file(s), which will clobber each other on installation:\n{}",
                    a.name().as_normalized(),
                    b.name().as_normalized(),
                    overlap.len(),
                    format_file_list(&overlap),
                ));
            }
        }
    }

    report(findings, error, "--error-overlapping-files")
}

/// Warn (or error) when files in a staging cache were not packaged by any output
/// that inherits from it.
///
/// Caches are compared per cache directory (one per staging output and variant
/// selection). Pass every output of the run, before any skipping: a cache is only
/// judged when all of its inheriting outputs were packaged, since a skipped or
/// failed consumer leaves usage unknown.
pub fn check_unused_staging_files(outputs: &[Output], error: bool) -> miette::Result<()> {
    // cache dir -> outputs expected to consume it
    let mut consumers: HashMap<PathBuf, Vec<&Output>> = HashMap::new();
    for output in outputs {
        let Some(cache_dir) = inherited_cache_dir(output) else {
            continue;
        };
        consumers.entry(cache_dir).or_default().push(output);
    }

    let mut findings = Vec::new();
    for (cache_dir, consumers) in consumers {
        // Judge only caches whose consumers were all packaged in this run.
        let files: Option<Vec<BTreeSet<PathBuf>>> = consumers
            .iter()
            .map(|o| o.packaged_prefix_files())
            .collect();
        let Some(files) = files else {
            continue;
        };

        let Ok(metadata) = fs::read_to_string(cache_dir.join("metadata.json")) else {
            continue;
        };
        let Ok(metadata) = serde_json::from_str::<StagingCacheMetadata>(&metadata) else {
            continue;
        };

        let used: HashSet<&PathBuf> = files.iter().flatten().collect();
        let unused: BTreeSet<PathBuf> = metadata
            .prefix_files
            .iter()
            // files like CACHEDIR.TAG or .pyo are never packaged
            .filter(|f| !filter_file(f) && !used.contains(f))
            .cloned()
            .collect();
        if unused.is_empty() {
            continue;
        }

        if let Some(first) = consumers.first() {
            first.record_warning(&format!(
                "{} file(s) from staging cache '{}' were not included in any output",
                unused.len(),
                metadata.name,
            ));
        }
        findings.push(format!(
            "{} file(s) from staging cache '{}' were not included in any of the outputs that inherit from it ({}):\n{}",
            unused.len(),
            metadata.name,
            consumers
                .iter()
                .map(|o| o.name().as_normalized().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            format_file_list(&unused),
        ));
    }

    report(findings, error, "--error-unused-staging-files")
}

/// The cache directory of the staging cache this output inherits from, if any.
fn inherited_cache_dir(output: &Output) -> Option<PathBuf> {
    let inherits = output.recipe.inherits_from.as_ref()?;
    let staging = output
        .recipe
        .staging_caches
        .iter()
        .find(|s| s.name == inherits.cache_name)?;
    let cache_key = output.staging_cache_key(staging).ok()?;
    Some(
        output
            .build_configuration
            .directories
            .cache_dir
            .join(format!("staging_{}", cache_key)),
    )
}
