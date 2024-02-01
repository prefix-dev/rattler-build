//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::{path::PathBuf, sync::Arc};

use rattler_networking::{authentication_storage, AuthenticationMiddleware, AuthenticationStorage};
use reqwest_middleware::ClientWithMiddleware;

/// Global configuration for the build
#[derive(Clone, Debug)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub multi_progress_indicator: indicatif::MultiProgress,

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

pub fn reqwest_client_from_auth_storage(auth_file: Option<PathBuf>) -> ClientWithMiddleware {
    let auth_storage = get_auth_store(auth_file);
    reqwest_middleware::ClientBuilder::new(
        reqwest::Client::builder()
            .no_gzip()
            .build()
            .expect("failed to create client"),
    )
    .with_arc(Arc::new(AuthenticationMiddleware::new(auth_storage)))
    .build()
}

impl Default for Configuration {
    fn default() -> Self {
        let auth_storage = AuthenticationStorage::default();
        Self {
            multi_progress_indicator: indicatif::MultiProgress::new(),
            client: reqwest_middleware::ClientBuilder::new(
                reqwest::Client::builder()
                    .no_gzip()
                    .build()
                    .expect("failed to create client"),
            )
            .with_arc(Arc::new(AuthenticationMiddleware::new(auth_storage)))
            .build(),
            no_clean: false,
            no_test: false,
            use_zstd: true,
            use_bz2: true,
        }
    }
}
