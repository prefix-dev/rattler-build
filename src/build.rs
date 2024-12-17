//! The build module contains the code for running the build process for a given
//! [`Output`]
use std::{path::PathBuf, vec};

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{Channel, MatchSpec};

use crate::{
    metadata::{build_reindexed_channels, Output},
    recipe::parser::TestType,
    render::solver::load_repodatas,
    tool_configuration,
};

/// Check if the build should be skipped because it already exists in any of the
/// channels
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

    let all_channels =
        build_reindexed_channels(&first_output.build_configuration, tool_configuration)
            .into_diagnostic()
            .context("failed to reindex output channel")?;

    let match_specs = outputs
        .iter()
        .map(|o| o.name().clone().into())
        .collect::<Vec<MatchSpec>>();

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
        first_output.host_platform().platform,
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
        let exists = existing_set.contains(&format!(
            "{}-{}-{}",
            output.name().as_normalized(),
            output.version(),
            &output.build_string()
        ));
        if exists {
            // The identifier should always be set at this point
            tracing::info!("Skipping build for {}", output.identifier());
        }
        !exists
    });

    Ok(outputs)
}

/// Run the build for the given output. This will fetch the sources, resolve the
/// dependencies, and execute the build script. Returns the path to the
/// resulting package.
pub async fn run_build(
    output: Output,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<(Output, PathBuf)> {
    output
        .build_configuration
        .directories
        .create_build_dir(true)
        .into_diagnostic()?;

    let span = tracing::info_span!("Running build for", recipe = output.identifier());
    let _enter = span.enter();
    output.record_build_start();

    let directories = output.build_configuration.directories.clone();

    let output = if output.recipe.cache.is_some() {
        output.build_or_fetch_cache(tool_configuration).await?
    } else {
        output
            .fetch_sources(tool_configuration)
            .await
            .into_diagnostic()?
    };

    let output = output
        .resolve_dependencies(tool_configuration)
        .await
        .into_diagnostic()?;

    output
        .install_environments(tool_configuration)
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
        if let TestType::PackageContents { package_contents } = test {
            package_contents
                .run_test(&paths_json, &output.build_configuration.target_platform)
                .into_diagnostic()?;
        }
    }

    if !tool_configuration.no_clean {
        directories.clean().into_diagnostic()?;
    }

    drop(enter);

    if !tool_configuration.no_clean {
        directories.clean().into_diagnostic()?;
    }

    Ok((output, result))
}
