//! The build module contains the code for running the build process for a given [`Output`]
use fs_err as fs;
use std::path::PathBuf;

use miette::IntoDiagnostic;
use rattler_index::index;

use crate::metadata::Output;
use crate::package_test::TestConfiguration;
use crate::packaging::{package_conda, Files};
use crate::recipe::parser::TestType;
use crate::render::resolved_dependencies::{install_environments, resolve_dependencies};
use crate::source::fetch_sources;
use crate::{package_test, tool_configuration};

/// Run the build for the given output. This will fetch the sources, resolve the dependencies,
/// and execute the build script. Returns the path to the resulting package.
pub async fn run_build(
    output: &Output,
    tool_configuration: tool_configuration::Configuration,
) -> miette::Result<PathBuf> {
    let directories = &output.build_configuration.directories;

    index(
        &directories.output_dir,
        Some(&output.build_configuration.target_platform),
    )
    .into_diagnostic()?;

    // Add the local channel to the list of channels
    let mut channels = vec![directories.output_dir.to_string_lossy().to_string()];
    channels.extend(output.build_configuration.channels.clone());

    let output = if let Some(finalized_sources) = &output.finalized_sources {
        fetch_sources(
            finalized_sources,
            directories,
            &output.system_tools,
            &tool_configuration,
        )
        .await
        .into_diagnostic()?;

        output.clone()
    } else {
        let rendered_sources = fetch_sources(
            output.recipe.sources(),
            directories,
            &output.system_tools,
            &tool_configuration,
        )
        .await
        .into_diagnostic()?;

        Output {
            finalized_sources: Some(rendered_sources),
            ..output.clone()
        }
    };

    let output = if output.finalized_dependencies.is_some() {
        tracing::info!("Using finalized dependencies");

        // The output already has the finalized dependencies, so we can just use it as-is
        install_environments(&output, tool_configuration.clone())
            .await
            .into_diagnostic()?;
        output.clone()
    } else {
        let finalized_dependencies =
            resolve_dependencies(&output, &channels, tool_configuration.clone())
                .await
                .into_diagnostic()?;

        // The output with the resolved dependencies
        Output {
            finalized_dependencies: Some(finalized_dependencies),
            ..output.clone()
        }
    };

    output.run_build_script().await.into_diagnostic()?;

    let files_after = Files::from_prefix(
        &directories.host_prefix,
        output.recipe.build().always_include_files(),
    )
    .into_diagnostic()?;

    let (result, paths_json) =
        package_conda(&output, &tool_configuration, &files_after).into_diagnostic()?;

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

    index(
        &directories.output_dir,
        Some(&output.build_configuration.target_platform),
    )
    .into_diagnostic()?;

    let test_dir = directories.work_dir.join("test");
    fs::create_dir_all(&test_dir).into_diagnostic()?;

    tracing::info!("{}", output);

    if tool_configuration.no_test {
        tracing::info!("Skipping tests");
    } else {
        tracing::info!("Running tests");

        package_test::run_test(
            &result,
            &TestConfiguration {
                test_prefix: test_dir.clone(),
                target_platform: Some(output.build_configuration.host_platform),
                keep_test_prefix: tool_configuration.no_clean,
                channels,
                tool_configuration: tool_configuration.clone(),
            },
        )
        .await
        .into_diagnostic()?;
    }

    if !tool_configuration.no_clean {
        fs::remove_dir_all(&directories.build_dir).into_diagnostic()?;
    }

    Ok(result)
}
