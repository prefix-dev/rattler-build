//! Linux specific environment variables
use std::env::consts::ARCH;
use std::{collections::HashMap, path::Path};

use rattler_build_script::{EnvironmentIsolation, RuntimeEnv};
use rattler_conda_types::Platform;

use crate::unix;

/// Get default env vars for Linux
pub fn default_env_vars_target(
    prefix: &Path,
    target_platform: &Platform,
    env_isolation: EnvironmentIsolation,
    runtime: &RuntimeEnv,
) -> HashMap<String, Option<String>> {
    let mut vars = unix::env::default_env_vars_target(prefix, runtime);

    let build_distro = match target_platform {
        Platform::Linux32 | Platform::Linux64 => "cos6",
        _ => "cos7",
    };

    let build_arch = ARCH;

    // There is also QEMU_SET_ENV, but that needs to be
    // filtered so it only contains the result of `linux_vars`
    // which, before this change was empty, and after it only
    // contains other QEMU env vars.
    if matches!(
        env_isolation,
        EnvironmentIsolation::CondaBuild | EnvironmentIsolation::None
    ) {
        vars.insert(
            "CFLAGS".to_string(),
            runtime.var("CFLAGS").map(str::to_owned),
        );
        vars.insert(
            "CXXFLAGS".to_string(),
            runtime.var("CXXFLAGS").map(str::to_owned),
        );
        vars.insert(
            "LDFLAGS".to_string(),
            runtime.var("LDFLAGS").map(str::to_owned),
        );
    }
    vars.insert(
        "QEMU_LD_PREFIX".to_string(),
        runtime.var("QEMU_LD_PREFIX").map(str::to_owned),
    );
    vars.insert(
        "QEMU_UNAME".to_string(),
        runtime.var("QEMU_UNAME").map(str::to_owned),
    );
    vars.insert(
        "DEJAGNU".to_string(),
        runtime.var("DEJAGNU").map(str::to_owned),
    );
    vars.insert(
        "DISPLAY".to_string(),
        runtime.var("DISPLAY").map(str::to_owned),
    );
    vars.insert(
        "LD_RUN_PATH".to_string(),
        runtime
            .var("LD_RUN_PATH")
            .map(str::to_owned)
            .or_else(|| Some(prefix.join("lib").to_string_lossy().to_string())),
    );
    vars.insert(
        "BUILD".to_string(),
        Some(format!("{}-conda_{}-linux-gnu", build_arch, build_distro)),
    );

    vars
}

pub fn default_env_vars_build(build_platform: &Platform) -> HashMap<String, Option<String>> {
    let mut vars = HashMap::<String, Option<String>>::new();
    let build_distro = match build_platform {
        Platform::Linux32 | Platform::Linux64 => "cos6",
        _ => "cos7",
    };

    let build_arch = match build_platform {
        Platform::Linux32 => "i686",
        Platform::Linux64 => "x86_64",
        Platform::LinuxPpc64le => "powerpc64le",
        _ => build_platform
            .arch()
            .expect("arch for build_platform missing")
            .as_str(),
    };

    vars.insert(
        "BUILD".to_string(),
        Some(format!("{}-conda_{}-linux-gnu", build_arch, build_distro)),
    );

    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn build_and_ld_run_path_defaults() {
        let tmp_prefix = tempfile::tempdir().unwrap();
        let runtime = RuntimeEnv::for_test(Platform::Linux64);

        let vars = default_env_vars_target(
            tmp_prefix.path(),
            &Platform::Linux64,
            EnvironmentIsolation::Strict,
            &runtime,
        );
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
    fn ld_run_path_env_preserved() {
        let tmp_prefix = tempfile::tempdir().unwrap();
        let runtime =
            RuntimeEnv::for_test(Platform::Linux64).with_var("LD_RUN_PATH", "/custom/lib");

        let vars = default_env_vars_target(
            tmp_prefix.path(),
            &Platform::Linux64,
            EnvironmentIsolation::Strict,
            &runtime,
        );
        assert_eq!(
            vars.get("LD_RUN_PATH"),
            Some(&Some("/custom/lib".to_string()))
        );
    }

    #[test]
    fn host_compiler_flags_not_forwarded() {
        let tmp_prefix = tempfile::tempdir().unwrap();
        let vars = default_env_vars_target(
            tmp_prefix.path(),
            &Platform::Linux64,
            EnvironmentIsolation::Strict,
            &RuntimeEnv::for_test(Platform::Linux64),
        );
        assert_eq!(vars.get("CFLAGS"), None);
        assert_eq!(vars.get("CXXFLAGS"), None);
        assert_eq!(vars.get("LDFLAGS"), None);
    }
}
