//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::{path::PathBuf, sync::Arc};

use crate::console_utils::LoggingOutputHandler;
use clap::ValueEnum;
use rattler_conda_types::ChannelConfig;
use rattler_networking::{
    authentication_storage::{self, backends::file::FileStorageError},
    AuthenticationMiddleware, AuthenticationStorage,
};
use reqwest_middleware::ClientWithMiddleware;

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

/// Global configuration for the build
#[derive(Clone, Debug)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub fancy_log_handler: LoggingOutputHandler,

    /// The authenticated reqwest download client to use
    pub client: ClientWithMiddleware,

    /// Set this to true if you want to keep the build directory after the build is done
    pub no_clean: bool,

    /// Whether to skip the test phase
    pub no_test: bool,

    /// Whether to use zstd
    pub use_zstd: bool,

    /// Whether to use bzip2
    pub use_bz2: bool,

    /// Whether to only render the build output
    pub render_only: bool,

    /// Whether to skip existing packages
    pub skip_existing: SkipExisting,

    /// Whether to continue building on failure of a package
    pub cont_on_fail: bool,

    /// The channel configuration to use when parsing channels.
    pub channel_config: ChannelConfig,

    /// How many threads to use for compression (only relevant for `.conda` archives).
    /// This value is not serialized because the number of threads does not matter for the final result.
    pub compression_threads: Option<u32>,
}

/// Get the authentication storage from the given file
pub fn get_auth_store(
    auth_file: Option<PathBuf>,
) -> Result<AuthenticationStorage, FileStorageError> {
    match auth_file {
        Some(auth_file) => {
            let mut store = AuthenticationStorage::new();
            store.add_backend(Arc::from(
                authentication_storage::backends::file::FileStorage::new(auth_file)?,
            ));
            Ok(store)
        }
        None => Ok(rattler_networking::AuthenticationStorage::default()),
    }
}

/// Create a reqwest client with the authentication middleware
pub fn reqwest_client_from_auth_storage(
    auth_file: Option<PathBuf>,
) -> Result<ClientWithMiddleware, FileStorageError> {
    let auth_storage = get_auth_store(auth_file)?;

    let timeout = 5 * 60;
    Ok(reqwest_middleware::ClientBuilder::new(
        reqwest::Client::builder()
            .no_gzip()
            .pool_max_idle_per_host(20)
            .user_agent(APP_USER_AGENT)
            .timeout(std::time::Duration::from_secs(timeout))
            .build()
            .expect("failed to create client"),
    )
    .with_arc(Arc::new(AuthenticationMiddleware::new(auth_storage)))
    .build())
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            fancy_log_handler: LoggingOutputHandler::default(),
            client: reqwest_client_from_auth_storage(None).expect("failed to create client"),
            no_clean: false,
            no_test: false,
            use_zstd: true,
            use_bz2: true,
            render_only: false,
            skip_existing: SkipExisting::None,
            cont_on_fail: false,
            channel_config: ChannelConfig::default_with_root_dir(
                std::env::current_dir().unwrap_or_else(|_err| PathBuf::from("/")),
            ),
            compression_threads: None,
        }
    }
}
