//! The build module contains the code for running the build process for a given [`Output`]
use fs_err as fs;
use rattler_conda_types::{MatchSpec, ParseStrictness};
use std::path::PathBuf;
use std::vec;

use miette::IntoDiagnostic;
use rattler_index::index;

use crate::metadata::Output;
use crate::package_test::TestConfiguration;
use crate::recipe::parser::TestType;
use crate::render::solver::load_repodatas;
use crate::{package_test, tool_configuration};

/// Check if the build should be skipped because it already exists in any of the channels
pub async fn skip_existing(
    output: &Output,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<bool> {
    // If we should skip existing builds, check if the build already exists
    if tool_configuration.skip_existing {
        let channels = output.reindex_channels().into_diagnostic()?;
        let match_spec =
            MatchSpec::from_str(output.name().as_normalized(), ParseStrictness::Strict)
                .into_diagnostic()?;
        let match_spec_vec = vec![match_spec.clone()];
        let (_, existing) = load_repodatas(
            &channels,
            output.target_platform(),
            tool_configuration,
            &match_spec_vec,
        )
        .await
        .unwrap();

        return Ok(existing.iter().flatten().any(|package| {
            package.package_record.version.to_string() == output.version()
                && output.build_string() == Some(&package.package_record.build)
        }));
    }
    Ok(false)
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
        fs::remove_dir_all(&directories.build_dir).into_diagnostic()?;
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
                tool_configuration: tool_configuration.clone(),
            },
        )
        .await
        .into_diagnostic()?;
    }

    drop(enter);

    if !tool_configuration.no_clean && directories.build_dir.exists() {
        fs::remove_dir_all(&directories.build_dir).into_diagnostic()?;
    }

    Ok((output, result))
}
