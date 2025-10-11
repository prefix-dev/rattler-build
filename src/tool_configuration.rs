//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use clap::ValueEnum;
use rattler::package_cache::PackageCache;
use rattler_conda_types::{ChannelConfig, Platform};
use rattler_networking::{
    AuthenticationStorage,
    authentication_storage::{self, AuthenticationStorageError},
    mirror_middleware, s3_middleware,
};
use rattler_repodata_gateway::Gateway;
use rattler_solve::ChannelPriority;
use url::Url;

use crate::console_utils::LoggingOutputHandler;

/// The user agent to use for the reqwest client
pub const APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

/// Whether to skip existing packages or not
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SkipExisting {
    /// Do not skip any packages
    None,
    /// Skip packages that already exist locally
    Local,
    /// Skip packages that already exist in any channel
    All,
}

/// Container for the CLI test strategy
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum TestStrategy {
    /// Skip the tests
    Skip,
    /// Run the tests only if the build platform is the same as the host platform.
    /// Otherwise, skip the tests. If the target platform is noarch,
    /// the tests are always executed.
    Native,
    /// Always run the tests
    #[default]
    NativeAndEmulated,
}

/// Whether we want to continue building on failure of a package or stop the build
/// entirely
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinueOnFailure {
    /// Continue building on failure of a package
    Yes,
    /// Stop the build entirely on failure of a package
    #[default]
    No,
}

// This is the key part - implement From<bool> for your type
impl From<bool> for ContinueOnFailure {
    fn from(value: bool) -> Self {
        if value {
            ContinueOnFailure::Yes
        } else {
            ContinueOnFailure::No
        }
    }
}

/// Global configuration for the build
#[derive(Clone)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub fancy_log_handler: LoggingOutputHandler,

    /// The HTTP client with S3, mirrors, and auth middleware
    pub client: rattler_build_networking::BaseClient,

    /// The source cache for downloading and caching source code
    pub source_cache: Option<Arc<rattler_build_source_cache::SourceCache>>,

    /// Set this to true if you want to keep the build directory after the build
    /// is done
    pub no_clean: bool,

    /// The strategy to use for running tests
    pub test_strategy: TestStrategy,

    /// Whether to use zstd
    pub use_zstd: bool,

    /// Whether to use bzip2
    pub use_bz2: bool,

    /// Whether to use sharded repodata
    pub use_sharded: bool,

    /// Whether to use JLAP (JSON Lines Append Protocol)
    pub use_jlap: bool,

    /// Whether to skip existing packages
    pub skip_existing: SkipExisting,

    /// The noarch platform to use (noarch builds are skipped on other platforms)
    pub noarch_build_platform: Option<Platform>,

    /// The channel configuration to use when parsing channels.
    pub channel_config: ChannelConfig,

    /// How many threads to use for compression (only relevant for `.conda`
    /// archives). This value is not serialized because the number of
    /// threads does not matter for the final result.
    pub compression_threads: Option<u32>,

    /// Concurrency limit for I/O operations
    pub io_concurrency_limit: Option<usize>,

    /// The package cache to use to store packages in.
    pub package_cache: PackageCache,

    /// The repodata gateway to use for querying repodata
    pub repodata_gateway: Gateway,

    /// What channel priority to use in solving
    pub channel_priority: ChannelPriority,

    /// List of hosts for which SSL certificate verification should be skipped
    pub allow_insecure_host: Option<Vec<String>>,

    /// Whether to continue building on failure of a package or stop the build
    pub continue_on_failure: ContinueOnFailure,

    /// Whether to error if the host prefix is detected in binary files
    pub error_prefix_in_binary: bool,

    /// Whether to allow symlinks in packages on Windows (defaults to false)
    pub allow_symlinks_on_windows: bool,

    /// Whether the environments are externally managed (e.g. by `pixi-build`).
    /// This is only useful for other libraries that build their own environments and only use rattler-build
    /// to execute scripts / bundle up files.
    pub environments_externally_managed: bool,
}

/// Get the authentication storage from the given file
pub fn get_auth_store(
    auth_file: Option<PathBuf>,
) -> Result<AuthenticationStorage, AuthenticationStorageError> {
    match auth_file {
        Some(auth_file) => {
            let mut store = AuthenticationStorage::empty();
            store.add_backend(Arc::from(
                authentication_storage::backends::file::FileStorage::from_path(auth_file)?,
            ));
            Ok(store)
        }
        None => rattler_networking::AuthenticationStorage::from_env_and_defaults(),
    }
}

/// Create a reqwest client with the authentication middleware
///
/// * `auth_file` - Optional path to an authentication file
/// * `allow_insecure_host` - Optional list of hosts for which to disable SSL certificate verification
pub fn reqwest_client_from_auth_storage(
    auth_file: Option<PathBuf>,
    s3_middleware_config: HashMap<String, s3_middleware::S3Config>,
    mirror_middleware_config: HashMap<Url, Vec<mirror_middleware::Mirror>>,
    allow_insecure_host: Option<Vec<String>>,
) -> Result<rattler_build_networking::BaseClient, AuthenticationStorageError> {
    let auth_storage = get_auth_store(auth_file)?;

    Ok(rattler_build_networking::BaseClient::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(5 * 60)
        .insecure_hosts(allow_insecure_host.unwrap_or_default())
        .with_authentication(auth_storage)
        .with_s3(s3_middleware_config)
        .with_mirrors(mirror_middleware_config)
        .build())
}

/// A builder for a [`Configuration`].
pub struct ConfigurationBuilder {
    cache_dir: Option<PathBuf>,
    fancy_log_handler: Option<LoggingOutputHandler>,
    client: Option<rattler_build_networking::BaseClient>,
    no_clean: bool,
    no_test: bool,
    test_strategy: TestStrategy,
    use_zstd: bool,
    use_bz2: bool,
    use_sharded: bool,
    use_jlap: bool,
    skip_existing: SkipExisting,
    noarch_build_platform: Option<Platform>,
    channel_config: Option<ChannelConfig>,
    compression_threads: Option<u32>,
    io_concurrency_limit: Option<usize>,
    channel_priority: ChannelPriority,
    allow_insecure_host: Option<Vec<String>>,
    continue_on_failure: ContinueOnFailure,
    error_prefix_in_binary: bool,
    allow_symlinks_on_windows: bool,
    environments_externally_managed: bool,
}

impl Configuration {
    /// Constructs a new builder for the configuration. Using the builder allows
    /// customizing the default configuration.
    pub fn builder() -> ConfigurationBuilder {
        ConfigurationBuilder::new()
    }
}

impl ConfigurationBuilder {
    fn new() -> Self {
        Self {
            cache_dir: None,
            fancy_log_handler: None,
            client: None,
            no_clean: false,
            no_test: false,
            test_strategy: TestStrategy::default(),
            use_zstd: true,
            use_bz2: true,
            use_sharded: true,
            use_jlap: false,
            skip_existing: SkipExisting::None,
            noarch_build_platform: None,
            channel_config: None,
            compression_threads: None,
            io_concurrency_limit: None,
            channel_priority: ChannelPriority::Strict,
            allow_insecure_host: None,
            continue_on_failure: ContinueOnFailure::No,
            error_prefix_in_binary: false,
            allow_symlinks_on_windows: false,
            environments_externally_managed: false,
        }
    }

    /// Set the default cache directory to use for objects that need to be
    /// cached.
    pub fn with_cache_dir(self, cache_dir: PathBuf) -> Self {
        Self {
            cache_dir: Some(cache_dir),
            ..self
        }
    }

    /// Whether to continue building on failure of a package or stop the build
    pub fn with_continue_on_failure(self, continue_on_failure: ContinueOnFailure) -> Self {
        Self {
            continue_on_failure,
            ..self
        }
    }

    /// Whether to error if the host prefix is detected in binary files
    pub fn with_error_prefix_in_binary(self, error_prefix_in_binary: bool) -> Self {
        Self {
            error_prefix_in_binary,
            ..self
        }
    }

    /// Whether to allow symlinks in packages on Windows
    pub fn with_allow_symlinks_on_windows(self, allow_symlinks_on_windows: bool) -> Self {
        Self {
            allow_symlinks_on_windows,
            ..self
        }
    }

    /// Set the default cache directory to use for objects that need to be
    /// cached.
    pub fn with_opt_cache_dir(self, cache_dir: Option<PathBuf>) -> Self {
        Self { cache_dir, ..self }
    }

    /// Set the logging output handler to use for logging
    pub fn with_logging_output_handler(self, fancy_log_handler: LoggingOutputHandler) -> Self {
        Self {
            fancy_log_handler: Some(fancy_log_handler),
            ..self
        }
    }

    /// Set whether to skip outputs that have already been build.
    pub fn with_skip_existing(self, skip_existing: SkipExisting) -> Self {
        Self {
            skip_existing,
            ..self
        }
    }

    /// Set the channel configuration to use.
    pub fn with_channel_config(self, channel_config: ChannelConfig) -> Self {
        Self {
            channel_config: Some(channel_config),
            ..self
        }
    }

    /// Set the number of threads to use for compression, or `None` to use the
    /// number of cores.
    pub fn with_compression_threads(self, compression_threads: Option<u32>) -> Self {
        Self {
            compression_threads,
            ..self
        }
    }

    /// Set the maximum I/O concurrency during package installation or None to use
    /// a default based on number of cores
    pub fn with_io_concurrency_limit(self, io_concurrency_limit: Option<usize>) -> Self {
        Self {
            io_concurrency_limit,
            ..self
        }
    }

    /// Sets whether to keep the build output or delete it after the build is
    /// done.
    pub fn with_keep_build(self, keep_build: bool) -> Self {
        Self {
            no_clean: keep_build,
            ..self
        }
    }

    /// Sets the request client to use for network requests.
    pub fn with_reqwest_client(self, client: rattler_build_networking::BaseClient) -> Self {
        Self {
            client: Some(client),
            ..self
        }
    }

    /// Sets whether tests should be executed.
    pub fn with_testing(self, testing_enabled: bool) -> Self {
        Self {
            no_test: !testing_enabled,
            ..self
        }
    }

    /// Sets the test strategy to use for running tests.
    pub fn with_test_strategy(self, test_strategy: TestStrategy) -> Self {
        Self {
            test_strategy,
            ..self
        }
    }

    /// Whether downloading repodata as `.zst` files is enabled.
    pub fn with_zstd_repodata_enabled(self, zstd_repodata_enabled: bool) -> Self {
        Self {
            use_zstd: zstd_repodata_enabled,
            ..self
        }
    }

    /// Whether downloading repodata as `.bz2` files is enabled.
    pub fn with_bz2_repodata_enabled(self, bz2_repodata_enabled: bool) -> Self {
        Self {
            use_bz2: bz2_repodata_enabled,
            ..self
        }
    }

    /// Whether downloading sharded repodata is enabled.
    pub fn with_sharded_repodata_enabled(self, sharded_repodata_enabled: bool) -> Self {
        Self {
            use_sharded: sharded_repodata_enabled,
            ..self
        }
    }

    /// Whether using JLAP (JSON Lines Append Protocol) is enabled.
    pub fn with_jlap_enabled(self, jlap_enabled: bool) -> Self {
        Self {
            use_jlap: jlap_enabled,
            ..self
        }
    }

    /// Define the noarch platform
    pub fn with_noarch_build_platform(self, noarch_build_platform: Option<Platform>) -> Self {
        Self {
            noarch_build_platform,
            ..self
        }
    }

    /// Sets the channel priority to be used when solving environments
    pub fn with_channel_priority(self, channel_priority: ChannelPriority) -> Self {
        Self {
            channel_priority,
            ..self
        }
    }

    /// Set the list of hosts for which SSL certificate verification should be skipped
    pub fn with_allow_insecure_host(self, allow_insecure_host: Option<Vec<String>>) -> Self {
        Self {
            allow_insecure_host,
            ..self
        }
    }

    /// Set whether the environments are externally managed (e.g. by `pixi-build`).
    /// This is only useful for other libraries that build their own environments and only use rattler
    /// to execute scripts / bundle up files.
    pub fn with_environments_externally_managed(
        self,
        environments_externally_managed: bool,
    ) -> Self {
        Self {
            environments_externally_managed,
            ..self
        }
    }

    /// Construct a [`Configuration`] from the builder.
    pub fn finish(self) -> Configuration {
        let cache_dir = self.cache_dir.unwrap_or_else(|| {
            rattler_cache::default_cache_dir().expect("failed to determine default cache directory")
        });
        let client = self.client.unwrap_or_default();
        let package_cache = PackageCache::new(cache_dir.join(rattler_cache::PACKAGE_CACHE_DIR));
        let channel_config = self.channel_config.unwrap_or_else(|| {
            ChannelConfig::default_with_root_dir(
                std::env::current_dir().unwrap_or_else(|_err| PathBuf::from("/")),
            )
        });
        let repodata_gateway = Gateway::builder()
            .with_cache_dir(cache_dir.join(rattler_cache::REPODATA_CACHE_DIR))
            .with_package_cache(package_cache.clone())
            .with_client(client.get_client().clone())
            .with_channel_config(rattler_repodata_gateway::ChannelConfig {
                default: rattler_repodata_gateway::SourceConfig {
                    jlap_enabled: self.use_jlap,
                    zstd_enabled: self.use_zstd,
                    bz2_enabled: self.use_bz2,
                    sharded_enabled: self.use_sharded,
                    cache_action: Default::default(),
                },
                per_channel: Default::default(),
            })
            .finish();

        let test_strategy = match self.no_test {
            true => TestStrategy::Skip,
            false => self.test_strategy,
        };

        Configuration {
            fancy_log_handler: self.fancy_log_handler.unwrap_or_default(),
            client,
            source_cache: None, // Built lazily on first use
            no_clean: self.no_clean,
            test_strategy,
            use_zstd: self.use_zstd,
            use_bz2: self.use_bz2,
            use_sharded: self.use_sharded,
            use_jlap: self.use_jlap,
            skip_existing: self.skip_existing,
            noarch_build_platform: self.noarch_build_platform,
            channel_config,
            compression_threads: self.compression_threads,
            io_concurrency_limit: self.io_concurrency_limit,
            package_cache,
            repodata_gateway,
            channel_priority: self.channel_priority,
            allow_insecure_host: self.allow_insecure_host,
            continue_on_failure: self.continue_on_failure,
            error_prefix_in_binary: self.error_prefix_in_binary,
            allow_symlinks_on_windows: self.allow_symlinks_on_windows,
            environments_externally_managed: self.environments_externally_managed,
        }
    }
}
