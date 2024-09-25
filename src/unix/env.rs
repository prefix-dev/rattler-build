use crate::env_vars::EnvVars;
use std::path::Path;

pub fn default_env_vars(prefix: &Path) -> EnvVars {
    let mut vars = EnvVars::new();
    vars.insert(
        "HOME".into(),
        std::env::var("HOME").unwrap_or_else(|_| "UNKNOWN".to_string()),
    );
    vars.insert(
        "PKG_CONFIG_PATH".into(),
        prefix.join("lib/pkgconfig").to_string_lossy().to_string(),
    );
    vars.insert("CMAKE_GENERATOR".into(), "Unix Makefiles".to_string());
    vars.insert(
        "SSL_CERT_FILE".into(),
        std::env::var("SSL_CERT_FILE").unwrap_or_default(),
    );
    vars
}
