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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// Temporarily sets an environment variable for the duration of the test.
    fn with_env_var<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let original = std::env::var(key).ok();
        match value {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        f();
        match original {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[test]
    #[serial]
    fn pkconfig_path_uses_prefix() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let env_vars = default_env_vars(tmp.path());
        let expected = tmp
            .path()
            .join("lib/pkgconfig")
            .to_string_lossy()
            .to_string();
        assert_eq!(env_vars.get("PKG_CONFIG_PATH"), Some(&Some(expected)));
    }

    #[test]
    #[serial]
    fn home_var_fallbacks_to_unknown() {
        with_env_var("HOME", None, || {
            let env_vars = default_env_vars(Path::new("/some/prefix"));
            assert_eq!(env_vars.get("HOME"), Some(&Some("UNKNOWN".to_string())));
        });
    }

    #[test]
    #[serial]
    fn home_var_preserved() {
        with_env_var("HOME", Some("/custom/home"), || {
            let env_vars = default_env_vars(Path::new("/some/prefix"));
            assert_eq!(
                env_vars.get("HOME"),
                Some(&Some("/custom/home".to_string()))
            );
        });
    }
}
