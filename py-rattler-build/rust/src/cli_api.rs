//! The functions in this module should not be needed anymore
//! now that we have a more powerful API.
//! We should remove this module at some point

use std::{collections::HashMap, path::PathBuf, str::FromStr};

use ::rattler_build::{
    build_recipes,
    metadata::Debug,
    opt::{BuildData, ChannelPriorityWrapper, CommonData, TestData},
    run_test,
    tool_configuration::{ContinueOnFailure, SkipExisting, TestStrategy},
};
use clap::ValueEnum;
use pyo3::prelude::*;
use rattler_conda_types::{NamedChannelOrUrl, Platform};
use rattler_config::config::{ConfigBase, build::PackageFormatAndCompression};

use crate::error::RattlerBuildError;
use crate::run_async_task;

#[pyfunction]
#[pyo3(signature = (recipes, up_to, build_platform, target_platform, host_platform, channel, variant_config, variant_overrides=None, ignore_recipe_variants=false, render_only=false, with_solve=false, keep_build=false, no_build_id=false, package_format=None, compression_threads=None, io_concurrency_limit=None, no_include_recipe=false, test=None, output_dir=None, auth_file=None, channel_priority=None, skip_existing=None, noarch_build_platform=None, allow_insecure_host=None, continue_on_failure=false, debug=false, error_prefix_in_binary=false, allow_symlinks_on_windows=false, allow_absolute_license_paths=false, exclude_newer=None, build_num=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
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
    debug: bool,
    error_prefix_in_binary: bool,
    allow_symlinks_on_windows: bool,
    allow_absolute_license_paths: bool,
    exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
    build_num: Option<u64>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    // todo: allow custom config here
    let config = ConfigBase::<()>::default();
    let common = CommonData::new(
        output_dir,
        false,
        auth_file.map(|a| a.into()),
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_jlap,
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
        false, // TUI disabled
        skip_existing,
        noarch_build_platform,
        None, // extra meta
        None, // sandbox configuration
        Debug::new(debug),
        ContinueOnFailure::from(continue_on_failure),
        error_prefix_in_binary,
        allow_symlinks_on_windows,
        allow_absolute_license_paths,
        exclude_newer,
        build_num,
    );

    run_async_task(async {
        build_recipes(recipes, build_data, &None).await?;
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (package_file, channel, compression_threads, auth_file, channel_priority, allow_insecure_host=None, debug=false, test_index=None, use_bz2=true, use_zstd=true, use_jlap=false, use_sharded=true))]
pub fn test_package_py(
    package_file: PathBuf,
    channel: Option<Vec<String>>,
    compression_threads: Option<u32>,
    auth_file: Option<PathBuf>,
    channel_priority: Option<String>,
    allow_insecure_host: Option<Vec<String>>,
    debug: bool,
    test_index: Option<usize>,
    use_bz2: bool,
    use_zstd: bool,
    use_jlap: bool,
    use_sharded: bool,
) -> PyResult<()> {
    let channel_priority = channel_priority
        .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
        .transpose()
        .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;
    // todo: allow custom config here
    let config = ConfigBase::<()>::default();
    let common = CommonData::new(
        None,
        false,
        auth_file,
        config,
        channel_priority,
        allow_insecure_host,
        use_bz2,
        use_zstd,
        use_jlap,
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
        Debug::new(debug),
        test_index,
        common,
    );

    run_async_task(async {
        run_test(test_data, None).await?;
        Ok(())
    })
}
