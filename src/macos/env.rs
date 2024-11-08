//! macOS specific environment variables
use rattler_conda_types::Platform;
use std::{collections::HashMap, path::Path};

use crate::unix;

/// Get default env vars for macOS
pub fn default_env_vars(
    prefix: &Path,
    target_platform: &Platform,
) -> HashMap<String, Option<String>> {
    let mut vars = unix::env::default_env_vars(prefix);
    let t_string = target_platform.to_string();
    let arch = t_string.split('-').collect::<Vec<&str>>()[1];
    let (osx_arch, deployment_target, build) = match arch {
        "32" => ("i386", "10.9", "i386-apple-darwin13.4.0"),
        "arm64" => ("arm64", "11.0", "arm64-apple-darwin20.0.0"),
        _ => ("x86_64", "10.9", "x86_64-apple-darwin13.4.0"),
    };

    vars.insert("OSX_ARCH".to_string(), Some(osx_arch.to_string()));
    vars.insert(
        "MACOSX_DEPLOYMENT_TARGET".to_string(),
        Some(deployment_target.to_string()),
    );
    vars.insert("BUILD".to_string(), Some(build.to_string()));
    vars
}
