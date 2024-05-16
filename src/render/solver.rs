use anyhow::Context;
use comfy_table::Table;
use futures::{stream, stream::FuturesUnordered, FutureExt, StreamExt, TryFutureExt, TryStreamExt};

use indicatif::{HumanBytes, ProgressBar};
use rattler::{
    install::{link_package, InstallDriver, InstallOptions, Transaction, TransactionOperation},
    package_cache::PackageCache,
};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, Platform, PrefixRecord,
    RepoDataRecord,
};
use rattler_repodata_gateway::{fetch::{
    CacheResult, FetchRepoDataError, FetchRepoDataOptions,
}, Reporter};
use rattler_repodata_gateway::sparse::SparseRepoData;
use rattler_solve::{resolvo::Solver, ChannelPriority, SolverImpl, SolverTask};
use reqwest_middleware::ClientWithMiddleware;
use url::Url;

use std::{
    future::ready, io::ErrorKind, path::{Path, PathBuf}, sync::Arc, time::Duration
};
use tokio::task::JoinHandle;

use crate::{console_utils::LoggingOutputHandler, tool_configuration};

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
    channels: &[String],
    tool_configuration: &tool_configuration::Configuration,
) -> anyhow::Result<Vec<RepoDataRecord>> {
    // Parse the specs from the command line. We do this explicitly instead of allow clap to deal
    // with this because we need to parse the `channel_config` when parsing matchspecs.

    tracing::info!("\nResolving environment for:\n");
    tracing::info!("  Platform: {}", target_platform);
    tracing::info!("  Channels: ");
    for channel in channels {
        tracing::info!("   - {}", channel);
    }
    tracing::info!("  Specs:");
    for spec in specs {
        tracing::info!("   - {}", spec);
    }

    let installed_packages = find_installed_packages(target_prefix, 100)
        .await
        .context("failed to determine currently installed packages")?;

    let (cache_dir, repodatas) =
        load_repodatas(channels, target_platform, tool_configuration, specs).await?;

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
        available_packages: &repodatas,
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
            &cache_dir,
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
    channels: &[String],
    target_platform: &Platform,
    tool_configuration: &tool_configuration::Configuration,
    specs: &[MatchSpec],
) -> Result<(PathBuf, Vec<Vec<RepoDataRecord>>), anyhow::Error> {
    let channel_config = ChannelConfig::default_with_root_dir(std::env::current_dir()?);
    let cache_dir = rattler::default_cache_dir()?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| anyhow::anyhow!("could not create cache directory: {}", e))?;

    let channels = channels
        .iter()
        .map(|channel_str| Channel::from_str(channel_str, &channel_config))
        .collect::<Result<Vec<_>, _>>()?;

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
        .map(move |(channel, platform)| {
            let repodata_cache = repodata_cache_path.clone();
            let download_client = repodata_download_client.clone();
            async move {
                fetch_repo_data_records_with_progress(
                    channel,
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
    cache_dir: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> anyhow::Result<()> {
    let installed_packages = vec![];
    // Construct a transaction to
    let transaction = Transaction::from_current_and_desired(
        installed_packages,
        required_packages.clone(),
        *target_platform,
    )?;

    print_as_table(required_packages);

    if !transaction.operations.is_empty() {
        // Execute the operations that are returned by the solver.
        execute_transaction(
            transaction,
            target_prefix,
            cache_dir,
            tool_configuration.client.clone(),
            tool_configuration.fancy_log_handler.clone(),
        )
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

/// Executes the transaction on the given environment.
async fn execute_transaction(
    transaction: Transaction<PrefixRecord, RepoDataRecord>,
    target_prefix: &Path,
    cache_dir: &Path,
    download_client: ClientWithMiddleware,
    fancy_log_handler: LoggingOutputHandler,
) -> anyhow::Result<()> {
    // Open the package cache
    let package_cache = PackageCache::new(cache_dir.join("pkgs"));

    // Create an install driver which helps limit the number of concurrent filesystem operations
    let install_driver = InstallDriver::default();

    // Define default installation options.
    let install_options = InstallOptions {
        python_info: transaction.python_info.clone(),
        platform: Some(transaction.platform),
        ..Default::default()
    };

    // Create a progress bars for downloads.
    let total_packages_to_download = transaction
        .operations
        .iter()
        .filter(|op| op.record_to_install().is_some())
        .count();
    let download_pb = if total_packages_to_download > 0 {
        let pb = fancy_log_handler.add_progress_bar(
            indicatif::ProgressBar::new(total_packages_to_download as u64)
                .with_style(fancy_log_handler.default_progress_style())
                .with_finish(indicatif::ProgressFinish::WithMessage("Done!".into()))
                .with_prefix("downloading"),
        );
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    // Create a progress bar to track all operations.
    let total_operations = transaction.operations.len();
    let link_pb = fancy_log_handler.add_progress_bar(
        indicatif::ProgressBar::new(total_operations as u64)
            .with_style(fancy_log_handler.default_progress_style())
            .with_finish(indicatif::ProgressFinish::WithMessage("Done!".into()))
            .with_prefix("linking"),
    );
    link_pb.enable_steady_tick(Duration::from_millis(100));

    // Perform all transactions operations in parallel.
    stream::iter(transaction.operations.clone())
        .map(Ok)
        .try_for_each_concurrent(50, |op| {
            let download_client = download_client.clone();
            let package_cache = &package_cache;
            let install_driver = &install_driver;
            let download_pb = download_pb.as_ref();
            let link_pb = &link_pb;
            let install_options = &install_options;
            let fancy_log_handler = fancy_log_handler.clone();
            async move {
                execute_operation(
                    target_prefix,
                    download_client,
                    package_cache,
                    install_driver,
                    download_pb,
                    link_pb,
                    op,
                    install_options,
                    fancy_log_handler,
                )
                .await
            }
        })
        .await?;

    install_driver.post_process(&transaction, target_prefix)?;

    Ok(())
}

/// Executes a single operation of a transaction on the environment.
/// TODO: Move this into an object or something.
#[allow(clippy::too_many_arguments)]
async fn execute_operation(
    target_prefix: &Path,
    download_client: ClientWithMiddleware,
    package_cache: &PackageCache,
    install_driver: &InstallDriver,
    download_pb: Option<&ProgressBar>,
    link_pb: &ProgressBar,
    op: TransactionOperation<PrefixRecord, RepoDataRecord>,
    install_options: &InstallOptions,
    fancy_log_handler: LoggingOutputHandler,
) -> anyhow::Result<()> {
    // Determine the package to install
    let install_record = op.record_to_install();
    let remove_record = op.record_to_remove();

    // Create a future to remove the existing package
    let remove_future = if let Some(remove_record) = remove_record {
        remove_package_from_environment(target_prefix, remove_record).left_future()
    } else {
        ready(Ok(())).right_future()
    };

    // Create a future to download the package
    let cached_package_dir_fut = if let Some(install_record) = install_record {
        async {
            // Make sure the package is available in the package cache.
            let result = package_cache
                .get_or_fetch_from_url(
                    &install_record.package_record,
                    install_record.url.clone(),
                    download_client.clone(),
                )
                .map_ok(|cache_dir| Some((install_record.clone(), cache_dir)))
                .map_err(anyhow::Error::from)
                .await;

            // Increment the download progress bar.
            if let Some(pb) = download_pb {
                pb.inc(1);
                if pb.length() == Some(pb.position()) {
                    pb.set_style(fancy_log_handler.finished_progress_style());
                }
            }

            result
        }
        .left_future()
    } else {
        ready(Ok(None)).right_future()
    };

    // Await removal and downloading concurrently
    let (_, install_package) = tokio::try_join!(remove_future, cached_package_dir_fut)?;

    // If there is a package to install, do that now.
    if let Some((record, package_dir)) = install_package {
        install_package_to_environment(
            target_prefix,
            package_dir,
            record.clone(),
            install_driver,
            install_options,
        )
        .await?;
    }

    // Increment the link progress bar since we finished a step!
    link_pb.inc(1);
    if link_pb.length() == Some(link_pb.position()) {
        link_pb.set_style(fancy_log_handler.finished_progress_style());
    }

    Ok(())
}

/// Install a package into the environment and write a `conda-meta` file that contains information
/// about how the file was linked.
async fn install_package_to_environment(
    target_prefix: &Path,
    package_dir: PathBuf,
    repodata_record: RepoDataRecord,
    install_driver: &InstallDriver,
    install_options: &InstallOptions,
) -> anyhow::Result<()> {
    // Link the contents of the package into our environment. This returns all the paths that were
    // linked.
    let paths = link_package(
        &package_dir,
        target_prefix,
        install_driver,
        install_options.clone(),
    )
    .await?;

    // Construct a PrefixRecord for the package
    let prefix_record = PrefixRecord {
        repodata_record,
        package_tarball_full_path: None,
        extracted_package_dir: Some(package_dir),
        files: paths
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect(),
        paths_data: paths.into(),
        // TODO: Retrieve the requested spec for this package from the request
        requested_spec: None,
        // TODO: What to do with this?
        link: None,
    };

    // Create the conda-meta directory if it doesn't exist yet.
    let target_prefix = target_prefix.to_path_buf();
    match tokio::task::spawn_blocking(move || {
        let conda_meta_path = target_prefix.join("conda-meta");
        std::fs::create_dir_all(&conda_meta_path)?;

        // Write the conda-meta information
        let pkg_meta_path = conda_meta_path.join(format!(
            "{}-{}-{}.json",
            prefix_record
                .repodata_record
                .package_record
                .name
                .as_normalized(),
            prefix_record.repodata_record.package_record.version,
            prefix_record.repodata_record.package_record.build
        ));
        prefix_record.write_to_path(pkg_meta_path, true)
    })
    .await
    {
        Ok(result) => Ok(result?),
        Err(err) => {
            if let Ok(panic) = err.try_into_panic() {
                std::panic::resume_unwind(panic);
            }
            // The operation has been cancelled, so we can also just ignore everything.
            Ok(())
        }
    }
}

/// Completely remove the specified package from the environment.
async fn remove_package_from_environment(
    target_prefix: &Path,
    package: &PrefixRecord,
) -> anyhow::Result<()> {
    // TODO: Take into account any clobbered files, they need to be restored.
    // TODO: Can we also delete empty directories?

    // Remove all entries
    for paths in package.paths_data.paths.iter() {
        match tokio::fs::remove_file(target_prefix.join(&paths.relative_path)).await {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // Simply ignore if the file is already gone.
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("failed to delete {}", paths.relative_path.display())
                });
            }
        }
    }

    // Remove the conda-meta file
    let conda_meta_path = target_prefix.join("conda-meta").join(format!(
        "{}-{}-{}.json",
        package.repodata_record.package_record.name.as_normalized(),
        package.repodata_record.package_record.version,
        package.repodata_record.package_record.build
    ));
    tokio::fs::remove_file(conda_meta_path).await?;

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

/// Scans the conda-meta directory of an environment and returns all the [`PrefixRecord`]s found in
/// there.
async fn find_installed_packages(
    target_prefix: &Path,
    concurrency_limit: usize,
) -> Result<Vec<PrefixRecord>, std::io::Error> {
    let mut meta_futures =
        FuturesUnordered::<JoinHandle<Result<PrefixRecord, std::io::Error>>>::new();
    let mut result = Vec::new();
    for entry in std::fs::read_dir(target_prefix.join("conda-meta"))
        .into_iter()
        .flatten()
    {
        let entry = entry?;
        let path = entry.path();
        if path.ends_with(".json") {
            continue;
        }

        // If there are too many pending entries, wait for one to be finished
        if meta_futures.len() >= concurrency_limit {
            match meta_futures
                .next()
                .await
                .expect("we know there are pending futures")
            {
                Ok(record) => result.push(record?),
                Err(e) => {
                    if let Ok(panic) = e.try_into_panic() {
                        std::panic::resume_unwind(panic);
                    }
                    // The future was cancelled, we can simply return what we have.
                    return Ok(result);
                }
            }
        }

        // Spawn loading on another thread
        let future = tokio::task::spawn_blocking(move || PrefixRecord::from_path(path));
        meta_futures.push(future);
    }

    while let Some(record) = meta_futures.next().await {
        match record {
            Ok(record) => result.push(record?),
            Err(e) => {
                if let Ok(panic) = e.try_into_panic() {
                    std::panic::resume_unwind(panic);
                }
                // The future was cancelled, we can simply return what we have.
                return Ok(result);
            }
        }
    }

    Ok(result)
}
