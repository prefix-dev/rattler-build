//! The functions in this module should not be needed anymore
//! now that we have a more powerful API.
//! We should remove this module at some point

use std::{collections::HashMap, path::PathBuf, str::FromStr};

use ::rattler_build::config::Config;
use ::rattler_build::{
    build_recipes,
    opt::{BuildData, ChannelPriorityWrapper, CommonData, TestData},
    render_recipes, run_test,
    tool_configuration::{ContinueOnFailure, SkipExisting, TestStrategy},
};
use clap::ValueEnum;
use pyo3::prelude::*;
use rattler_build_script::EnvironmentIsolation;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::build::PackageFormatAndCompression;

use crate::error::RattlerBuildError;
use crate::repodata_revision::PyRepodataRevision;
use crate::run_async_task;

#[pyfunction]
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, variant_overrides=None, ignore_recipe_variants=false, render_only=false, with_solve=false, keep_build=false, no_build_id=false, package_format=None, compression_threads=None, io_concurrency_limit=None, no_include_recipe=false, test=None, output_dir=None, auth_file=None, channel_priority=None, skip_existing=None, noarch_build_platform=None, allow_insecure_host=None, continue_on_failure=false, error_prefix_in_binary=false, allow_symlinks_on_windows=false, allow_absolute_license_paths=false, exclude_newer=None, build_num=None, build_string_prefix=None, use_bz2=true, use_zstd=true, use_sharded=true, repodata_revision=None))]
#[allow(clippy::too_many_arguments)]
pub fn build_recipes_py(
    recipes: Vec<PathBuf>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<PathBuf>>,
    variant_overrides: Option<HashMap<String, Vec<String>>>,
    ignore_recipe_variants: bool,
    render_only: bool,
    with_solve: bool,
    keep_build: bool,
    no_build_id: bool,
    package_format: Option<String>,
    compression_threads: Option<u32>,
    io_concurrency_limit: Option<usize>,
    no_include_recipe: bool,
    test: Option<String>,
    output_dir: Option<PathBuf>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    skip_existing: Option<String>,
    noarch_build_platform: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    continue_on_failure: bool,
    error_prefix_in_binary: bool,
    allow_symlinks_on_windows: bool,
    allow_absolute_license_paths: bool,
    exclude_newer: Option<jiff::Timestamp>,
    build_num: Option<u64>,
    build_string_prefix: Option<String>,
    use_bz2: bool,
    use_zstd: bool,
    use_sharded: bool,
    repodata_revision: Option<PyRepodataRevision>,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    let config = Config::default();
    let v3 = matches!(
        repodata_revision.unwrap_or_default(),
        PyRepodataRevision::V3
    );
    let common = CommonData::new(
        output_dir,
        false,
        v3,
        auth_file.map(|a| a.into()),
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_sharded,
    );
    let build_platform = build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let package_format = package_format
        .map(|p| PackageFormatAndCompression::from_str(&p))
        .transpose()
        .map_err(|e| RattlerBuildError::PackageFormat(e.to_string()))?;
    let test = test.map(|t| TestStrategy::from_str(&t, false).unwrap());
    let skip_existing = skip_existing.map(|s| SkipExisting::from_str(&s, false).unwrap());
    let noarch_build_platform = noarch_build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                        .map_err(|e| e.into())
                })
                .collect::<PyResult<_>>()?,
        ),
    };

    let build_data = BuildData::new(
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        variant_overrides.unwrap_or_default(),
        ignore_recipe_variants,
        render_only,
        with_solve,
        keep_build,
        no_build_id,
        package_format,
        compression_threads,
        io_concurrency_limit,
        no_include_recipe,
        test,
        common,
        skip_existing,
        noarch_build_platform,
        None, // extra meta
        None, // sandbox configuration
        EnvironmentIsolation::default(),
        ContinueOnFailure::from(continue_on_failure),
        error_prefix_in_binary,
        allow_symlinks_on_windows,
        allow_absolute_license_paths,
        exclude_newer,
        build_num,
        build_string_prefix,
        None, // markdown_summary
    );

    run_async_task(async {
        build_recipes(recipes, build_data, &None).await?;
        Ok(())
    })
}

/// Render recipes without building them.
///
/// Returns the same JSON string that `rattler-build build --render-only`
/// prints: a list of outputs with their rendered recipe and build
/// configuration, skip-filtered and sorted topologically.
#[pyfunction]
#[pyo3(signature = (recipes, up_to=None, build_platform=None, target_platform=None, host_platform=None, channel=None, variant_config=None, variant_overrides=None, ignore_recipe_variants=false, with_solve=false, no_build_id=false, output_dir=None, auth_file=None, channel_priority=None, allow_insecure_host=None, exclude_newer=None, build_num=None, build_string_prefix=None, use_bz2=true, use_zstd=true, use_sharded=true, repodata_revision=None))]
#[allow(clippy::too_many_arguments)]
pub fn render_recipes_py(
    recipes: Vec<PathBuf>,
    up_to: Option<String>,
    build_platform: Option<String>,
    target_platform: Option<String>,
    host_platform: Option<String>,
    channel: Option<Vec<String>>,
    variant_config: Option<Vec<PathBuf>>,
    variant_overrides: Option<HashMap<String, Vec<String>>>,
    ignore_recipe_variants: bool,
    with_solve: bool,
    no_build_id: bool,
    output_dir: Option<PathBuf>,
    auth_file: Option<String>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    exclude_newer: Option<jiff::Timestamp>,
    build_num: Option<u64>,
    build_string_prefix: Option<String>,
    use_bz2: bool,
    use_zstd: bool,
    use_sharded: bool,
    repodata_revision: Option<PyRepodataRevision>,
) -> PyResult<String> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    let config = Config::default();
    let v3 = matches!(
        repodata_revision.unwrap_or_default(),
        PyRepodataRevision::V3
    );
    let common = CommonData::new(
        output_dir,
        false,
        v3,
        auth_file.map(|a| a.into()),
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_sharded,
    );
    let build_platform = build_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let target_platform = target_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let host_platform = host_platform
        .map(|p| Platform::from_str(&p))
        .transpose()
        .map_err(RattlerBuildError::from)?;
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                        .map_err(|e| e.into())
                })
                .collect::<PyResult<_>>()?,
        ),
    };

    let build_data = BuildData::new(
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        variant_overrides.unwrap_or_default(),
        ignore_recipe_variants,
        true, // render_only
        with_solve,
        false, // keep_build
        no_build_id,
        None,  // package_format
        None,  // compression_threads
        None,  // io_concurrency_limit
        false, // no_include_recipe
        None,  // test
        common,
        None, // skip_existing
        None, // noarch_build_platform
        None, // extra meta
        None, // sandbox configuration
        EnvironmentIsolation::default(),
        ContinueOnFailure::from(false),
        false, // error_prefix_in_binary
        false, // allow_symlinks_on_windows
        false, // allow_absolute_license_paths
        exclude_newer,
        build_num,
        build_string_prefix,
        None, // markdown_summary
    );

    run_async_task(async {
        let outputs = render_recipes(recipes, &build_data, &None).await?;
        serde_json::to_string(&outputs)
            .map_err(|e| miette::miette!("failed to serialize rendered outputs: {e}"))
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (package_file, channel, compression_threads, auth_file, channel_priority, allow_insecure_host=None, test_index=None, use_bz2=true, use_zstd=true, use_sharded=true))]
pub fn test_package_py(
    package_file: PathBuf,
    channel: Option<Vec<String>>,
    compression_threads: Option<u32>,
    auth_file: Option<PathBuf>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    test_index: Option<usize>,
    use_bz2: bool,
    use_zstd: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    let config = Config::default();
    let common = CommonData::new(
        None,
        false,
        false,
        auth_file,
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_sharded,
    );
    let channel = match channel {
        None => None,
        Some(channel) => Some(
            channel
                .iter()
                .map(|c| {
                    NamedChannelOrUrl::from_str(c)
                        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                        .map_err(|e| e.into())
                })
                .collect::<PyResult<_>>()?,
        ),
    };
    let test_data = TestData::new(
        package_file,
        channel,
        compression_threads,
        test_index,
        common,
    );

    run_async_task(async {
        run_test(test_data, None).await?;
        Ok(())
    })
}
