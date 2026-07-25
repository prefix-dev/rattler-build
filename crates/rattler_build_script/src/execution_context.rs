//! Platform-aware prefix and process context for script execution.

use std::path::{Path, PathBuf};

use rattler_conda_types::Platform;

use crate::RuntimeEnv;

/// A conda prefix together with the platform of the environment it contains.
#[derive(Debug, Clone)]
pub struct PrefixWithPlatform {
    path: PathBuf,
    platform: Platform,
}

impl PrefixWithPlatform {
    /// Creates a prefix execution descriptor.
    pub fn new(path: impl Into<PathBuf>, platform: Platform) -> Self {
        Self {
            path: path.into(),
            platform,
        }
    }

    /// The prefix path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The platform of the environment installed in this prefix.
    pub fn platform(&self) -> Platform {
        self.platform
    }
}

/// Whether build and host environments have distinct or shared prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixLayout {
    /// Build and host environments are separate prefixes.
    Separate,
    /// Build and host environments share one prefix and must be activated once.
    Shared,
}

/// Process and prefix information needed to execute a build or test script.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    runtime: RuntimeEnv,
    build: PrefixWithPlatform,
    host: PrefixWithPlatform,
    layout: PrefixLayout,
}

impl ExecutionContext {
    /// Creates a context with separate build and host prefixes.
    pub fn separate(
        runtime: RuntimeEnv,
        build_path: impl Into<PathBuf>,
        build_platform: Platform,
        host_path: impl Into<PathBuf>,
        host_platform: Platform,
    ) -> Self {
        Self {
            runtime,
            build: PrefixWithPlatform::new(build_path, build_platform),
            host: PrefixWithPlatform::new(host_path, host_platform),
            layout: PrefixLayout::Separate,
        }
    }

    /// Creates a context whose build and host environments share one prefix.
    pub fn shared(
        runtime: RuntimeEnv,
        path: impl Into<PathBuf>,
        build_platform: Platform,
        host_platform: Platform,
    ) -> Self {
        let path = path.into();
        Self {
            runtime,
            build: PrefixWithPlatform::new(path.clone(), build_platform),
            host: PrefixWithPlatform::new(path, host_platform),
            layout: PrefixLayout::Shared,
        }
    }

    /// The environment and architecture of the rattler-build process.
    pub fn runtime(&self) -> &RuntimeEnv {
        &self.runtime
    }

    /// Returns a copy with a different rattler-build process runtime.
    #[must_use]
    pub fn with_runtime(mut self, runtime: RuntimeEnv) -> Self {
        self.runtime = runtime;
        self
    }

    /// The prefix that supplies build tools and the platform they execute on.
    pub fn build(&self) -> &PrefixWithPlatform {
        &self.build
    }

    /// The prefix that supplies host dependencies and the platform it represents.
    pub fn host(&self) -> &PrefixWithPlatform {
        &self.host
    }

    /// Whether build and host prefixes are separate or shared.
    pub fn layout(&self) -> PrefixLayout {
        self.layout
    }

    /// The processor architecture to expose in a Windows child process.
    ///
    /// Returns a value only when the Windows runner must switch to x86 or
    /// between x64 and ARM64 with `start /machine`; otherwise the child
    /// inherits its normal process environment.
    pub fn windows_processor_architecture(&self) -> Option<&'static str> {
        crate::native_runner::windows_machine_transition(
            self.runtime.process_platform(),
            self.build.platform(),
        )
        .map(crate::native_runner::WindowsMachine::processor_architecture)
    }

    /// The value to set for `PROCESSOR_ARCHITEW6432` in a switched child.
    ///
    /// `Some(None)` clears the marker for a 64-bit child. `Some(value)` sets
    /// the detected native architecture for an x86 WOW64 child. `None` keeps
    /// the inherited value when no transition is needed or native detection is
    /// unavailable.
    pub fn windows_processor_architecture_w6432(&self) -> Option<Option<&'static str>> {
        let machine = crate::native_runner::windows_machine_transition(
            self.runtime.process_platform(),
            self.build.platform(),
        )?;

        match machine {
            crate::native_runner::WindowsMachine::X86 => {
                crate::native_runner::native_windows_machine()
                    .map(crate::native_runner::WindowsMachine::wow64_processor_architecture)
            }
            crate::native_runner::WindowsMachine::Amd64
            | crate::native_runner::WindowsMachine::Arm64 => Some(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_context_retains_both_prefixes() {
        let context = ExecutionContext::separate(
            RuntimeEnv::for_test(Platform::Win64),
            "build",
            Platform::Win64,
            "host",
            Platform::WinArm64,
        );

        assert_eq!(context.layout(), PrefixLayout::Separate);
        assert_eq!(context.build().path(), Path::new("build"));
        assert_eq!(context.build().platform(), Platform::Win64);
        assert_eq!(context.host().path(), Path::new("host"));
        assert_eq!(context.host().platform(), Platform::WinArm64);
    }

    #[test]
    fn shared_context_uses_one_path_with_both_platforms() {
        let context = ExecutionContext::shared(
            RuntimeEnv::for_test(Platform::Win64),
            "prefix",
            Platform::Win64,
            Platform::WinArm64,
        );

        assert_eq!(context.layout(), PrefixLayout::Shared);
        assert_eq!(context.build().path(), Path::new("prefix"));
        assert_eq!(context.host().path(), Path::new("prefix"));
        assert_eq!(context.build().platform(), Platform::Win64);
        assert_eq!(context.host().platform(), Platform::WinArm64);
    }
}
