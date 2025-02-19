//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::{path::PathBuf, sync::Arc};

use clap::ValueEnum;
use rattler::package_cache::PackageCache;
use rattler_cache::run_exports_cache::RunExportsCache;
use rattler_conda_types::{ChannelConfig, Platform};
use rattler_networking::{
    authentication_storage::{self, AuthenticationStorageError},
    AuthenticationMiddleware, AuthenticationStorage,
};
use rattler_repodata_gateway::Gateway;
use rattler_solve::ChannelPriority;
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};

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

/// Global configuration for the build
#[derive(Clone)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub fancy_log_handler: LoggingOutputHandler,

    /// The authenticated reqwest download client to use
    pub client: ClientWithMiddleware,

    /// Set this to true if you want to keep the build directory after the build
    /// is done
    pub no_clean: bool,

    /// The strategy to use for running tests
    pub test_strategy: TestStrategy,

    /// Whether to use zstd
    pub use_zstd: bool,

    /// Whether to use bzip2
    pub use_bz2: bool,

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

    /// The package cache to use to store packages in.
    pub package_cache: PackageCache,

    /// The run exports cache to use to store run exports in.
    pub run_exports_cache: RunExportsCache,

    /// The repodata gateway to use for querying repodata
    pub repodata_gateway: Gateway,

    /// What channel priority to use in solving
    pub channel_priority: ChannelPriority,
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
pub fn reqwest_client_from_auth_storage(
    auth_file: Option<PathBuf>,
) -> Result<ClientWithMiddleware, AuthenticationStorageError> {
    let auth_storage = get_auth_store(auth_file)?;

    let timeout = 5 * 60;
    Ok(reqwest_middleware::ClientBuilder::new(
        reqwest::Client::builder()
            .no_gzip()
            .pool_max_idle_per_host(20)
            .user_agent(APP_USER_AGENT)
            .read_timeout(std::time::Duration::from_secs(timeout))
            .build()
            .expect("failed to create client"),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(3),
    ))
    .with_arc(Arc::new(AuthenticationMiddleware::from_auth_storage(
        auth_storage,
    )))
    .build())
}

/// A builder for a [`Configuration`].
pub struct ConfigurationBuilder {
    cache_dir: Option<PathBuf>,
    fancy_log_handler: Option<LoggingOutputHandler>,
    client: Option<ClientWithMiddleware>,
    no_clean: bool,
    no_test: bool,
    test_strategy: TestStrategy,
    use_zstd: bool,
    use_bz2: bool,
    skip_existing: SkipExisting,
    noarch_build_platform: Option<Platform>,
    channel_config: Option<ChannelConfig>,
    compression_threads: Option<u32>,
    channel_priority: ChannelPriority,
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
            skip_existing: SkipExisting::None,
            noarch_build_platform: None,
            channel_config: None,
            compression_threads: None,
            channel_priority: ChannelPriority::Strict,
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

    /// Sets whether to keep the build output or delete it after the build is
    /// done.
    pub fn with_keep_build(self, keep_build: bool) -> Self {
        Self {
            no_clean: keep_build,
            ..self
        }
    }

    /// Sets the request client to use for network requests.
    pub fn with_reqwest_client(self, client: ClientWithMiddleware) -> Self {
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

    /// Construct a [`Configuration`] from the builder.
    pub fn finish(self) -> Configuration {
        let cache_dir = self.cache_dir.unwrap_or_else(|| {
            rattler_cache::default_cache_dir().expect("failed to determine default cache directory")
        });
        let client = self.client.unwrap_or_else(|| {
            reqwest_client_from_auth_storage(None).expect("failed to create client")
        });
        let package_cache = PackageCache::new(cache_dir.join(rattler_cache::PACKAGE_CACHE_DIR));
        let run_exports_cache =
            RunExportsCache::new(cache_dir.join(rattler_cache::RUN_EXPORTS_CACHE_DIR));
        let channel_config = self.channel_config.unwrap_or_else(|| {
            ChannelConfig::default_with_root_dir(
                std::env::current_dir().unwrap_or_else(|_err| PathBuf::from("/")),
            )
        });
        let repodata_gateway = Gateway::builder()
            .with_cache_dir(cache_dir.join(rattler_cache::REPODATA_CACHE_DIR))
            .with_package_cache(package_cache.clone())
            .with_client(client.clone())
            .with_channel_config(rattler_repodata_gateway::ChannelConfig {
                default: rattler_repodata_gateway::SourceConfig {
                    jlap_enabled: true,
                    zstd_enabled: self.use_zstd,
                    bz2_enabled: self.use_bz2,
                    sharded_enabled: true,
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
            no_clean: self.no_clean,
            test_strategy,
            use_zstd: self.use_zstd,
            use_bz2: self.use_bz2,
            skip_existing: self.skip_existing,
            noarch_build_platform: self.noarch_build_platform,
            channel_config,
            compression_threads: self.compression_threads,
            package_cache,
            run_exports_cache,
            repodata_gateway,
            channel_priority: self.channel_priority,
        }
    }
}
