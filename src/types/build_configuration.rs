//! All the metadata that makes up a recipe file
use std::collections::BTreeMap;

use rattler_conda_types::{ChannelUrl, PackageName, Platform};
use rattler_solve::{ChannelPriority, SolveStrategy};
use serde::{Deserialize, Serialize};

use crate::{
    hash::HashInfo,
    normalized_key::NormalizedKey,
    recipe::{jinja::SelectorConfig, variable::Variable},
    script::SandboxConfiguration,
    types::{
        Debug, Directories, PackageIdentifier, PackagingSettings, PlatformWithVirtualPackages,
    },
};

/// Default value for store recipe for backwards compatibility
fn default_true() -> bool {
    true
}
/// The configuration for a build of a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfiguration {
    /// The target platform for the build
    pub target_platform: Platform,
    /// The host platform (usually target platform, but for `noarch` it's the
    /// build platform)
    pub host_platform: PlatformWithVirtualPackages,
    /// The build platform (the platform that the build is running on)
    pub build_platform: PlatformWithVirtualPackages,
    /// The selected variant for this build
    pub variant: BTreeMap<NormalizedKey, Variable>,
    /// THe computed hash of the variant
    pub hash: HashInfo,
    /// The directories for the build (work, source, build, host, ...)
    pub directories: Directories,
    /// The channels to use when resolving environments
    pub channels: Vec<ChannelUrl>,
    /// The channel priority that is used to resolve dependencies
    pub channel_priority: ChannelPriority,
    /// The solve strategy to use when resolving dependencies
    pub solve_strategy: SolveStrategy,
    /// The timestamp to use for the build
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// All subpackages coming from this output or other outputs from the same
    /// recipe
    pub subpackages: BTreeMap<PackageName, PackageIdentifier>,
    /// Package format (.tar.bz2 or .conda)
    pub packaging_settings: PackagingSettings,
    /// Whether to store the recipe and build instructions in the final package
    /// or not
    #[serde(skip_serializing, default = "default_true")]
    pub store_recipe: bool,
    /// Whether to set additional environment variables to force colors in the
    /// build script or not
    #[serde(skip_serializing, default = "default_true")]
    pub force_colors: bool,

    /// The configuration for the sandbox
    #[serde(skip_serializing, default)]
    pub sandbox_config: Option<SandboxConfiguration>,
    /// Whether to enable debug output in build scripts
    #[serde(skip_serializing, default)]
    pub debug: Debug,
    /// Exclude packages newer than this date from the solver
    #[serde(skip_serializing, default)]
    pub exclude_newer: Option<chrono::DateTime<chrono::Utc>>,
}

impl BuildConfiguration {
    /// true if the build is cross-compiling
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform.platform
    }

    /// Retrieve the sandbox configuration for this output
    pub fn sandbox_config(&self) -> Option<&SandboxConfiguration> {
        self.sandbox_config.as_ref()
    }

    /// Construct a `SelectorConfig` from the given `BuildConfiguration`
    pub fn selector_config(&self) -> SelectorConfig {
        SelectorConfig {
            target_platform: self.target_platform,
            host_platform: self.host_platform.platform,
            build_platform: self.build_platform.platform,
            variant: self.variant.clone(),
            hash: Some(self.hash.clone()),
            experimental: false,
            allow_undefined: false,
            recipe_path: None,
        }
    }
}
