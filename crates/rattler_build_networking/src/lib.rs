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
}

impl Default for BaseClientBuilder {
    fn default() -> Self {
        Self {
            user_agent: None,
            timeout_secs: 5 * 60, // 5 minutes default
            insecure_hosts: None,
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

    /// Build the BaseClient with the configured settings
    pub fn build(self) -> BaseClient {
        let user_agent = self
            .user_agent
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        let mut client = BaseClient::new_with_config(user_agent, self.timeout_secs);

        if let Some(hosts) = self.insecure_hosts {
            client.allow_insecure_host = Some(hosts);
        }

        client
    }
}
