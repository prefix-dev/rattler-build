use std::{future::IntoFuture, path::Path};

use crate::{metadata::PlatformWithVirtualPackages, packaging::Files, tool_configuration};
use comfy_table::Table;
use console::style;
use futures::FutureExt;
use indicatif::HumanBytes;
use itertools::Itertools;
use miette::{IntoDiagnostic, WrapErr};
use rattler::install::{DefaultProgressFormatter, IndicatifReporter, Installer};
use rattler_conda_types::{
    Channel, ChannelUrl, MatchSpec, Platform, PrefixRecord, RepoDataRecord,
    package::FileMode,
};
use rattler_solve::{ChannelPriority, SolveStrategy, SolverImpl, SolverTask, resolvo::Solver};

use super::reporters::GatewayReporter;

fn print_as_table(packages: &[RepoDataRecord]) {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
    table.set_header(vec![
        "Package", "Version", "Build", "Channel", "Size",
        // "License",
    ]);
    let column = table.column_mut(4).expect("This should be column five");
    column.set_cell_alignment(comfy_table::CellAlignment::Right);

    for package in packages
        .iter()
        .sorted_by_key(|p| p.package_record.name.as_normalized())
    {
        let channel_short = if package.channel.as_deref().unwrap_or_default().contains('/') {
            package
                .channel
                .as_ref()
                .and_then(|s| s.rsplit('/').find(|s| !s.is_empty()))
                .expect("expected channel to be defined and contain '/'")
                .to_string()
        } else {
            package.channel.as_deref().unwrap_or_default().to_string()
        };

        table.add_row([
            package.package_record.name.as_normalized().to_string(),
            package.package_record.version.to_string(),
            package.package_record.build.clone(),
            channel_short,
            HumanBytes(package.package_record.size.unwrap_or(0)).to_string(),
            // package.package_record.license.clone().unwrap_or_else(|| "".to_string()),
        ]);
    }

    tracing::info!("\n{table}");
}

#[allow(clippy::too_many_arguments)]
pub async fn solve_environment(
    name: &str,
    specs: &[MatchSpec],
    target_platform: &PlatformWithVirtualPackages,
    channels: &[ChannelUrl],
    tool_configuration: &tool_configuration::Configuration,
    channel_priority: ChannelPriority,
    solve_strategy: SolveStrategy,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
) -> miette::Result<Vec<RepoDataRecord>> {
    let vp_string = format!("[{}]", target_platform.virtual_packages.iter().format(", "));

    tracing::info!("\nResolving {name} environment:\n");
    tracing::info!(
        "  Platform: {} {}",
        target_platform.platform,
        style(vp_string).dim()
    );
    tracing::info!("  Channels: ");
    for channel in channels {
        tracing::info!(
            "   - {}",
            tool_configuration
                .channel_config
                .canonical_name(channel.url())
        );
    }
    tracing::info!("  Specs:");
    for spec in specs {
        tracing::info!("   - {}", spec);
    }

    let repo_data = load_repodatas(
        channels,
        target_platform.platform,
        specs,
        tool_configuration,
    )
    .await?;

    // Now that we parsed and downloaded all information, construct the packaging
    // problem that we need to solve. We do this by constructing a
    // `SolverProblem`. This encapsulates all the information required to be
    // able to solve the problem.
    let solver_task = SolverTask {
        virtual_packages: target_platform.virtual_packages.clone(),
        specs: specs.to_vec(),
        channel_priority,
        strategy: solve_strategy,
        exclude_newer,
        ..SolverTask::from_iter(&repo_data)
    };

    // Next, use a solver to solve this specific problem. This provides us with all
    // the operations we need to apply to our environment to bring it up to
    // date.
    let solver_result = tool_configuration
        .fancy_log_handler
        .wrap_in_progress("solving", move || Solver.solve(solver_task))
        .into_diagnostic()?;

    // Print the result as a table
    print_as_table(&solver_result.records);

    Ok(solver_result.records)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_environment(
    name: &str,
    specs: &[MatchSpec],
    target_platform: &PlatformWithVirtualPackages,
    target_prefix: &Path,
    channels: &[ChannelUrl],
    tool_configuration: &tool_configuration::Configuration,
    channel_priority: ChannelPriority,
    solve_strategy: SolveStrategy,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
) -> miette::Result<Vec<RepoDataRecord>> {
    let required_packages = solve_environment(
        name,
        specs,
        target_platform,
        channels,
        tool_configuration,
        channel_priority,
        solve_strategy,
        exclude_newer,
    )
    .await?;

    install_packages(
        name,
        &required_packages,
        target_platform.platform,
        target_prefix,
        tool_configuration,
    )
    .await?;

    Ok(required_packages)
}

/// Load repodata from channels. Only includes necessary records for platform &
/// specs.
pub async fn load_repodatas(
    channels: &[ChannelUrl],
    target_platform: Platform,
    specs: &[MatchSpec],
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<Vec<rattler_repodata_gateway::RepoData>> {
    let channels = channels
        .iter()
        .map(|url| Channel::from_url(url.clone()))
        .collect::<Vec<_>>();

    let result = tool_configuration
        .repodata_gateway
        .query(
            channels,
            [target_platform, Platform::NoArch],
            specs.to_vec(),
        )
        .with_reporter(
            GatewayReporter::builder()
                .with_multi_progress(
                    tool_configuration
                        .fancy_log_handler
                        .multi_progress()
                        .clone(),
                )
                .with_progress_template(tool_configuration.fancy_log_handler.default_bytes_style())
                .with_finish_template(
                    tool_configuration
                        .fancy_log_handler
                        .finished_progress_style(),
                )
                .finish(),
        )
        .recursive(true)
        .into_future()
        .boxed()
        .await
        .into_diagnostic()?;

    tool_configuration
        .fancy_log_handler
        .multi_progress()
        .clear()
        .unwrap();

    Ok(result)
}

/// Remove stale `.pyc` files for any `.py` file that underwent text-mode prefix
/// substitution during installation.
///
/// # Why this is needed
///
/// When rattler installs a package it performs two actions for text-mode prefix
/// files:
///   1. Replaces every occurrence of the build-time placeholder path with the
///      actual target prefix (text substitution).
///   2. Restores the original file modification time (mtime) so that the
///      installed file looks identical to the cached original from the
///      perspective of file-system timestamps.
///
/// For Python `.py` files the package often also ships a pre-compiled
/// `__pycache__/<module>.cpython-XY.pyc` sibling.  That `.pyc` file is
/// **not** listed with a `prefix_placeholder` in `paths.json` (correctly so –
/// the binary bytecode is not textually substitutable), so rattler installs it
/// as-is with its original mtime preserved.
///
/// Python validates a cached `.pyc` by comparing **two** values recorded in
/// the `.pyc` header against the on-disk source:
///   * the source mtime  (bytes 8–11 of the `.pyc` header), and
///   * the source size   (bytes 12–15 of the `.pyc` header).
///
/// Both values are written at the time the `.pyc` was built (by conda-build on
/// the conda-forge CI).  After rattler's installation:
///
///   * **mtime** → always matches, because rattler restores the original
///     timestamp via `filetime::set_file_times` even after rewriting the
///     file's content.
///   * **size** → *normally* does not match (the file shrinks when the
///     255-character placeholder is replaced by a shorter real prefix).
///     Python therefore detects a size mismatch and regenerates the `.pyc`
///     from the (correctly substituted) `.py` – this is the happy path.
///
/// However, rattler-build pads its own host-environment prefix to exactly
/// **255 characters** (see `src/types/directories.rs`) to enable binary
/// relinking.  conda-forge likewise pads its build prefixes to 255 characters.
/// When the old and new prefixes are the *same length*, the text substitution
/// does not change the file size.  Combined with the preserved mtime, Python's
/// validator sees:
///
///   * mtime  : ✓ matches
///   * size   : ✓ matches  ← false positive!
///
/// Python therefore trusts the stale `.pyc`, which still contains the
/// conda-forge CI path as a bytecode string constant.  Any call such as
/// `sysconfig.get_config_var("prefix")` then returns the wrong stale path
/// instead of the actual rattler-build host prefix.
///
/// The fix applied here: after installation, find every `.py` file that had
/// text-mode prefix substitution applied (by inspecting the conda-meta
/// `PrefixRecord` entries) and **delete** its cached `.pyc` companions in
/// `__pycache__/`.  Python will regenerate them from the correctly-substituted
/// source on the next import.
///
/// See <https://github.com/prefix-dev/rattler-build/issues/2147> for the
/// original bug report.
fn remove_stale_pyc_files(prefix: &Path) -> miette::Result<()> {
    let records: Vec<PrefixRecord> = PrefixRecord::collect_from_prefix(prefix).into_diagnostic()?;

    let mut removed = 0usize;
    for record in &records {
        for entry in &record.paths_data.paths {
            // We only care about text-mode substituted files.
            // In PrefixRecord's PathsEntry, `prefix_placeholder` is Option<String>
            // (the placeholder value) and `file_mode` is a separate Option<FileMode>.
            let is_text_substituted = entry.prefix_placeholder.is_some()
                && entry.file_mode == Some(FileMode::Text);
            if !is_text_substituted {
                continue;
            }

            // Only `.py` source files have corresponding `.pyc` companions.
            if entry.relative_path.extension().and_then(|e| e.to_str()) != Some("py") {
                continue;
            }

            let py_path = prefix.join(&entry.relative_path);
            let Some(parent) = py_path.parent() else {
                continue;
            };
            let cache_dir = parent.join("__pycache__");
            if !cache_dir.is_dir() {
                continue;
            }

            let stem = match py_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_owned(),
                None => continue,
            };

            // Remove every `<stem>.*.pyc` file (e.g. cpython-311, cpython-312…).
            let entries = match std::fs::read_dir(&cache_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for cache_entry in entries.flatten() {
                let cache_path = cache_entry.path();
                let name = match cache_path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_owned(),
                    None => continue,
                };
                if name.starts_with(&format!("{stem}."))
                    && cache_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e == "pyc")
                {
                    if let Err(err) = std::fs::remove_file(&cache_path) {
                        tracing::warn!(
                            "Failed to remove stale .pyc file {}: {}",
                            cache_path.display(),
                            err
                        );
                    } else {
                        tracing::debug!("Removed stale .pyc: {}", cache_path.display());
                        removed += 1;
                    }
                }
            }
        }
    }

    if removed > 0 {
        tracing::info!(
            "Removed {} stale .pyc file(s) to prevent Python from using cached \
             bytecode with an outdated prefix path.",
            removed
        );
    }

    Ok(())
}

pub async fn install_packages(
    name: &str,
    required_packages: &[RepoDataRecord],
    target_platform: Platform,
    target_prefix: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<()> {
    // Make sure the target prefix exists, regardless of whether we'll actually
    // install anything in there.
    let prefix = rattler_conda_types::prefix::Prefix::create(target_prefix)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "failed to create target prefix: {}",
                target_prefix.display()
            )
        })?;

    if !prefix.path().join("conda-meta/history").exists() {
        // Create an empty history file if it doesn't exist
        fs_err::create_dir_all(prefix.path().join("conda-meta")).into_diagnostic()?;
        fs_err::File::create(prefix.path().join("conda-meta/history")).into_diagnostic()?;
    }

    let installed_packages = PrefixRecord::collect_from_prefix(target_prefix).into_diagnostic()?;

    if !installed_packages.is_empty() && name.starts_with("host") {
        // we have to clean up extra files in the prefix
        let extra_files =
            Files::from_prefix(target_prefix, &Default::default(), &Default::default())
                .into_diagnostic()?;

        tracing::info!(
            "Cleaning up {} files in the prefix from a previous build.",
            extra_files.new_files.len()
        );

        for f in extra_files.new_files {
            if !f.is_dir() {
                fs_err::remove_file(target_prefix.join(f)).into_diagnostic()?;
            }
        }
    }

    tracing::info!("\nInstalling {name} environment\n");
    Installer::new()
        .with_download_client(tool_configuration.client.get_client().clone())
        .with_target_platform(target_platform)
        .with_execute_link_scripts(true)
        .with_package_cache(tool_configuration.package_cache.clone())
        .with_installed_packages(installed_packages)
        .with_io_concurrency_limit(tool_configuration.io_concurrency_limit.unwrap_or_default())
        .with_reporter(
            IndicatifReporter::builder()
                .with_multi_progress(
                    tool_configuration
                        .fancy_log_handler
                        .multi_progress()
                        .clone(),
                )
                .with_formatter(
                    DefaultProgressFormatter::default()
                        .with_prefix(tool_configuration.fancy_log_handler.with_indent_levels("")),
                )
                .finish(),
        )
        .install(&target_prefix, required_packages.to_owned())
        .await
        .into_diagnostic()?;

    // Remove stale `.pyc` files for any `.py` file that underwent text-mode
    // prefix substitution.  This is needed because rattler preserves the
    // original mtime after substitution AND rattler-build pads its host env
    // prefix to the same 255-char length that conda-forge uses for its build
    // prefixes.  The combination means Python's two-factor `.pyc` validation
    // (mtime + source size) would pass for the stale cached bytecode and
    // Python would use it instead of the correctly-substituted source.
    // See the doc-comment on `remove_stale_pyc_files` for the full analysis.
    remove_stale_pyc_files(target_prefix)?;

    tracing::info!(
        "{} Successfully updated the {name} environment",
        console::style(console::Emoji("✔", "")).green(),
    );

    Ok(())
}
