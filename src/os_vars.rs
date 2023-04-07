use std::path::Path;
use std::{collections::HashMap, env};

use rattler_conda_types::Platform;

use crate::linux;
use crate::macos;
use crate::unix;
use crate::windows;

/// Returns a map of environment variables that are used in the build process.
/// Also adds platform-specific variables.
pub fn os_vars(prefix: &Path, platform: &Platform) -> HashMap<String, String> {
    let mut vars = HashMap::<String, String>::new();

    vars.insert(
        "CPU_COUNT".to_string(),
        env::var("CPU_COUNT").unwrap_or_else(|_| num_cpus::get().to_string()),
    );
    vars.insert("LANG".to_string(), env::var("LANG").unwrap_or_default());
    vars.insert("LC_ALL".to_string(), env::var("LC_ALL").unwrap_or_default());
    vars.insert(
        "MAKEFLAGS".to_string(),
        env::var("MAKEFLAGS").unwrap_or_default(),
    );

    let shlib_ext = if platform.is_windows() {
        ".dll".to_string()
    } else if platform.is_osx() {
        ".dylib".to_string()
    } else if platform.is_linux() {
        ".so".to_string()
    } else {
        ".not_implemented".to_string()
    };

    vars.insert("SHLIB_EXT".to_string(), shlib_ext);
    vars.insert("PATH".to_string(), env::var("PATH").unwrap_or_default());

    if cfg!(target_family = "windows") {
        vars.extend(windows::env::default_env_vars(prefix, platform).into_iter());
    } else if cfg!(target_family = "unix") {
        vars.extend(unix::env::default_env_vars(prefix).into_iter());
    }

    if platform.is_osx() {
        vars.extend(macos::env::default_env_vars(prefix, platform).into_iter());
    } else if platform.is_linux() {
        vars.extend(linux::env::default_env_vars(prefix).into_iter());
    }

    vars
}
