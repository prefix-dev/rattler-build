use comfy_table::Table;

use indicatif::ProgressStyle;
use indicatif::{HumanBytes, ProgressBar};
use rattler::install::{DefaultProgressFormatter, IndicatifReporter, Installer};
use rattler_conda_types::{
    Channel, GenericVirtualPackage, MatchSpec, Platform, PrefixRecord, RepoDataRecord,
};
use rattler_repodata_gateway::Gateway;
use rattler_solve::{resolvo::Solver, SolveStrategy, SolverImpl, SolverTask};
use url::Url;

use crate::tool_configuration;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

    let repo_data = load_repodatas(channels, target_platform, specs, tool_configuration).await?;

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

struct GatewayReporter {
    progress_bars: Arc<Mutex<Vec<ProgressBar>>>,
    multi_progress: indicatif::MultiProgress,
    progress_template: Option<ProgressStyle>,
    finish_template: Option<ProgressStyle>,
}

#[derive(Default)]
struct GatewayReporterBuilder {
    multi_progress: Option<indicatif::MultiProgress>,
    progress_template: Option<ProgressStyle>,
    finish_template: Option<ProgressStyle>,
}

impl GatewayReporter {
    pub fn builder() -> GatewayReporterBuilder {
        GatewayReporterBuilder::default()
    }
}

impl rattler_repodata_gateway::Reporter for GatewayReporter {
    fn on_download_start(&self, _url: &Url) -> usize {
        let progress_bar = self
            .multi_progress
            .add(ProgressBar::new(1))
            .with_finish(indicatif::ProgressFinish::AndLeave);

        progress_bar.enable_steady_tick(Duration::from_millis(100));

        // use the configured style
        if let Some(template) = &self.progress_template {
            progress_bar.set_style(template.clone());
        }
        let mut progress_bars = self.progress_bars.lock().unwrap();
        progress_bars.push(progress_bar);
        progress_bars.len() - 1
    }

    fn on_download_complete(&self, url: &Url, index: usize) {
        // Remove the progress bar from the multi progress
        let pb = &self.progress_bars.lock().unwrap()[index];
        if let Some(template) = &self.finish_template {
            pb.set_style(template.clone());
            pb.finish_with_message(format!("Done: {}", url));
        } else {
            pb.finish();
        }
    }

    fn on_download_progress(&self, _url: &Url, index: usize, bytes: usize, total: Option<usize>) {
        let progress_bar = &self.progress_bars.lock().unwrap()[index];
        progress_bar.set_length(total.unwrap_or(bytes) as u64);
        progress_bar.set_position(bytes as u64);
    }
}

impl GatewayReporterBuilder {
    #[must_use]
    pub fn with_multi_progress(
        mut self,
        multi_progress: indicatif::MultiProgress,
    ) -> GatewayReporterBuilder {
        self.multi_progress = Some(multi_progress);
        self
    }

    #[must_use]
    pub fn with_progress_template(mut self, template: ProgressStyle) -> GatewayReporterBuilder {
        self.progress_template = Some(template);
        self
    }

    #[must_use]
    pub fn with_finish_template(mut self, template: ProgressStyle) -> GatewayReporterBuilder {
        self.finish_template = Some(template);
        self
    }

    pub fn finish(self) -> GatewayReporter {
        GatewayReporter {
            progress_bars: Arc::new(Mutex::new(Vec::new())),
            multi_progress: self.multi_progress.expect("multi progress is required"),
            progress_template: self.progress_template,
            finish_template: self.finish_template,
        }
    }
}

/// Load repodata from channels. Only includes necessary records for platform & specs.
pub async fn load_repodatas(
    channels: &[Url],
    target_platform: &Platform,
    specs: &[MatchSpec],
    tool_configuration: &tool_configuration::Configuration,
) -> anyhow::Result<Vec<rattler_repodata_gateway::RepoData>> {
    let cache_dir = rattler::default_cache_dir()?;
    let download_client = tool_configuration.client.clone();

    // Get the package names from the matchspecs so we can only load the package records that we need.
    let gateway = Gateway::builder()
        .with_cache_dir(cache_dir.join("repodata"))
        .with_client(download_client.clone())
        .finish();

    // Determine the channels to use from the command line or select the default. Like matchspecs
    // this also requires the use of the `channel_config` so we have to do this manually.
    let channel_config = ChannelConfig::default_with_root_dir(std::env::current_dir()?);
    let channels = channels
        .iter()
        .map(|channel_str| Channel::from_str(channel_str, &channel_config))
        .collect::<Result<Vec<_>, _>>()?;

    let pb =
        ProgressBar::new(50).with_style(tool_configuration.fancy_log_handler.default_bytes_style());

    let test_pb = tool_configuration
        .fancy_log_handler
        .multi_progress()
        .add(pb);
    test_pb.finish();

    let result = gateway
        .query(
            channels,
            [*target_platform, Platform::NoArch],
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
        .await?;

    tool_configuration
        .fancy_log_handler
        .multi_progress()
        .clear()?;

    Ok(result)
}

pub async fn install_packages(
    required_packages: &Vec<RepoDataRecord>,
    target_platform: &Platform,
    target_prefix: &Path,
    tool_configuration: &tool_configuration::Configuration,
) -> anyhow::Result<()> {
    let installed_packages = vec![];

    print_as_table(required_packages);

    if !required_packages.is_empty() {
        Installer::new()
            .with_download_client(tool_configuration.client.clone())
            .with_target_platform(*target_platform)
            .with_installed_packages(installed_packages)
            .with_execute_link_scripts(true)
            .with_reporter(
                IndicatifReporter::builder()
                    .with_multi_progress(
                        tool_configuration
                            .fancy_log_handler
                            .multi_progress()
                            .clone(),
                    )
                    .with_formatter(
                        DefaultProgressFormatter::default().with_prefix(
                            tool_configuration.fancy_log_handler.with_indent_levels(""),
                        ),
                    )
                    .finish(),
            )
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
