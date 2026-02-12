use clap::ValueEnum;
use pyo3::prelude::*;
use rattler_build::tool_configuration::{
    Configuration, ContinueOnFailure, SkipExisting, TestStrategy,
};
use rattler_conda_types::Platform;
use rattler_solve::ChannelPriority;

use crate::error::RattlerBuildError;

/// Python wrapper for ToolConfiguration
#[pyclass(name = "ToolConfiguration")]
#[derive(Clone)]
pub struct PyToolConfiguration {
    pub(crate) inner: Configuration,
}

#[pymethods]
impl PyToolConfiguration {
    /// Create a new tool configuration with default settings
    #[new]
    #[pyo3(signature = (
        keep_build=false,
        compression_threads=None,
        io_concurrency_limit=None,
        test_strategy=None,
        skip_existing=None,
        continue_on_failure=false,
        noarch_build_platform=None,
        channel_priority=None,
        allow_insecure_host=None,
        error_prefix_in_binary=false,
        allow_symlinks_on_windows=false,
        use_zstd=true,
        use_bz2=true,
        use_sharded=true,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        keep_build: bool,
        compression_threads: Option<u32>,
        io_concurrency_limit: Option<usize>,
        test_strategy: Option<String>,
        skip_existing: Option<String>,
        continue_on_failure: bool,
        noarch_build_platform: Option<String>,
        channel_priority: Option<String>,
        allow_insecure_host: Option<Vec<String>>,
        error_prefix_in_binary: bool,
        allow_symlinks_on_windows: bool,
        use_zstd: bool,
        use_bz2: bool,
        use_sharded: bool,
    ) -> PyResult<Self> {
        let channel_priority = channel_priority
            .map(|c| match c.to_lowercase().as_str() {
                "strict" => Ok(ChannelPriority::Strict),
                "disabled" => Ok(ChannelPriority::Disabled),
                _ => Err(RattlerBuildError::Other(format!(
                    "Invalid channel priority: {}. Must be 'strict' or 'disabled'",
                    c
                ))),
            })
            .transpose()?
            .unwrap_or(ChannelPriority::Strict);

        let test_strategy = test_strategy
            .map(|t| {
                TestStrategy::from_str(&t, false)
                    .map_err(|e| RattlerBuildError::Other(format!("Invalid test strategy: {}", e)))
            })
            .transpose()?
            .unwrap_or(TestStrategy::Skip);

        let skip_existing = skip_existing
            .map(|s| {
                SkipExisting::from_str(&s, false)
                    .map_err(|e| RattlerBuildError::Other(format!("Invalid skip existing: {}", e)))
            })
            .transpose()?
            .unwrap_or(SkipExisting::None);

        let noarch_build_platform = noarch_build_platform
            .map(|p| {
                p.parse::<Platform>()
                    .map_err(|e| RattlerBuildError::Other(format!("Invalid platform: {}", e)))
            })
            .transpose()?;

        let config = rattler_config::config::ConfigBase::<()>::default();

        let mut builder = Configuration::builder()
            .with_keep_build(keep_build)
            .with_test_strategy(test_strategy)
            .with_skip_existing(skip_existing)
            .with_continue_on_failure(ContinueOnFailure::from(continue_on_failure))
            .with_channel_priority(channel_priority)
            .with_error_prefix_in_binary(error_prefix_in_binary)
            .with_allow_symlinks_on_windows(allow_symlinks_on_windows)
            .with_zstd_repodata_enabled(use_zstd)
            .with_bz2_repodata_enabled(use_bz2)
            .with_sharded_repodata_enabled(use_sharded)
            .with_channel_config(config.channel_config);

        if let Some(threads) = compression_threads {
            builder = builder.with_compression_threads(Some(threads));
        }

        if let Some(limit) = io_concurrency_limit {
            builder = builder.with_io_concurrency_limit(Some(limit));
        }

        if let Some(platform) = noarch_build_platform {
            builder = builder.with_noarch_build_platform(Some(platform));
        }

        if let Some(hosts) = allow_insecure_host {
            builder = builder.with_allow_insecure_host(Some(hosts));
        }

        Ok(Self {
            inner: builder.finish(),
        })
    }

    /// Whether to keep the build directory after the build is done
    #[getter]
    fn keep_build(&self) -> bool {
        self.inner.no_clean
    }

    /// The test strategy to use
    #[getter]
    fn test_strategy(&self) -> String {
        format!("{:?}", self.inner.test_strategy)
    }

    /// Whether to skip existing packages
    #[getter]
    fn skip_existing(&self) -> String {
        format!("{:?}", self.inner.skip_existing)
    }

    /// Whether to continue building on failure
    #[getter]
    fn continue_on_failure(&self) -> bool {
        matches!(self.inner.continue_on_failure, ContinueOnFailure::Yes)
    }

    /// The channel priority to use in solving
    #[getter]
    fn channel_priority(&self) -> String {
        format!("{:?}", self.inner.channel_priority)
    }

    /// Whether to use zstd compression
    #[getter]
    fn use_zstd(&self) -> bool {
        self.inner.use_zstd
    }

    /// Whether to use bzip2 compression
    #[getter]
    fn use_bz2(&self) -> bool {
        self.inner.use_bz2
    }

    /// Whether to use sharded repodata
    #[getter]
    fn use_sharded(&self) -> bool {
        self.inner.use_sharded
    }

    /// Compression threads
    #[getter]
    fn compression_threads(&self) -> Option<u32> {
        self.inner.compression_threads
    }

    /// IO concurrency limit
    #[getter]
    fn io_concurrency_limit(&self) -> Option<usize> {
        self.inner.io_concurrency_limit
    }

    /// List of hosts for which SSL certificate verification should be skipped
    #[getter]
    fn allow_insecure_host(&self) -> Option<Vec<String>> {
        self.inner.allow_insecure_host.clone()
    }

    /// Whether to error if the host prefix is detected in binary files
    #[getter]
    fn error_prefix_in_binary(&self) -> bool {
        self.inner.error_prefix_in_binary
    }

    /// Whether to allow symlinks in packages on Windows
    #[getter]
    fn allow_symlinks_on_windows(&self) -> bool {
        self.inner.allow_symlinks_on_windows
    }

    fn __repr__(&self) -> String {
        format!(
            "ToolConfiguration(keep_build={}, test_strategy={:?}, skip_existing={:?}, continue_on_failure={}, channel_priority={:?})",
            self.inner.no_clean,
            self.inner.test_strategy,
            self.inner.skip_existing,
            matches!(self.inner.continue_on_failure, ContinueOnFailure::Yes),
            self.inner.channel_priority
        )
    }
}

/// Register the tool_config module with Python
pub fn register_tool_config_module(py: Python<'_>, parent: &Bound<'_, PyModule>) -> PyResult<()> {
    let m = PyModule::new(py, "tool_config")?;
    m.add_class::<PyToolConfiguration>()?;
    parent.add_submodule(&m)?;
    Ok(())
}
