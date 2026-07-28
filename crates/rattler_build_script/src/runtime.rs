//! The runtime environment rattler-build itself executes in.
//!
//! [`RuntimeEnv`] bundles the process environment variables (including `PATH`)
//! and the current [`Platform`]. Threading a `RuntimeEnv` explicitly through
//! script generation and execution, instead of reading process globals
//! (`std::env::var`, `Platform::current`), keeps behavior deterministic and lets
//! tests inject a synthetic environment without mutating global process state.

use std::collections::HashMap;

use rattler_conda_types::Platform;

#[derive(Debug, Clone)]
struct EnvironmentVariables {
    values: HashMap<String, String>,
    case_insensitive_keys: Option<HashMap<String, String>>,
}

impl EnvironmentVariables {
    fn new(case_insensitive: bool) -> Self {
        Self {
            values: HashMap::new(),
            case_insensitive_keys: case_insensitive.then(HashMap::new),
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        let name = self
            .case_insensitive_keys
            .as_ref()
            .and_then(|keys| keys.get(&name.to_ascii_lowercase()))
            .map_or(name, String::as_str);
        self.values.get(name).map(String::as_str)
    }

    fn insert(&mut self, name: String, value: String) {
        if let Some(keys) = &mut self.case_insensitive_keys {
            let normalized = name.to_ascii_lowercase();
            if let Some(original) = keys.get(&normalized) {
                self.values.insert(original.clone(), value);
                return;
            }
            keys.insert(normalized, name.clone());
        }
        self.values.insert(name, value);
    }

    fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// The environment rattler-build is running in: the process environment
/// variables (including `PATH`) and the platform.
#[derive(Debug, Clone)]
pub struct RuntimeEnv {
    env: EnvironmentVariables,
    process_platform: Platform,
}

impl RuntimeEnv {
    /// Captures the real process environment variables and the current platform.
    pub fn current() -> Self {
        let current_platform = Platform::current();
        let mut runtime_env = Self {
            env: EnvironmentVariables::new(current_platform.is_windows()),
            process_platform: current_platform,
        };
        for (name, value) in std::env::vars() {
            runtime_env.env.insert(name, value)
        }
        runtime_env
    }

    /// Creates a runtime environment with an empty variable set and the given
    /// platform. Intended for tests; combine with [`RuntimeEnv::with_var`] to
    /// inject the variables a test needs.
    pub fn for_test(platform: Platform) -> Self {
        Self {
            env: EnvironmentVariables::new(platform.is_windows()),
            process_platform: platform,
        }
    }

    /// The platform of the rattler-build process.
    pub fn process_platform(&self) -> Platform {
        self.process_platform
    }

    /// Looks up an environment variable by name.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.env.get(name)
    }

    /// The value of `PATH`, or an empty string when it is unset.
    pub fn path(&self) -> &str {
        self.var("PATH").unwrap_or_default()
    }

    /// The executable file suffix for this platform (`.exe` on Windows, empty
    /// elsewhere), keyed off the platform rather than the one rattler-build was
    /// compiled for (unlike [`std::env::consts::EXE_SUFFIX`]).
    pub(crate) fn exe_suffix(&self) -> &'static str {
        if self.process_platform.is_windows() {
            ".exe"
        } else {
            ""
        }
    }

    /// Iterates over all environment variables as `(name, value)` pairs.
    pub fn vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.env.iter()
    }

    /// Returns a copy with `name` set to `value` (builder style, for tests).
    #[must_use]
    pub fn with_var(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Returns a copy with the given rattler-build process platform (for tests).
    #[must_use]
    pub fn with_process_platform(mut self, platform: Platform) -> Self {
        self.process_platform = platform;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_suffix_follows_the_platform() {
        assert_eq!(RuntimeEnv::for_test(Platform::Win64).exe_suffix(), ".exe");
        assert_eq!(RuntimeEnv::for_test(Platform::Linux64).exe_suffix(), "");
        assert_eq!(RuntimeEnv::for_test(Platform::OsxArm64).exe_suffix(), "");
    }

    #[test]
    fn env_var_case_insensitive_on_windows() {
        let runtime_env = RuntimeEnv::for_test(Platform::Win64);
        assert_eq!(
            runtime_env
                .with_var("TEST_CASE", "1")
                .var("test_case")
                .unwrap(),
            "1"
        );
    }
}
