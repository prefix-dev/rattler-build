//! Linux specific environment variables
use std::env::consts::ARCH;
use std::{collections::HashMap, path::Path};

use rattler_conda_types::Platform;

use crate::unix;

/// Get default env vars for Linux
pub fn default_env_vars(
    prefix: &Path,
    target_platform: &Platform,
) -> HashMap<String, Option<String>> {
    let mut vars = unix::env::default_env_vars(prefix);

    let build_distro = match target_platform {
        Platform::Linux32 | Platform::Linux64 => "cos6",
        _ => "cos7",
    };

    let build_arch = ARCH;

    // There is also QEMU_SET_ENV, but that needs to be
    // filtered so it only contains the result of `linux_vars`
    // which, before this change was empty, and after it only
    // contains other QEMU env vars.
    vars.insert("CFLAGS".to_string(), std::env::var("CFLAGS").ok());
    vars.insert("CXXFLAGS".to_string(), std::env::var("CXXFLAGS").ok());
    vars.insert("LDFLAGS".to_string(), std::env::var("LDFLAGS").ok());
    vars.insert(
        "QEMU_LD_PREFIX".to_string(),
        std::env::var("QEMU_LD_PREFIX").ok(),
    );
    vars.insert("QEMU_UNAME".to_string(), std::env::var("QEMU_UNAME").ok());
    vars.insert("DEJAGNU".to_string(), std::env::var("DEJAGNU").ok());
    vars.insert("DISPLAY".to_string(), std::env::var("DISPLAY").ok());
    vars.insert(
        "LD_RUN_PATH".to_string(),
        std::env::var("LD_RUN_PATH")
            .ok()
            .or_else(|| Some(prefix.join("lib").to_string_lossy().to_string())),
    );
    vars.insert(
        "BUILD".to_string(),
        Some(format!("{}-conda_{}-linux-gnu", build_arch, build_distro)),
    );

    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn build_and_ld_run_path_defaults() {
        let tmp_prefix = tempfile::tempdir().unwrap();
        unsafe { std::env::remove_var("LD_RUN_PATH") };

        let vars = default_env_vars(tmp_prefix.path(), &Platform::Linux64);
        let build_val = vars
            .get("BUILD")
            .and_then(|o| o.as_ref())
            .expect("BUILD missing");
        assert!(build_val.contains(std::env::consts::ARCH));
        assert!(build_val.contains("cos"));
        assert_eq!(
            vars.get("CMAKE_GENERATOR"),
            Some(&Some("Unix Makefiles".to_string()))
        );

        let expected_ld = tmp_prefix.path().join("lib").to_string_lossy().to_string();
        assert_eq!(vars.get("LD_RUN_PATH"), Some(&Some(expected_ld)));
    }

    #[test]
    #[serial]
    fn ld_run_path_env_preserved() {
        let tmp_prefix = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("LD_RUN_PATH", "/custom/lib") };

        let vars = default_env_vars(tmp_prefix.path(), &Platform::Linux64);
        assert_eq!(
            vars.get("LD_RUN_PATH"),
            Some(&Some("/custom/lib".to_string()))
        );

        unsafe { std::env::remove_var("LD_RUN_PATH") };
    }
}
