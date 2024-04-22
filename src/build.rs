//! The build module contains the code for running the build process for a given [`Output`]
use rattler_conda_types::{Channel, MatchSpec, ParseStrictness};
use std::path::PathBuf;
use std::vec;

use miette::IntoDiagnostic;
use rattler_index::index;
use rattler_solve::{ChannelPriority, SolveStrategy};

use crate::metadata::Output;
use crate::package_test::TestConfiguration;
use crate::recipe::parser::TestType;
use crate::render::solver::load_repodatas;
use crate::utils::remove_dir_all_force;
use crate::{package_test, tool_configuration};

/// Check if the build should be skipped because it already exists in any of the channels
pub async fn skip_existing(
    mut outputs: Vec<Output>,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<Vec<Output>> {
    let span = tracing::info_span!("Checking existing builds");
    let _enter = span.enter();

    let only_local = match tool_configuration.skip_existing {
        tool_configuration::SkipExisting::Local => true,
        tool_configuration::SkipExisting::All => false,
        tool_configuration::SkipExisting::None => return Ok(outputs),
    };

    // If we should skip existing builds, check if the build already exists
    let Some(first_output) = outputs.first() else {
        return Ok(outputs);
    };

    let all_channels = first_output.reindex_channels().into_diagnostic()?;

    let match_specs = outputs
        .iter()
        .map(|o| {
            MatchSpec::from_str(o.name().as_normalized(), ParseStrictness::Strict).into_diagnostic()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let channels = if only_local {
        vec![
            Channel::from_directory(&first_output.build_configuration.directories.output_dir)
                .base_url,
        ]
    } else {
        all_channels
    };

    let existing = load_repodatas(
        &channels,
        first_output.host_platform(),
        &match_specs,
        tool_configuration,
    )
    .await
    .map_err(|e| miette::miette!("Failed to load repodata: {e}."))?;

    let existing_set = existing
        .iter()
        .flatten()
        .map(|p| {
            format!(
                "{}-{}-{}",
                p.package_record.name.as_normalized(),
                p.package_record.version,
                p.package_record.build
            )
        })
        .collect::<std::collections::HashSet<_>>();


    // Retain only the outputs that do not exist yet
    outputs.retain(|output| {
        tracing::info!("Checking: {}-{}-{}", output.name().as_normalized(), output.version(), output.build_string().unwrap_or_default());
        tracing::info!("Existing: {:?}", existing_set);
        let exists = existing_set.contains(&format!(
            "{}-{}-{}",
            output.name().as_normalized(),
            output.version(),
            output.build_string().unwrap_or_default()
        ));
        if exists {
            // The identifier should always be set at this point
            tracing::info!(
                "Skipping build for {}",
                output.identifier().as_deref().unwrap_or("unknown")
            );
        }
        !exists
    });

    Ok(outputs)
}

/// Run the build for the given output. This will fetch the sources, resolve the dependencies,
/// and execute the build script. Returns the path to the resulting package.
pub async fn run_build(
    output: Output,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<(Output, PathBuf)> {
    if output.build_string().is_none() {
        miette::bail!("Build string is not set for {:?}", output.name());
    }

    output
        .build_configuration
        .directories
        .create_build_dir()
        .into_diagnostic()?;

    let span = tracing::info_span!("Running build for", recipe = output.identifier().unwrap());
    let _enter = span.enter();
    output.record_build_start();

    let directories = output.build_configuration.directories.clone();

    index(
        &directories.output_dir,
        Some(&output.build_configuration.target_platform.clone()),
    )
    .into_diagnostic()?;

    let output = output
        .fetch_sources(tool_configuration)
        .await
        .into_diagnostic()?;

    let output = output
        .resolve_dependencies(tool_configuration)
        .await
        .into_diagnostic()?;

    output.run_build_script().await.into_diagnostic()?;

    // Package all the new files
    let (result, paths_json) = output
        .create_package(tool_configuration)
        .await
        .into_diagnostic()?;

    output.record_artifact(&result, &paths_json);

    let span = tracing::info_span!("Running package tests");
    let enter = span.enter();

    // We run all the package content tests
    for test in output.recipe.tests() {
        // TODO we could also run each of the (potentially multiple) test scripts and collect the errors
        if let TestType::PackageContents(package_contents) = test {
            package_contents
                .run_test(&paths_json, &output.build_configuration.target_platform)
                .into_diagnostic()?;
        }
    }

    if !tool_configuration.no_clean {
        remove_dir_all_force(&directories.build_dir).into_diagnostic()?;
    }

    if tool_configuration.no_test {
        tracing::info!("Skipping tests");
    } else {
        package_test::run_test(
            &result,
            &TestConfiguration {
                test_prefix: directories.work_dir.join("test"),
                target_platform: Some(output.build_configuration.host_platform),
                keep_test_prefix: tool_configuration.no_clean,
                channels: output.reindex_channels().into_diagnostic()?,
                channel_priority: ChannelPriority::Strict,
                solve_strategy: SolveStrategy::Highest,
                tool_configuration: tool_configuration.clone(),
            },
        )
        .await
        .into_diagnostic()?;
    }

    drop(enter);

    if !tool_configuration.no_clean && directories.build_dir.exists() {
        remove_dir_all_force(&directories.build_dir).into_diagnostic()?;
    }

    Ok((output, result))
}
