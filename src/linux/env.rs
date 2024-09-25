//! Linux specific environment variables
use std::env::consts::ARCH;
use std::path::Path;

use rattler_conda_types::Platform;

use crate::env_vars::EnvVars;
use crate::unix;

/// Get default env vars for Linux
pub fn default_env_vars(prefix: &Path, target_platform: &Platform) -> EnvVars {
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
    vars.insert("CFLAGS".into(), std::env::var("CFLAGS").unwrap_or_default());
    vars.insert(
        "CXXFLAGS".into(),
        std::env::var("CXXFLAGS").unwrap_or_default(),
    );
    vars.insert(
        "LDFLAGS".into(),
        std::env::var("LDFLAGS").unwrap_or_default(),
    );
    vars.insert(
        "QEMU_LD_PREFIX".into(),
        std::env::var("QEMU_LD_PREFIX").unwrap_or_default(),
    );
    vars.insert(
        "QEMU_UNAME".into(),
        std::env::var("QEMU_UNAME").unwrap_or_default(),
    );
    vars.insert(
        "DEJAGNU".into(),
        std::env::var("DEJAGNU").unwrap_or_default(),
    );
    vars.insert(
        "DISPLAY".into(),
        std::env::var("DISPLAY").unwrap_or_default(),
    );
    vars.insert(
        "LD_RUN_PATH".into(),
        std::env::var("LD_RUN_PATH")
            .unwrap_or_else(|_| prefix.join("lib").to_string_lossy().to_string()),
    );
    vars.insert(
        "BUILD".into(),
        format!("{}-conda_{}-linux-gnu", build_arch, build_distro),
    );

    vars
}
