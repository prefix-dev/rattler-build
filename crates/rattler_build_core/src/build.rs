//! The build module contains the code for running the build process for a given
//! [`Output`]
use std::{collections::HashSet, path::PathBuf, vec};

use miette::{Context, IntoDiagnostic};
use rattler_build_recipe::stage1::TestType;
use rattler_build_script::InterpreterError;
use rattler_conda_types::{
    Channel, ChannelUrl, MatchSpec, PackageName, PackageRecord, Subdir, package::PathsJson,
};
use rattler_repodata_gateway::{Gateway, GatewayError};
use tracing::Instrument;

use crate::{
    metadata::{Output, build_reindexed_channels},
    package_test::PackageContentsTestExt as _,
    packaging::record_files,
    render::{resolved_dependencies::RunExportsDownload, solver::load_repodatas},
    tool_configuration,
};

/// Behavior for handling the working directory during the build process
#[derive(Debug, Clone, Copy)]
pub enum WorkingDirectoryBehavior {
    /// Preserve the working directory (don't clean up)
    Preserve,
    /// Clean up the working directory after build
    Cleanup,
}

/// The name, version and build string of a package.
///
/// Two packages with the same identity are considered the same build by
/// `--skip-existing`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageIdentity {
    /// The package name
    pub name: PackageName,
    /// The package version, as written in the package record
    pub version: String,
    /// The build string
    pub build_string: String,
}

impl PackageIdentity {
    /// Create a new identity from a name, version and build string.
    pub fn new(name: PackageName, version: impl ToString, build_string: impl Into<String>) -> Self {
        Self {
            name,
            version: version.to_string(),
            build_string: build_string.into(),
        }
    }
}

impl From<&PackageRecord> for PackageIdentity {
    fn from(record: &PackageRecord) -> Self {
        Self::new(record.name.clone(), &record.version, record.build.clone())
    }
}

impl From<&Output> for PackageIdentity {
    fn from(output: &Output) -> Self {
        Self::new(
            output.name().clone(),
            output.version(),
            output.build_string().into_owned(),
        )
    }
}

/// Return the `packages` that have a record with the same name, version and
/// build string in `records` (e.g. the records of a gateway query, filtered by
/// the caller).
pub fn existing_packages<'a>(
    records: impl IntoIterator<Item = &'a PackageRecord>,
    packages: &[PackageIdentity],
) -> HashSet<PackageIdentity> {
    let wanted = packages.iter().collect::<HashSet<_>>();
    records
        .into_iter()
        .map(PackageIdentity::from)
        .filter(|identity| wanted.contains(identity))
        .collect()
}

/// Return the `packages` that already exist in `channels` for `platform` (or
/// `noarch`).
///
/// Only the records of the given package names are fetched, and nothing is
/// rendered, solved or reindexed, so this can be used to check a large number
/// of outputs against a channel without constructing [`Output`]s.
pub async fn find_existing_packages(
    gateway: &Gateway,
    channels: &[ChannelUrl],
    platform: Subdir,
    packages: &[PackageIdentity],
) -> Result<HashSet<PackageIdentity>, GatewayError> {
    if packages.is_empty() {
        return Ok(HashSet::new());
    }
    let channels = channels
        .iter()
        .map(|url| Channel::from_url(url.clone()))
        .collect::<Vec<_>>();
    let specs = packages
        .iter()
        .map(|p| MatchSpec::from(p.name.clone()))
        .collect::<Vec<_>>();
    let result = gateway
        .query(channels, [platform, Subdir::NoArch], specs)
        .await?;
    for warning in &result.warnings {
        tracing::warn!("{warning}");
    }
    Ok(existing_packages(
        result
            .repodata
            .iter()
            .flatten()
            .map(|record| &record.package_record),
        packages,
    ))
}

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
        // Nothing has been built yet (`--render-only` does not create the
        // output directory)
        let output_dir = &first_output.build_configuration.directories.output_dir;
        if !output_dir.exists() {
            return Ok(outputs);
        }
        vec![
            Channel::try_from_directory(output_dir)
                .expect("could not create channel from directory")
                .base_url,
        ]
    } else {
        all_channels
    };

    // Uses `load_repodatas` rather than `find_existing_packages` to keep the
    // progress bars and channel notices of a regular build.
    let existing = load_repodatas(
        &channels,
        first_output.host_platform().platform,
        &match_specs,
        tool_configuration,
    )
    .await
    .map_err(|e| miette::miette!("Failed to load repodata: {e}."))?;

    let identities = outputs
        .iter()
        .map(PackageIdentity::from)
        .collect::<Vec<_>>();
    let existing_set = existing_packages(
        existing
            .iter()
            .flatten()
            .map(|record| &record.package_record),
        &identities,
    );

    // Retain only the outputs that do not exist yet
    let mut identities = identities.into_iter();
    outputs.retain(|output| {
        let identity = identities.next().expect("one identity per output");
        let exists = existing_set.contains(&identity);
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
    working_directory_behavior: WorkingDirectoryBehavior,
) -> miette::Result<(Output, PathBuf)> {
    let cleanup = matches!(
        working_directory_behavior,
        WorkingDirectoryBehavior::Cleanup
    );
    output
        .build_configuration
        .directories
        .create_build_dir(cleanup)
        .into_diagnostic()?;

    let span = tracing::info_span!(
        "Running build for",
        recipe = output.identifier(),
        span_color = output.identifier()
    );
    run_build_in_span(output, tool_configuration)
        .instrument(span)
        .await
}

async fn run_build_in_span(
    mut output: Output,
    tool_configuration: &tool_configuration::Configuration,
) -> miette::Result<(Output, PathBuf)> {
    output.record_build_start();

    let directories = output.build_configuration.directories.clone();

    // Process staging caches if this output depends on any
    // This will build or restore staging caches and return their dependencies/sources if inherited
    let staging_result = output.process_staging_caches(tool_configuration).await?;

    // If we inherit from a staging cache, store its dependencies, sources, and
    // library name map for overlinking checks
    if let Some((deps, sources, library_name_map)) = staging_result {
        output.finalized_cache_dependencies = Some(deps);
        output.finalized_cache_sources = Some(sources);
        output.staging_library_name_map = Some(library_name_map);
    }

    // Fetch sources for this output
    let output = output
        .fetch_sources(tool_configuration, crate::source::patch::apply_patch_custom)
        .await
        .into_diagnostic()?;

    // Resolve dependencies for this output
    // If we inherited from a staging cache, finalized_cache_dependencies will be merged
    // into the final dependencies during the resolve_dependencies call
    let output = output
        .resolve_dependencies(tool_configuration, RunExportsDownload::DownloadMissing)
        .await
        .into_diagnostic()?;

    // Snapshot the host prefix before dependency installation so we can
    // detect files added by post-link scripts (which aren't recorded in
    // conda-meta and would otherwise leak into the downstream package).
    let pre_install_files = record_files(&output.build_configuration.directories.host_prefix).ok();

    output
        .install_environments(tool_configuration)
        .await
        .into_diagnostic()?;

    // Compute the set of files added during install_environments. This
    // includes both properly-recorded dependency files *and* untracked
    // post-link artifacts. Passing this delta to create_package ensures
    // the untracked artifacts are excluded without accidentally hiding
    // files restored from a staging cache (which existed before install).
    let install_added_files = record_files(&output.build_configuration.directories.host_prefix)
        .ok()
        .map(|post| {
            if let Some(pre) = &pre_install_files {
                post.difference(pre).cloned().collect()
            } else {
                post
            }
        });

    let interpreter_field = if output.recipe.build().plan.steps().is_some() {
        "build.steps[].interpreter"
    } else {
        "build.script.interpreter"
    };

    match output.run_build_script(tool_configuration).await {
        Ok(_) => {}
        Err(InterpreterError::ExecutionFailed(err))
            if err.kind() == std::io::ErrorKind::InvalidInput =>
        {
            return Err(miette::miette!("{err}"));
        }
        Err(InterpreterError::ExecutionFailed(_)) => {
            return Err(miette::miette!("Script failed to execute"));
        }
        Err(InterpreterError::InterpreterNotFound(interpreter)) => {
            return Err(miette::miette!(
                help = format!(
                    "Add `{interpreter}` to the `requirements/build` section of your recipe so it is provided by the build environment."
                ),
                "interpreter `{interpreter}` was not found in the build environment"
            ));
        }
        Err(InterpreterError::InvalidInterpreter {
            interpreter,
            reason,
        }) => {
            return Err(miette::miette!(
                "interpreter `{interpreter}` was found but is not valid: {reason}"
            ));
        }
        Err(InterpreterError::UnsupportedInterpreter(interpreter)) => {
            return Err(
                match rattler_build_script::closest_interpreter(&interpreter) {
                    Some(suggestion) => miette::miette!(
                        help = format!("Did you mean `{suggestion}`?"),
                        "unsupported interpreter `{interpreter}` in `{interpreter_field}`"
                    ),
                    None => miette::miette!(
                        "unsupported interpreter `{interpreter}` in `{interpreter_field}`"
                    ),
                },
            );
        }
    }

    // Package all the new files. The archive is compressed into a temporary
    // file and only the final rename into the output channel takes its lock.
    let (result, paths_json) = output
        .create_package(tool_configuration, install_added_files.as_ref())
        .await
        .into_diagnostic()?;

    // Check for binary prefix if configured
    if tool_configuration.error_prefix_in_binary {
        tracing::info!("Checking for embedded prefix in binary files...");
        check_for_binary_prefix(&output, &paths_json)?;
    }

    // Check for symlinks on Windows if not allowed
    // Skip the check for noarch packages that have __unix in run dependencies,
    // since they will never be installed on Windows.
    if (output.build_configuration.target_platform.is_windows()
        || (output.build_configuration.target_platform == Subdir::NoArch
            && !has_unix_virtual_package(&output)))
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
        if let Some(prefix_placeholder) = &paths_entry.prefix_placeholder
            && prefix_placeholder.file_mode == FileMode::Binary
        {
            return Err(miette::miette!(
                "Package {} contains Binary file {} which contains host prefix placeholder, which may cause issues when the package is installed to a different location. \
                    Consider fixing the build process to avoid embedding the host prefix in binaries. \
                    To allow this, remove the --error-prefix-in-binary flag.",
                output.name().as_normalized(),
                paths_entry.relative_path.display()
            ));
        }
    }

    Ok(())
}

/// Check if the output has a Unix-specific virtual package (`__unix`, `__osx`,
/// `__linux`, `__glibc`, `__ios`, or `__android`) in its finalized run
/// dependencies, indicating this package is only intended for Unix systems.
fn has_unix_virtual_package(output: &Output) -> bool {
    output.finalized_dependencies.as_ref().is_some_and(|deps| {
        deps.run.depends.iter().any(|dep| {
            dep.spec().name.as_exact().is_some_and(|name| {
                [
                    "__unix",
                    "__osx",
                    "__linux",
                    "__glibc",
                    "__ios",
                    "__android",
                ]
                .contains(&name.as_normalized())
            })
        })
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_repodata(channel: &std::path::Path, subdir: &str, packages: &[(&str, &str, &str)]) {
        let packages = packages
            .iter()
            .map(|(name, version, build)| {
                (
                    format!("{name}-{version}-{build}.conda"),
                    serde_json::json!({
                        "name": name,
                        "version": version,
                        "build": build,
                        "build_number": 0,
                        "depends": [],
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let dir = channel.join(subdir);
        fs_err::create_dir_all(&dir).unwrap();
        fs_err::write(
            dir.join("repodata.json"),
            serde_json::json!({
                "info": { "subdir": subdir },
                "packages": {},
                "packages.conda": packages,
                "repodata_version": 1,
            })
            .to_string(),
        )
        .unwrap();
    }

    fn identity(name: &str, version: &str, build: &str) -> PackageIdentity {
        PackageIdentity::new(name.parse().unwrap(), version, build)
    }

    #[tokio::test]
    async fn finds_existing_packages_in_platform_and_noarch() {
        let channel_dir = tempfile::tempdir().unwrap();
        write_repodata(
            channel_dir.path(),
            "linux-64",
            &[("foo", "1.0", "h123_0"), ("foo", "1.0", "h123_1")],
        );
        write_repodata(channel_dir.path(), "noarch", &[("bar", "2.0", "pyh_0")]);
        write_repodata(channel_dir.path(), "osx-64", &[("baz", "1.0", "0")]);
        let channel = Channel::try_from_directory(channel_dir.path()).unwrap();

        let packages = [
            identity("foo", "1.0", "h123_0"),
            // Same name and version, different build string
            identity("foo", "1.0", "h456_0"),
            // Same name and build string, different version
            identity("foo", "1.1", "h123_1"),
            // From noarch
            identity("bar", "2.0", "pyh_0"),
            // Only exists for another platform
            identity("baz", "1.0", "0"),
            // Not in the channel at all
            identity("qux", "1.0", "0"),
        ];

        let cache_dir = tempfile::tempdir().unwrap();
        let gateway = Gateway::builder()
            .with_cache_dir(cache_dir.path().to_path_buf())
            .finish();
        let existing =
            find_existing_packages(&gateway, &[channel.base_url], Subdir::Linux64, &packages)
                .await
                .unwrap();

        assert_eq!(
            existing,
            HashSet::from([
                identity("foo", "1.0", "h123_0"),
                identity("bar", "2.0", "pyh_0"),
            ])
        );
    }
}
