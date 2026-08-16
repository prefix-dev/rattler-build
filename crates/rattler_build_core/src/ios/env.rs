//! iOS specific environment variables
use rattler_build_script::RuntimeEnv;
use rattler_conda_types::Platform;
use std::{collections::HashMap, path::Path};

use crate::unix;

/// Minimum iOS version targeted by conda packages. This matches the minimum
/// supported by CPython's iOS support (PEP 730).
const DEFAULT_DEPLOYMENT_TARGET: &str = "13.0";

/// Returns the clang target triple for an iOS platform, e.g. `arm64-apple-ios`
/// for a device or `arm64-apple-ios-simulator` for the simulator.
fn target_triple(target_platform: &Platform) -> Option<String> {
    let arch = target_platform.arch()?.as_str().to_string();
    match target_platform {
        Platform::IosArm64 => Some(format!("{arch}-apple-ios")),
        Platform::IosSimulatorArm64 | Platform::IosSimulator64 => {
            Some(format!("{arch}-apple-ios-simulator"))
        }
        _ => None,
    }
}

/// Get default env vars for iOS
pub fn default_env_vars_target(
    prefix: &Path,
    target_platform: &Platform,
    runtime: &RuntimeEnv,
) -> HashMap<String, Option<String>> {
    let mut vars = unix::env::default_env_vars_target(prefix, runtime);

    if let Some(arch) = target_platform.arch() {
        vars.insert("IOS_ARCH".to_string(), Some(arch.as_str().to_string()));
    }

    // Xcode's standard knob for the minimum supported OS version. Recipes and
    // variant configs can override it just like `MACOSX_DEPLOYMENT_TARGET`.
    vars.insert(
        "IPHONEOS_DEPLOYMENT_TARGET".to_string(),
        Some(DEFAULT_DEPLOYMENT_TARGET.to_string()),
    );

    if let Some(triple) = target_triple(target_platform) {
        vars.insert("HOST".to_string(), Some(triple));
    }

    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_distinguish_device_and_simulator() {
        assert_eq!(
            target_triple(&Platform::IosArm64).as_deref(),
            Some("arm64-apple-ios")
        );
        assert_eq!(
            target_triple(&Platform::IosSimulatorArm64).as_deref(),
            Some("arm64-apple-ios-simulator")
        );
        assert_eq!(
            target_triple(&Platform::IosSimulator64).as_deref(),
            Some("x86_64-apple-ios-simulator")
        );
        assert_eq!(target_triple(&Platform::OsxArm64), None);
    }

    #[test]
    fn target_vars_include_unix_and_ios_specific_vars() {
        let vars = default_env_vars_target(
            Path::new("/some/prefix"),
            &Platform::IosArm64,
            &RuntimeEnv::for_test(Platform::Linux64),
        );
        // inherited from the generic unix vars
        assert!(vars.contains_key("PKG_CONFIG_PATH"));
        assert!(vars.contains_key("CMAKE_GENERATOR"));
        // iOS specific
        assert_eq!(vars.get("IOS_ARCH"), Some(&Some("arm64".to_string())));
        assert_eq!(
            vars.get("IPHONEOS_DEPLOYMENT_TARGET"),
            Some(&Some(DEFAULT_DEPLOYMENT_TARGET.to_string()))
        );
        assert_eq!(vars.get("HOST"), Some(&Some("arm64-apple-ios".to_string())));
    }
}
