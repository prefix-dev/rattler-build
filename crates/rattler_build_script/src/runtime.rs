//! The runtime environment rattler-build itself executes in.
//!
//! [`RuntimeEnv`] bundles the process environment variables (including `PATH`)
//! and the current [`Platform`]. Threading a `RuntimeEnv` explicitly through
//! script generation and execution, instead of reading process globals
//! (`std::env::var`, `Platform::current`), keeps behavior deterministic and lets
//! tests inject a synthetic environment without mutating global process state.

use std::collections::HashMap;

use rattler_conda_types::Platform;

/// The environment rattler-build is running in: the process environment
/// variables (including `PATH`) and the platform.
#[derive(Debug, Clone)]
pub struct RuntimeEnv {
    env: HashMap<String, String>,
    process_platform: Platform,
}

impl RuntimeEnv {
    /// Captures the real process environment variables and the current platform.
    pub fn current() -> Self {
        Self {
            env: std::env::vars().collect(),
            process_platform: Platform::current(),
        }
    }

    /// Creates a runtime environment with an empty variable set and the given
    /// platform. Intended for tests; combine with [`RuntimeEnv::with_var`] to
    /// inject the variables a test needs.
    pub fn for_test(platform: Platform) -> Self {
        Self {
            env: HashMap::new(),
            process_platform: platform,
        }
    }

    /// The platform of the rattler-build process.
    pub fn process_platform(&self) -> Platform {
        self.process_platform
    }

    /// Looks up an environment variable by name.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.env.get(name).map(String::as_str)
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
        self.env.iter().map(|(k, v)| (k.as_str(), v.as_str()))
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
}
