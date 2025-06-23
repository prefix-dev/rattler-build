//! The build module contains the code for running the build process for a given
//! [`Output`]
use std::{path::PathBuf, vec};

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{Channel, MatchSpec, Platform, package::PathsJson};

use crate::{
    apply_patch_custom,
    metadata::{Output, build_reindexed_channels},
    recipe::parser::TestType,
    render::solver::load_repodatas,
    script::InterpreterError,
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
            .await
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
            .fetch_sources(tool_configuration, apply_patch_custom)
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

    match output.run_build_script().await {
        Ok(_) => {}
        Err(InterpreterError::Debug(info)) => {
            tracing::info!("{}", info);
            return Err(miette::miette!(
                "Script not executed because debug mode is enabled"
            ));
        }
        Err(InterpreterError::ExecutionFailed(_)) => {
            return Err(miette::miette!("Script failed to execute"));
        }
    }

    // Package all the new files
    let (result, paths_json) = output
        .create_package(tool_configuration)
        .await
        .into_diagnostic()?;

    // Check for binary prefix if configured
    if tool_configuration.error_prefix_in_binary {
        tracing::info!("Checking for embedded prefix in binary files...");
        check_for_binary_prefix(&output, &paths_json)?;
    }

    // Check for symlinks on Windows if not allowed
    if (output.build_configuration.target_platform.is_windows()
        || output.build_configuration.target_platform == Platform::NoArch)
        && !tool_configuration.allow_symlinks_on_windows
    {
        tracing::info!("Checking for symlinks ...");
        check_for_symlinks_on_windows(&output, &paths_json)?;
    }

    output.record_artifact(&result, &paths_json);

    let span = tracing::info_span!("Running package tests");
    let enter = span.enter();

    // We run all the package content tests
    for test in output.recipe.tests() {
        if let TestType::PackageContents { package_contents } = test {
            package_contents
                .run_test(&paths_json, &output)
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

/// Check if any binary files contain the host prefix
fn check_for_binary_prefix(output: &Output, paths_json: &PathsJson) -> Result<(), miette::Error> {
    use rattler_conda_types::package::FileMode;

    for paths_entry in &paths_json.paths {
        if let Some(prefix_placeholder) = &paths_entry.prefix_placeholder {
            if prefix_placeholder.file_mode == FileMode::Binary {
                return Err(miette::miette!(
                    "Package {} contains Binary file {} which contains host prefix placeholder, which may cause issues when the package is installed to a different location. \
                    Consider fixing the build process to avoid embedding the host prefix in binaries. \
                    To allow this, remove the --error-prefix-in-binary flag.",
                    output.name().as_normalized(),
                    paths_entry.relative_path.display()
                ));
            }
        }
    }

    Ok(())
}

/// Check if any files are symlinks on Windows
fn check_for_symlinks_on_windows(
    output: &Output,
    paths_json: &PathsJson,
) -> Result<(), miette::Error> {
    use rattler_conda_types::package::PathType;

    let mut symlinks = Vec::new();

    for paths_entry in &paths_json.paths {
        if paths_entry.path_type == PathType::SoftLink {
            symlinks.push(paths_entry.relative_path.display().to_string());
        }
    }

    if !symlinks.is_empty() {
        return Err(miette::miette!(
            "Package {} contains symlinks which are not supported on most Windows systems:\n  - {}\n\
            To allow symlinks, use the --allow-symlinks-on-windows flag.",
            output.name().as_normalized(),
            symlinks.join("\n  - ")
        ));
    }

    Ok(())
}
