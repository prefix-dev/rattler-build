//! macOS specific environment variables
use rattler_conda_types::Platform;
use std::{collections::HashMap, path::Path};

use crate::unix;

/// Get default env vars for macOS
pub fn default_env_vars_target(
    prefix: &Path,
    target_platform: &Platform,
) -> HashMap<String, Option<String>> {
    let mut vars = unix::env::default_env_vars_target(prefix);
    let arch = target_platform
        .arch()
        .expect("arch missing on target_platform")
        .as_str();
    let (osx_arch, deployment_target) = match arch {
        "x86" => ("i386", "10.9"),
        "arm64" => ("arm64", "11.0"),
        _ => ("x86_64", "10.9"),
    };

    vars.insert("OSX_ARCH".to_string(), Some(osx_arch.to_string()));
    vars.insert(
        "MACOSX_DEPLOYMENT_TARGET".to_string(),
        Some(deployment_target.to_string()),
    );
    vars
}

pub fn default_env_vars_build(build_platform: &Platform) -> HashMap<String, Option<String>> {
    let mut vars = HashMap::<String, Option<String>>::new();
    let arch = build_platform
        .arch()
        .expect("arch missing on build_platform")
        .as_str();
    let build = match arch {
        "x86" => "i386-apple-darwin13.4.0",
        "arm64" => "arm64-apple-darwin20.0.0",
        _ => "x86_64-apple-darwin13.4.0",
    };

    vars.insert("BUILD".to_string(), Some(build.to_string()));
    vars
}
