//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::{path::PathBuf, sync::Arc};

use crate::console_utils::LoggingOutputHandler;
use rattler_networking::{authentication_storage, AuthenticationMiddleware, AuthenticationStorage};
use reqwest_middleware::ClientWithMiddleware;

/// The user agent to use for the reqwest client
pub const APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

/// Global configuration for the build
#[derive(Clone, Debug)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub fancy_log_handler: LoggingOutputHandler,

    /// The authenticated reqwest download client to use
    pub client: ClientWithMiddleware,

    /// Set this to true if you want to keep the build folder after the build is done
    pub no_clean: bool,

    /// Whether to skip the test phase
    pub no_test: bool,

    /// Whether to use zstd
    pub use_zstd: bool,

    /// Whether to use bzip2
    pub use_bz2: bool,
}

/// Get the authentication storage from the given file
pub fn get_auth_store(auth_file: Option<PathBuf>) -> AuthenticationStorage {
    match auth_file {
        Some(auth_file) => {
            let mut store = AuthenticationStorage::new();
            store.add_backend(Arc::from(
                authentication_storage::backends::file::FileStorage::new(auth_file),
            ));
            store
        }
        None => rattler_networking::AuthenticationStorage::default(),
    }
}

/// Create a reqwest client with the authentication middleware
pub fn reqwest_client_from_auth_storage(auth_file: Option<PathBuf>) -> ClientWithMiddleware {
    let auth_storage = get_auth_store(auth_file);

    let timeout = 5 * 60;
    reqwest_middleware::ClientBuilder::new(
        reqwest::Client::builder()
            .no_gzip()
            .pool_max_idle_per_host(20)
            .user_agent(APP_USER_AGENT)
            .timeout(std::time::Duration::from_secs(timeout))
            .build()
            .expect("failed to create client"),
    )
    .with_arc(Arc::new(AuthenticationMiddleware::new(auth_storage)))
    .build()
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            fancy_log_handler: LoggingOutputHandler::default(),
            client: reqwest_client_from_auth_storage(None),
            no_clean: false,
            no_test: false,
            use_zstd: true,
            use_bz2: true,
        }
    }
}
