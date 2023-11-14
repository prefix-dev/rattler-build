//! Configuration for the rattler-build tool
//! This is useful when using rattler-build as a library

use std::path::PathBuf;

use rattler_networking::AuthenticatedClient;

/// Global configuration for the build
#[derive(Clone)]
pub struct Configuration {
    /// If set to a value, a progress bar will be shown
    pub multi_progress_indicator: indicatif::MultiProgress,

    /// The authenticated reqwest download client to use
    pub client: AuthenticatedClient,

    /// Set this to true if you want to keep the build folder after the build is done
    pub no_clean: bool,

    /// Whether to skip the test phase
    pub no_test: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            multi_progress_indicator: indicatif::MultiProgress::new(),
            client: AuthenticatedClient::from_client(
                reqwest::Client::builder()
                    .no_gzip()
                    .build()
                    .expect("failed to create client"),
                rattler_networking::AuthenticationStorage::new(
                    "rattler",
                    &PathBuf::from("~/.rattler"),
                ),
            ),
            no_clean: false,
            no_test: false,
        }
    }
}
