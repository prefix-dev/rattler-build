//! Networking utilities for rattler-build
//!
//! This crate provides shared HTTP client functionality used across rattler-build components.

use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use url::Url;

/// Default user agent if none is provided
const DEFAULT_USER_AGENT: &str =
    concat!("rattler-build-networking", "/", env!("CARGO_PKG_VERSION"));

/// A client that can handle both secure and insecure connections
#[derive(Clone)]
pub struct BaseClient {
    /// The standard client with SSL verification enabled
    client: ClientWithMiddleware,
    /// The dangerous client with SSL verification disabled
    dangerous_client: ClientWithMiddleware,
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
            client,
            dangerous_client,
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

    fn new_with_config(user_agent: String, timeout_secs: u64) -> Self {
        let client = reqwest_middleware::ClientBuilder::new(
            reqwest::Client::builder()
                .no_gzip()
                .pool_max_idle_per_host(20)
                .user_agent(&user_agent)
                .read_timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .expect("failed to create client"),
        )
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .build();

        let dangerous_client = reqwest_middleware::ClientBuilder::new(
            reqwest::Client::builder()
                .no_gzip()
                .pool_max_idle_per_host(20)
                .user_agent(&user_agent)
                .read_timeout(std::time::Duration::from_secs(timeout_secs))
                .danger_accept_invalid_certs(true)
                .build()
                .expect("failed to create dangerous client"),
        )
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .build();

        Self {
            client,
            dangerous_client,
            allow_insecure_host: None,
        }
    }

    /// Create a new BaseClient with insecure hosts
    ///
    /// Deprecated: Use `BaseClient::builder().insecure_hosts(hosts).build()` instead
    pub fn with_insecure_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allow_insecure_host = Some(hosts);
        self
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

impl Default for BaseClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for configuring a BaseClient
#[derive(Debug, Clone)]
pub struct BaseClientBuilder {
    user_agent: Option<String>,
    timeout_secs: u64,
    insecure_hosts: Option<Vec<String>>,
    #[cfg(feature = "middleware")]
    auth_storage: Option<rattler_networking::AuthenticationStorage>,
    #[cfg(feature = "middleware")]
    s3_config:
        Option<std::collections::HashMap<String, rattler_networking::s3_middleware::S3Config>>,
    #[cfg(feature = "middleware")]
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
            #[cfg(feature = "middleware")]
            auth_storage: None,
            #[cfg(feature = "middleware")]
            s3_config: None,
            #[cfg(feature = "middleware")]
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

    /// Set authentication storage for authenticated requests
    #[cfg(feature = "middleware")]
    pub fn with_authentication(
        mut self,
        auth_storage: rattler_networking::AuthenticationStorage,
    ) -> Self {
        self.auth_storage = Some(auth_storage);
        self
    }

    /// Set S3 s3 configuration
    #[cfg(feature = "middleware")]
    pub fn with_s3(
        mut self,
        s3_config: std::collections::HashMap<String, rattler_networking::s3_middleware::S3Config>,
    ) -> Self {
        self.s3_config = Some(s3_config);
        self
    }

    /// Set mirror middleware configuration
    #[cfg(feature = "middleware")]
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
        #[cfg(feature = "middleware")]
        {
            let has_middleware = self.auth_storage.is_some()
                || self.s3_config.is_some()
                || self.mirror_config.is_some();
            if has_middleware {
                return self.build_with_middleware();
            }
        }

        let user_agent = self
            .user_agent
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        let mut client = BaseClient::new_with_config(user_agent, self.timeout_secs);

        if let Some(hosts) = self.insecure_hosts {
            client.allow_insecure_host = Some(hosts);
        }

        client
    }

    #[cfg(feature = "middleware")]
    fn build_with_middleware(self) -> BaseClient {
        use rattler_networking::{
            AuthenticationMiddleware, mirror_middleware::MirrorMiddleware,
            s3_middleware::S3Middleware,
        };
        use std::sync::Arc;

        let user_agent = self
            .user_agent
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        let timeout_secs = self.timeout_secs;
        let insecure_hosts = self.insecure_hosts;

        let common_settings = |builder: reqwest::ClientBuilder| -> reqwest::ClientBuilder {
            builder
                .no_gzip()
                .pool_max_idle_per_host(20)
                .user_agent(&user_agent)
                .read_timeout(std::time::Duration::from_secs(timeout_secs))
        };

        let auth_storage = self.auth_storage.unwrap_or_else(|| {
            rattler_networking::AuthenticationStorage::from_env_and_defaults()
                .expect("Failed to load authentication storage")
        });

        // Prepare middlewares once and reuse the same instances (via Arc) for both clients.
        let mirror_mw = self
            .mirror_config
            .map(|cfg| Arc::new(MirrorMiddleware::from_map(cfg)));
        let s3_mw = self
            .s3_config
            .map(|cfg| Arc::new(S3Middleware::new(cfg, auth_storage.clone())));
        let auth_mw = Arc::new(AuthenticationMiddleware::from_auth_storage(
            auth_storage.clone(),
        ));
        let retry_mw = Arc::new(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ));

        // Build the secure client with the exact middleware chain in a fixed order.
        let mut client_builder = reqwest_middleware::ClientBuilder::new(
            common_settings(reqwest::Client::builder())
                .build()
                .expect("failed to create client"),
        );
        if let Some(mw) = &mirror_mw {
            client_builder = client_builder.with_arc(mw.clone());
        }
        if let Some(mw) = &s3_mw {
            client_builder = client_builder.with_arc(mw.clone());
        }
        client_builder = client_builder.with_arc(auth_mw.clone());
        client_builder = client_builder.with_arc(retry_mw.clone());
        let client = client_builder.build();

        // Build dangerous client (insecure)
        let mut dangerous_client_builder = reqwest_middleware::ClientBuilder::new(
            common_settings(reqwest::Client::builder())
                .danger_accept_invalid_certs(true)
                .build()
                .expect("failed to create dangerous client"),
        );

        // Apply the exact same middleware chain and order to the dangerous client.
        if let Some(mw) = &mirror_mw {
            dangerous_client_builder = dangerous_client_builder.with_arc(mw.clone());
        }
        if let Some(mw) = &s3_mw {
            dangerous_client_builder = dangerous_client_builder.with_arc(mw.clone());
        }
        dangerous_client_builder = dangerous_client_builder.with_arc(auth_mw);
        dangerous_client_builder = dangerous_client_builder.with_arc(retry_mw);

        let dangerous_client = dangerous_client_builder.build();

        BaseClient {
            client,
            dangerous_client,
            allow_insecure_host: insecure_hosts,
        }
    }
}
