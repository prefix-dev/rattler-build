//! Networking utilities for Rattler-Build, providing shared HTTP client functionality
//! with retry middleware.

use std::sync::{Arc, LazyLock};

use rattler_networking::{
    AuthenticationStorage, LazyClient,
    authentication_storage::{AuthenticationStorageError, backends::file::FileStorage},
};
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use url::Url;

/// Default user agent if none is provided
const DEFAULT_USER_AGENT: &str =
    concat!("rattler-build-networking", "/", env!("CARGO_PKG_VERSION"));

/// A client that can handle both secure and insecure connections.
#[derive(Clone)]
pub struct BaseClient {
    client: LazyClient,
    dangerous_client: LazyClient,
    /// List of hosts for which SSL verification should be skipped
    allow_insecure_host: Option<Vec<String>>,
}

impl BaseClient {
    /// Create a new BaseClient with default settings
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Create a BaseClient from existing clients
    pub fn new_from_clients(
        client: ClientWithMiddleware,
        dangerous_client: ClientWithMiddleware,
        allow_insecure_host: Option<Vec<String>>,
    ) -> Self {
        Self {
            client: client.into(),
            dangerous_client: dangerous_client.into(),
            allow_insecure_host,
        }
    }

    /// Create a builder for configuring the BaseClient
    pub fn builder() -> BaseClientBuilder {
        BaseClientBuilder::default()
    }

    /// Create a new BaseClient with a custom timeout in seconds
    ///
    /// Deprecated: Use `BaseClient::builder().timeout(secs).build()` instead
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self::builder().timeout(timeout_secs).build()
    }

    /// Create a new BaseClient with insecure hosts
    ///
    /// Deprecated: Use `BaseClient::builder().insecure_hosts(hosts).build()` instead
    pub fn with_insecure_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allow_insecure_host = Some(hosts);
        self
    }

    /// Get the default lazy client (with SSL verification enabled).
    ///
    /// Returns the [`LazyClient`] without forcing it to initialize, so
    /// passing the result to types that themselves accept a `LazyClient`
    /// (such as `Gateway::with_client` or
    /// `Installer::with_download_client`) keeps the underlying reqwest
    /// client construction deferred.
    pub fn get_client(&self) -> &LazyClient {
        &self.client
    }

    /// Selects the appropriate lazy client based on the host's trustworthiness.
    pub fn for_host(&self, url: &Url) -> &LazyClient {
        if self.disable_ssl(url) {
            &self.dangerous_client
        } else {
            &self.client
        }
    }

    /// Returns true if SSL verification should be disabled for the given URL
    fn disable_ssl(&self, url: &Url) -> bool {
        if let Some(hosts) = &self.allow_insecure_host
            && let Some(host) = url.host_str()
        {
            return hosts.iter().any(|h| h == host);
        }
        false
    }
}

impl Default for BaseClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for configuring a BaseClient
pub struct BaseClientBuilder {
    user_agent: Option<String>,
    timeout_secs: u64,
    insecure_hosts: Option<Vec<String>>,
    auth_file: Option<FileStorage>,
    #[cfg(feature = "s3")]
    s3_config:
        Option<std::collections::HashMap<String, rattler_networking::s3_middleware::S3Config>>,
    mirror_config: Option<
        std::collections::HashMap<url::Url, Vec<rattler_networking::mirror_middleware::Mirror>>,
    >,
}

impl Default for BaseClientBuilder {
    fn default() -> Self {
        Self {
            user_agent: None,
            timeout_secs: 5 * 60, // 5 minutes default
            insecure_hosts: None,
            auth_file: None,
            #[cfg(feature = "s3")]
            s3_config: None,
            mirror_config: None,
        }
    }
}

impl BaseClientBuilder {
    /// Set a custom user agent string
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Set the timeout in seconds
    pub fn timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set hosts for which SSL verification should be disabled
    pub fn insecure_hosts(mut self, hosts: Vec<String>) -> Self {
        self.insecure_hosts = Some(hosts);
        self
    }

    /// Add a credential file to consult in addition to the default
    /// authentication backends.
    ///
    /// The file is loaded eagerly so a bad path errors here.
    pub fn with_auth_file(
        mut self,
        auth_file: Option<std::path::PathBuf>,
    ) -> Result<Self, AuthenticationStorageError> {
        self.auth_file = auth_file.map(FileStorage::from_path).transpose()?;
        Ok(self)
    }

    /// Set S3 s3 configuration
    #[cfg(feature = "s3")]
    pub fn with_s3(
        mut self,
        s3_config: std::collections::HashMap<String, rattler_networking::s3_middleware::S3Config>,
    ) -> Self {
        self.s3_config = Some(s3_config);
        self
    }

    /// Set mirror middleware configuration
    pub fn with_mirrors(
        mut self,
        mirror_config: std::collections::HashMap<
            url::Url,
            Vec<rattler_networking::mirror_middleware::Mirror>,
        >,
    ) -> Self {
        self.mirror_config = Some(mirror_config);
        self
    }

    /// Build the BaseClient with the configured settings
    pub fn build(self) -> BaseClient {
        let user_agent = self
            .user_agent
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let timeout_secs = self.timeout_secs;
        let insecure_hosts = self.insecure_hosts;
        let auth_file = self.auth_file;
        let mirror_config = self.mirror_config;
        #[cfg(feature = "s3")]
        let s3_config = self.s3_config;

        // Lazily build the authentication storage exactly once and share it
        // between the secure and dangerous clients via Arc<LazyLock>.
        let shared_auth: Arc<LazyLock<AuthenticationStorage, _>> =
            Arc::new(LazyLock::new(Box::new(move || {
                let mut store = AuthenticationStorage::from_env_and_defaults()
                    .expect("Failed to load authentication storage");
                if let Some(file_storage) = auth_file {
                    store.add_backend(Arc::from(file_storage));
                }
                store
            })
                as Box<dyn FnOnce() -> AuthenticationStorage + Send + Sync>));

        let ua_for_secure = user_agent.clone();
        let auth_for_secure = shared_auth.clone();
        let mirror_for_secure = mirror_config.clone();
        #[cfg(feature = "s3")]
        let s3_for_secure = s3_config.clone();
        let client = LazyClient::new(move || {
            build_middleware_client(
                &ua_for_secure,
                timeout_secs,
                false,
                (*auth_for_secure).clone(),
                mirror_for_secure,
                #[cfg(feature = "s3")]
                s3_for_secure,
            )
        });

        let dangerous_client = LazyClient::new(move || {
            build_middleware_client(
                &user_agent,
                timeout_secs,
                true,
                (*shared_auth).clone(),
                mirror_config,
                #[cfg(feature = "s3")]
                s3_config,
            )
        });

        BaseClient {
            client,
            dangerous_client,
            allow_insecure_host: insecure_hosts,
        }
    }
}

fn reqwest_client(user_agent: &str, timeout_secs: u64, dangerous: bool) -> reqwest::Client {
    let builder = reqwest::Client::builder()
        .no_gzip()
        .pool_max_idle_per_host(20)
        .user_agent(user_agent)
        .referer(false)
        .read_timeout(std::time::Duration::from_secs(timeout_secs));

    #[cfg(any(feature = "native-tls", feature = "rustls-tls"))]
    let builder = if dangerous {
        builder.danger_accept_invalid_certs(true)
    } else {
        builder
    };
    #[cfg(not(any(feature = "native-tls", feature = "rustls-tls")))]
    let _ = dangerous;

    builder.build().expect("failed to create reqwest client")
}

fn retry_middleware() -> RetryTransientMiddleware<ExponentialBackoff> {
    RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(3),
    )
}

fn build_middleware_client(
    user_agent: &str,
    timeout_secs: u64,
    dangerous: bool,
    auth_storage: AuthenticationStorage,
    mirror_config: Option<
        std::collections::HashMap<url::Url, Vec<rattler_networking::mirror_middleware::Mirror>>,
    >,
    #[cfg(feature = "s3")] s3_config: Option<
        std::collections::HashMap<String, rattler_networking::s3_middleware::S3Config>,
    >,
) -> ClientWithMiddleware {
    use rattler_networking::{AuthenticationMiddleware, mirror_middleware::MirrorMiddleware};

    let mut builder =
        reqwest_middleware::ClientBuilder::new(reqwest_client(user_agent, timeout_secs, dangerous));
    if let Some(cfg) = mirror_config {
        builder = builder.with(MirrorMiddleware::from_map(cfg));
    }
    #[cfg(feature = "s3")]
    if let Some(cfg) = s3_config {
        builder = builder.with(rattler_networking::s3_middleware::S3Middleware::new(
            cfg,
            auth_storage.clone(),
        ));
    }
    builder = builder.with(AuthenticationMiddleware::from_auth_storage(auth_storage));
    builder.with(retry_middleware()).build()
}
