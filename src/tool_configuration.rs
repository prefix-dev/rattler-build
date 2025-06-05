//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use clap::ValueEnum;
use rattler::package_cache::PackageCache;
use rattler_conda_types::{ChannelConfig, Platform};
use rattler_networking::{
    AuthenticationMiddleware, AuthenticationStorage,
    authentication_storage::{self, AuthenticationStorageError},
    mirror_middleware, s3_middleware,
};
use rattler_repodata_gateway::Gateway;
use rattler_solve::ChannelPriority;
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
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

/// A client that can handle both secure and insecure connections
#[derive(Clone, Default)]
pub struct BaseClient {
    /// The standard client with SSL verification enabled
    client: ClientWithMiddleware,
    /// The dangerous client with SSL verification disabled
    dangerous_client: ClientWithMiddleware,
    /// List of hosts for which SSL verification should be skipped
    allow_insecure_host: Option<Vec<String>>,
}

impl BaseClient {
    /// Create a new BaseClient with both secure and insecure clients
    pub fn new(
        auth_file: Option<PathBuf>,
        allow_insecure_host: Option<Vec<String>>,
        s3_middleware_config: HashMap<String, s3_middleware::S3Config>,
        mirror_middleware_config: HashMap<Url, Vec<mirror_middleware::Mirror>>,
    ) -> Result<Self, AuthenticationStorageError> {
        let auth_storage = get_auth_store(auth_file)?;
        let timeout = 5 * 60;

        let s3_middleware =
            s3_middleware::S3Middleware::new(s3_middleware_config, auth_storage.clone());
        let mirror_middleware =
            mirror_middleware::MirrorMiddleware::from_map(mirror_middleware_config);

        let common_settings = |builder: reqwest::ClientBuilder| -> reqwest::ClientBuilder {
            builder
                .no_gzip()
                .pool_max_idle_per_host(20)
                .user_agent(APP_USER_AGENT)
                .read_timeout(std::time::Duration::from_secs(timeout))
        };

        let client = reqwest_middleware::ClientBuilder::new(
            common_settings(reqwest::Client::builder())
                .build()
                .expect("failed to create client"),
        )
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .with_arc(Arc::new(AuthenticationMiddleware::from_auth_storage(
            auth_storage.clone(),
        )))
        .with(mirror_middleware)
        .with(s3_middleware)
        .build();

        let dangerous_client = reqwest_middleware::ClientBuilder::new(
            common_settings(reqwest::Client::builder())
                .danger_accept_invalid_certs(true)
                .build()
                .expect("failed to create dangerous client"),
        )
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .with_arc(Arc::new(AuthenticationMiddleware::from_auth_storage(
            auth_storage,
        )))
        .build();

        Ok(Self {
            client,
            dangerous_client,
            allow_insecure_host,
        })
    }

    /// Get the default client (with SSL verification enabled)
    pub fn get_client(&self) -> &ClientWithMiddleware {
        &self.client
    }

    /// Selects the appropriate client based on the host's trustworthiness
    pub fn for_host(&self, url: &Url) -> &ClientWithMiddleware {
        if self.disable_ssl(url) {
            &self.dangerous_client
        } else {
            &self.client
        }
    }

    /// Returns true if SSL verification should be disabled for the given URL
    fn disable_ssl(&self, url: &Url) -> bool {
        if let Some(hosts) = &self.allow_insecure_host {
            if let Some(host) = url.host_str() {
                return hosts.iter().any(|h| h == host);
            }
        }
        false
    }
}

/// Global configuration for the build
#[derive(Clone)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub fancy_log_handler: LoggingOutputHandler,

    /// The authenticated reqwest download client to use
    pub client: BaseClient,

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
) -> Result<BaseClient, AuthenticationStorageError> {
    BaseClient::new(
        auth_file,
        allow_insecure_host,
        s3_middleware_config,
        mirror_middleware_config,
    )
}

/// A builder for a [`Configuration`].
pub struct ConfigurationBuilder {
    cache_dir: Option<PathBuf>,
    fancy_log_handler: Option<LoggingOutputHandler>,
    client: Option<BaseClient>,
    no_clean: bool,
    no_test: bool,
    test_strategy: TestStrategy,
    use_zstd: bool,
    use_bz2: bool,
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
            io_concurrency_limit: None,
            channel_priority: ChannelPriority::Strict,
            allow_insecure_host: None,
            continue_on_failure: ContinueOnFailure::No,
            error_prefix_in_binary: false,
            allow_symlinks_on_windows: false,
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
    pub fn with_reqwest_client(self, client: BaseClient) -> Self {
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

    /// Set the list of hosts for which SSL certificate verification should be skipped
    pub fn with_allow_insecure_host(self, allow_insecure_host: Option<Vec<String>>) -> Self {
        Self {
            allow_insecure_host,
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
            .with_client(client.client.clone())
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
            io_concurrency_limit: self.io_concurrency_limit,
            package_cache,
            repodata_gateway,
            channel_priority: self.channel_priority,
            allow_insecure_host: self.allow_insecure_host,
            continue_on_failure: self.continue_on_failure,
            error_prefix_in_binary: self.error_prefix_in_binary,
            allow_symlinks_on_windows: self.allow_symlinks_on_windows,
        }
    }
}
