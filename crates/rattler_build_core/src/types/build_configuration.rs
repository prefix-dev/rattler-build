//! All the metadata that makes up a recipe file
use std::collections::BTreeMap;

use rattler_build_jinja::{JinjaConfig, Variable};
use rattler_build_recipe::stage1::HashInfo;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{ChannelUrl, PackageName, Platform, RepodataRevision};
use rattler_solve::{ChannelPriority, SolveStrategy};
use serde::{Deserialize, Serialize};

use crate::types::{
    Directories, PackageIdentifier, PackagingSettings, PlatformWithVirtualPackages,
};

use rattler_build_script::{EnvironmentIsolation, SandboxConfiguration};

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
    /// The computed hash of the variant
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
    pub timestamp: jiff::Timestamp,
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

    /// Whether experimental features are enabled for this build invocation.
    #[serde(skip_serializing, default)]
    pub experimental: bool,

    /// The environment isolation mode for build scripts
    #[serde(skip_serializing, default)]
    pub env_isolation: EnvironmentIsolation,

    /// The configuration for the sandbox
    #[serde(skip_serializing, default)]
    pub sandbox_config: Option<SandboxConfiguration>,
    /// Exclude packages newer than this date from the solver
    #[serde(skip_serializing, default)]
    pub exclude_newer: Option<jiff::Timestamp>,
    /// Repodata revision to target when writing package metadata.
    #[serde(skip_serializing, default)]
    pub repodata_revision: RepodataRevision,

    /// All variant keys and their possible values from the variant
    /// configuration (not just the ones the recipe actually uses). When
    /// `--pass-all-variants-as-env` is passed, the keys among these
    /// that only have a single possible value are exported as environment
    /// variables to the build script in addition to the variant keys the
    /// recipe actually uses. Empty unless that flag is set.
    #[serde(skip_serializing, default)]
    pub all_variants: BTreeMap<NormalizedKey, Vec<Variable>>,
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

    /// Construct a `JinjaConfig` from the given `BuildConfiguration`
    pub fn selector_config(&self) -> JinjaConfig {
        JinjaConfig {
            target_platform: self.target_platform,
            host_platform: self.host_platform.platform,
            build_platform: self.build_platform.platform,
            variant: self.variant.clone(),
            experimental: self.experimental,
            undefined_behavior: rattler_build_jinja::UndefinedBehavior::Lenient,
            recipe_path: None,
        }
    }

    /// Variant keys from `all_variants` that only have a single possible
    /// value (empty unless `--pass-all-variants-as-env` was passed).
    pub fn single_value_variants(&self) -> BTreeMap<NormalizedKey, Variable> {
        self.all_variants
            .iter()
            .filter_map(|(key, values)| match values.as_slice() {
                [value] => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    /// The variant keys the recipe actually uses, plus (only when
    /// `--pass-all-variants-as-env` was passed) any additional variant keys
    /// that only have a single possible value. Used for provenance data
    /// (e.g. the `variant_config.yaml` stored in the package) rather than
    /// hashing or build-string computation.
    pub fn variant_with_single_value_extras(&self) -> BTreeMap<NormalizedKey, Variable> {
        let mut variant = self.variant.clone();
        for (key, value) in self.single_value_variants() {
            variant.entry(key).or_insert(value);
        }
        variant
    }
}
