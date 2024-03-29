//! Linux specific environment variables
use std::env::consts::ARCH;
use std::{collections::HashMap, path::Path};

use rattler_conda_types::Platform;

use crate::unix;

/// Get default env vars for Linux
pub fn default_env_vars(prefix: &Path, target_platform: &Platform) -> HashMap<String, String> {
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
    vars.insert(
        "CFLAGS".to_string(),
        std::env::var("CFLAGS").unwrap_or_default(),
    );
    vars.insert(
        "CXXFLAGS".to_string(),
        std::env::var("CXXFLAGS").unwrap_or_default(),
    );
    vars.insert(
        "LDFLAGS".to_string(),
        std::env::var("LDFLAGS").unwrap_or_default(),
    );
    vars.insert(
        "QEMU_LD_PREFIX".to_string(),
        std::env::var("QEMU_LD_PREFIX").unwrap_or_default(),
    );
    vars.insert(
        "QEMU_UNAME".to_string(),
        std::env::var("QEMU_UNAME").unwrap_or_default(),
    );
    vars.insert(
        "DEJAGNU".to_string(),
        std::env::var("DEJAGNU").unwrap_or_default(),
    );
    vars.insert(
        "DISPLAY".to_string(),
        std::env::var("DISPLAY").unwrap_or_default(),
    );
    vars.insert(
        "LD_RUN_PATH".to_string(),
        std::env::var("LD_RUN_PATH")
            .unwrap_or_else(|_| prefix.join("lib").to_string_lossy().to_string()),
    );
    vars.insert(
        "BUILD".to_string(),
        format!("{}-conda_{}-linux-gnu", build_arch, build_distro),
    );

    vars
}
