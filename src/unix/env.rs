use std::{collections::HashMap, path::Path};

pub fn default_env_vars(prefix: &Path) -> HashMap<String, Option<String>> {
    let mut vars = HashMap::new();
    vars.insert(
        "HOME".to_string(),
        Some(std::env::var("HOME").unwrap_or_else(|_| "UNKNOWN".to_string())),
    );
    vars.insert(
        "PKG_CONFIG_PATH".to_string(),
        Some(prefix.join("lib/pkgconfig").to_string_lossy().to_string()),
    );
    vars.insert(
        "CMAKE_GENERATOR".to_string(),
        Some("Unix Makefiles".to_string()),
    );
    vars.insert(
        "SSL_CERT_FILE".to_string(),
        std::env::var("SSL_CERT_FILE").ok(),
    );
    vars
}
