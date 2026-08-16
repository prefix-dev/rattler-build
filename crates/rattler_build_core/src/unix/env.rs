use std::{collections::HashMap, path::Path};

use rattler_build_script::RuntimeEnv;

pub fn default_env_vars_target(
    prefix: &Path,
    runtime: &RuntimeEnv,
) -> HashMap<String, Option<String>> {
    let mut vars = HashMap::new();
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
        runtime.var("SSL_CERT_FILE").map(str::to_owned),
    );
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkconfig_path_uses_prefix() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let env_vars = default_env_vars_target(
            tmp.path(),
            &RuntimeEnv::for_test(rattler_conda_types::Platform::Linux64),
        );
        let expected = tmp
            .path()
            .join("lib/pkgconfig")
            .to_string_lossy()
            .to_string();
        assert_eq!(env_vars.get("PKG_CONFIG_PATH"), Some(&Some(expected)));
    }

    #[test]
    fn home_not_set_by_default_env_vars() {
        let env_vars = default_env_vars_target(
            Path::new("/some/prefix"),
            &RuntimeEnv::for_test(rattler_conda_types::Platform::Linux64),
        );
        assert_eq!(env_vars.get("HOME"), None);
    }
}
