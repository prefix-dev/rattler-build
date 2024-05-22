use comfy_table::Table;
use futures::StreamExt;

use indicatif::{HumanBytes, ProgressBar};
use rattler::install::Installer;
use rattler_conda_types::{
    Channel, GenericVirtualPackage, MatchSpec, Platform, PrefixRecord, RepoDataRecord,
};
use rattler_repodata_gateway::{
    fetch::{CacheResult, FetchRepoDataError, FetchRepoDataOptions},
    Reporter,
};
use rattler_repodata_gateway::{sparse::SparseRepoData, Gateway};
use rattler_solve::{resolvo::Solver, ChannelPriority, SolveStrategy, SolverImpl, SolverTask};
use reqwest_middleware::ClientWithMiddleware;
use url::Url;

use crate::{console_utils::LoggingOutputHandler, tool_configuration};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

fn print_as_table(packages: &Vec<RepoDataRecord>) {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
    table.set_header(vec![
        "Package", "Version", "Build", "Channel", "Size",
        // "License",
    ]);

    for package in packages {
        let channel_short = if package.channel.contains('/') {
            package
                .channel
                .rsplit('/')
                .find(|s| !s.is_empty())
                .expect("yep will crash if ")
                .to_string()
        } else {
            package.channel.to_string()
        };

        table.add_row(vec![
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

pub async fn create_environment(
    specs: &[MatchSpec],
    target_platform: &Platform,
    target_prefix: &Path,
    channels: &[Url],
    tool_configuration: &tool_configuration::Configuration,
) -> anyhow::Result<Vec<RepoDataRecord>> {
    // Parse the specs from the command line. We do this explicitly instead of allow clap to deal
    // with this because we need to parse the `channel_config` when parsing matchspecs.

    tracing::info!("\nResolving environment for:\n");
    tracing::info!("  Platform: {}", target_platform);
    tracing::info!("  Channels: ");
    for channel in channels {
        tracing::info!(
            "   - {}",
            tool_configuration.channel_config.canonical_name(channel)
        );
    }
    tracing::info!("  Specs:");
    for spec in specs {
        tracing::info!("   - {}", spec);
    }

    let installed_packages = PrefixRecord::collect_from_prefix(target_prefix)?;

    let download_client = tool_configuration.client.clone();

    let cache_dir = rattler::default_cache_dir()?;

    // Get the package names from the matchspecs so we can only load the package records that we need.
    let gateway = Gateway::builder()
        .with_cache_dir(cache_dir.join("repodata"))
        .with_client(download_client.clone())
        .finish();

    // Determine the channels to use from the command line or select the default. Like matchspecs
    // this also requires the use of the `channel_config` so we have to do this manually.
    let channel_config = ChannelConfig::default_with_root_dir(std::env::current_dir()?);
    let channels = channels
        .into_iter()
        .map(|channel_str| Channel::from_str(channel_str, &channel_config))
        .collect::<Result<Vec<_>, _>>()?;

    let repo_data = gateway
        .query(
            channels,
            [*target_platform, Platform::NoArch],
            specs.to_vec(),
        )
        .recursive(true)
        .await?;

    // Determine virtual packages of the system. These packages define the capabilities of the
    // system. Some packages depend on these virtual packages to indicate compatibility with the
    // hardware of the system.
    let virtual_packages = tool_configuration.fancy_log_handler.wrap_in_progress(
        "determining virtual packages",
        move || {
            rattler_virtual_packages::VirtualPackage::current().map(|vpkgs| {
                vpkgs
                    .iter()
                    .map(|vpkg| GenericVirtualPackage::from(vpkg.clone()))
                    .collect::<Vec<_>>()
            })
        },
    )?;

    // Now that we parsed and downloaded all information, construct the packaging problem that we
    // need to solve. We do this by constructing a `SolverProblem`. This encapsulates all the
    // information required to be able to solve the problem.
    let solver_task = SolverTask {
        locked_packages: installed_packages
            .iter()
            .map(|record| record.repodata_record.clone())
            .collect(),
        virtual_packages,
        specs: specs.to_vec(),
        pinned_packages: Vec::new(),
        timeout: None,
        channel_priority: ChannelPriority::Strict,
        exclude_newer: None,
        strategy: SolveStrategy::Highest,
        ..SolverTask::from_iter(&repo_data)
    };

    // Next, use a solver to solve this specific problem. This provides us with all the operations
    // we need to apply to our environment to bring it up to date.
    let required_packages = tool_configuration
        .fancy_log_handler
        .wrap_in_progress("solving", move || Solver.solve(solver_task))?;

    if !tool_configuration.render_only {
        install_packages(
            &required_packages,
            target_platform,
            target_prefix,
            tool_configuration,
        )
        .await?;
    } else {
        tracing::info!("skipping installation when --render-only is used");
    }

    Ok(required_packages)
}

/// Load repodata for given matchspecs and channels.
pub async fn load_repodatas(
    channels: &[Url],
    target_platform: &Platform,
    tool_configuration: &tool_configuration::Configuration,
    specs: &[MatchSpec],
) -> Result<(PathBuf, Vec<Vec<RepoDataRecord>>), anyhow::Error> {
    let cache_dir = rattler::default_cache_dir()?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| anyhow::anyhow!("could not create cache directory: {}", e))?;

    let platforms = [Platform::NoArch, *target_platform];
    let channel_urls = channels
        .iter()
        .flat_map(|channel| {
            platforms
                .iter()
                .map(move |platform| (channel.clone(), *platform))
        })
        .collect::<Vec<_>>();

    let repodata_cache_path = cache_dir.join("repodata");

    let channel_and_platform_len = channel_urls.len();
    let repodata_download_client = tool_configuration.client.clone();
    let sparse_repo_datas = futures::stream::iter(channel_urls)
        .map(move |(url, platform)| {
            let repodata_cache = repodata_cache_path.clone();
            let download_client = repodata_download_client.clone();
            async move {
                fetch_repo_data_records_with_progress(
                    Channel::from_url(url),
                    platform,
                    &repodata_cache,
                    download_client.clone(),
                    tool_configuration.fancy_log_handler.clone(),
                    platform != Platform::NoArch,
                )
                .await
            }
        })
        .buffered(channel_and_platform_len)
        .collect::<Vec<_>>()
        .await
        // Collect into another iterator where we extract the first erroneous result
        .into_iter()
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;

    let package_names = specs.iter().filter_map(|spec| spec.name.clone());
    let repodatas = tool_configuration
        .fancy_log_handler
        .wrap_in_progress("parsing repodata", move || {
            SparseRepoData::load_records_recursive(&sparse_repo_datas, package_names, None)
        })?;

    Ok((cache_dir, repodatas))
}

pub async fn install_packages(
    required_packages: &Vec<RepoDataRecord>,
    target_platform: &Platform,
    target_prefix: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> anyhow::Result<()> {
    let installed_packages = vec![];

    tracing::info!(
        "The following packages will be installed ({} total):",
        required_packages.len()
    );
    print_as_table(required_packages);

    if !required_packages.is_empty() {
        Installer::new()
            .with_download_client(tool_configuration.client.clone())
            .with_target_platform(*target_platform)
            .with_installed_packages(installed_packages)
            .with_execute_link_scripts(true)
            // .with_reporter(reporter) // TODO implement reporter in fancy progress
            .install(&target_prefix, required_packages.clone())
            .await?;

        tracing::info!(
            "{} Successfully updated the environment",
            console::style(console::Emoji("✔", "")).green(),
        );
    } else {
        tracing::info!(
            "{} Already up to date",
            console::style(console::Emoji("✔", "")).green(),
        );
    }

    Ok(())
}

struct DownloadProgressReporter {
    progress_bar: ProgressBar,
}

impl Reporter for DownloadProgressReporter {
    fn on_download_progress(&self, _url: &Url, _index: usize, bytes: usize, total: Option<usize>) {
        self.progress_bar.set_length(total.unwrap_or(bytes) as u64);
        self.progress_bar.set_position(bytes as u64);
    }
}

/// Given a channel and platform, download and cache the `repodata.json` for it. This function
/// reports its progress via a CLI progressbar.
async fn fetch_repo_data_records_with_progress(
    channel: Channel,
    platform: Platform,
    repodata_cache: &Path,
    client: ClientWithMiddleware,
    fancy_log_handler: LoggingOutputHandler,
    allow_not_found: bool,
) -> anyhow::Result<Option<SparseRepoData>> {
    // Create a progress bar
    let progress_bar = fancy_log_handler.add_progress_bar(
        indicatif::ProgressBar::new(1)
            .with_finish(indicatif::ProgressFinish::AndLeave)
            .with_prefix(format!("{}/{platform}", friendly_channel_name(&channel)))
            .with_style(fancy_log_handler.default_bytes_style()),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let progress_reporter = DownloadProgressReporter {
        progress_bar: progress_bar.clone(),
    };

    // Download the repodata.json
    let result = rattler_repodata_gateway::fetch::fetch_repo_data(
        channel.platform_url(platform),
        client,
        repodata_cache.to_path_buf(),
        FetchRepoDataOptions {
            ..Default::default()
        },
        Some(Arc::new(progress_reporter)),
    )
    .await;

    // Error out if an error occurred, but also update the progress bar
    let result = match result {
        Err(e) => {
            if matches!(e, FetchRepoDataError::NotFound(_)) && allow_not_found {
                progress_bar.set_style(fancy_log_handler.errored_progress_style());
                progress_bar.finish_with_message("Not Found");
                return Ok(None);
            }
            progress_bar.set_style(fancy_log_handler.errored_progress_style());
            progress_bar.finish_with_message("404 not found");
            return Err(e.into());
        }
        Ok(result) => result,
    };

    // Notify that we are deserializing
    progress_bar.set_style(fancy_log_handler.deserializing_progress_style());
    progress_bar.set_message("Deserializing..");

    // Deserialize the data. This is a hefty blocking operation so we spawn it as a tokio blocking
    // task.
    let repo_data_json_path = result.repo_data_json_path.clone();
    match tokio::task::spawn_blocking(move || {
        SparseRepoData::new(channel, platform.to_string(), repo_data_json_path, None)
    })
    .await
    {
        Ok(Ok(repodata)) => {
            let is_cache_hit = matches!(
                result.cache_result,
                CacheResult::CacheHit | CacheResult::CacheHitAfterFetch
            );
            progress_bar.set_style(fancy_log_handler.finished_progress_style());
            progress_bar.finish_with_message(if is_cache_hit { "Using cache" } else { "Done" });
            Ok(Some(repodata))
        }
        Ok(Err(err)) => {
            progress_bar.set_style(fancy_log_handler.errored_progress_style());
            progress_bar.finish_with_message(format!("Error: {:?}", err));
            Err(err.into())
        }
        Err(err) => match err.try_into_panic() {
            Ok(panic) => {
                std::panic::resume_unwind(panic);
            }
            Err(_) => {
                progress_bar.set_style(fancy_log_handler.errored_progress_style());
                progress_bar.finish_with_message("Canceled...");
                // Since the task was cancelled most likely the whole async stack is being cancelled.
                Err(anyhow::anyhow!("canceled"))
            }
        },
    }
}

/// Returns a friendly name for the specified channel.
fn friendly_channel_name(channel: &Channel) -> String {
    channel
        .name
        .as_ref()
        .map(String::from)
        .unwrap_or_else(|| channel.canonical_name())
}
