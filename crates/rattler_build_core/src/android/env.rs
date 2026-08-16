//! Android specific environment variables
use rattler_build_script::RuntimeEnv;
use rattler_conda_types::Platform;
use std::{collections::HashMap, path::Path};

use crate::unix;

/// Minimum Android API level targeted by conda packages. This matches the
/// minimum supported by CPython's Android support (PEP 738).
const DEFAULT_API_LEVEL: &str = "24";

/// Returns the NDK ABI name for an Android platform, e.g. `arm64-v8a`. These are
/// the names used by the NDK's CMake toolchain (`ANDROID_ABI`).
fn android_abi(target_platform: &Platform) -> Option<&'static str> {
    match target_platform {
        Platform::AndroidAarch64 => Some("arm64-v8a"),
        Platform::AndroidArmV7a => Some("armeabi-v7a"),
        Platform::Android64 => Some("x86_64"),
        Platform::Android32 => Some("x86"),
        _ => None,
    }
}

/// Returns the clang target triple for an Android platform. Note that the 32-bit
/// arm target uses the `androideabi` suffix.
fn target_triple(target_platform: &Platform) -> Option<&'static str> {
    match target_platform {
        Platform::AndroidAarch64 => Some("aarch64-linux-android"),
        Platform::AndroidArmV7a => Some("armv7a-linux-androideabi"),
        Platform::Android64 => Some("x86_64-linux-android"),
        Platform::Android32 => Some("i686-linux-android"),
        _ => None,
    }
}

/// Get default env vars for Android
pub fn default_env_vars_target(
    prefix: &Path,
    target_platform: &Platform,
    runtime: &RuntimeEnv,
) -> HashMap<String, Option<String>> {
    let mut vars = unix::env::default_env_vars_target(prefix, runtime);

    if let Some(abi) = android_abi(target_platform) {
        vars.insert("ANDROID_ABI".to_string(), Some(abi.to_string()));
    }

    // The NDK's minimum supported API level. Recipes and variant configs can
    // override it just like `MACOSX_DEPLOYMENT_TARGET` on macOS.
    vars.insert(
        "ANDROID_API_LEVEL".to_string(),
        Some(DEFAULT_API_LEVEL.to_string()),
    );

    if let Some(triple) = target_triple(target_platform) {
        vars.insert("HOST".to_string(), Some(triple.to_string()));
    }

    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abis_match_ndk_names() {
        assert_eq!(android_abi(&Platform::AndroidAarch64), Some("arm64-v8a"));
        assert_eq!(android_abi(&Platform::AndroidArmV7a), Some("armeabi-v7a"));
        assert_eq!(android_abi(&Platform::Android64), Some("x86_64"));
        assert_eq!(android_abi(&Platform::Android32), Some("x86"));
        assert_eq!(android_abi(&Platform::Linux64), None);
    }

    #[test]
    fn armv7a_triple_uses_androideabi_suffix() {
        assert_eq!(
            target_triple(&Platform::AndroidArmV7a),
            Some("armv7a-linux-androideabi")
        );
        assert_eq!(
            target_triple(&Platform::AndroidAarch64),
            Some("aarch64-linux-android")
        );
    }

    #[test]
    fn target_vars_include_unix_and_android_specific_vars() {
        let vars = default_env_vars_target(
            Path::new("/some/prefix"),
            &Platform::AndroidAarch64,
            &RuntimeEnv::for_test(Platform::Linux64),
        );
        // inherited from the generic unix vars
        assert!(vars.contains_key("PKG_CONFIG_PATH"));
        assert!(vars.contains_key("CMAKE_GENERATOR"));
        // Android specific
        assert_eq!(
            vars.get("ANDROID_ABI"),
            Some(&Some("arm64-v8a".to_string()))
        );
        assert_eq!(
            vars.get("ANDROID_API_LEVEL"),
            Some(&Some(DEFAULT_API_LEVEL.to_string()))
        );
        assert_eq!(
            vars.get("HOST"),
            Some(&Some("aarch64-linux-android".to_string()))
        );
    }
}
