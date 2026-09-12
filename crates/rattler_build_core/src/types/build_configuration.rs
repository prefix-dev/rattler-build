//! All the metadata that makes up a recipe file
use std::collections::BTreeMap;

use rattler_build_jinja::{JinjaConfig, Variable};
use rattler_build_recipe::stage1::HashInfo;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{Channel, ChannelUrl, PackageName, Platform, RepodataRevision};
use rattler_solve::{ChannelPriority, ExcludeNewer, SolveStrategy};
use serde::{Deserialize, Serialize};

use crate::types::{
    Directories, PackageIdentifier, PackagingSettings, PlatformWithVirtualPackages,
};

use rattler_build_script::{EnvironmentIsolation, SandboxConfiguration};

/// Default value for store recipe for backwards compatibility
fn default_true() -> bool {
    true
}

// Build metadata omits the cutoff, but older callers may supply a scalar when
// deserializing a build configuration.
fn deserialize_exclude_newer<'de, D>(deserializer: D) -> Result<Option<ExcludeNewer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<jiff::Timestamp>::deserialize(deserializer).map(|cutoff| cutoff.map(Into::into))
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
    /// Exclude packages according to global, channel, and package cutoff dates.
    #[serde(
        skip_serializing,
        default,
        deserialize_with = "deserialize_exclude_newer"
    )]
    pub exclude_newer: Option<ExcludeNewer>,
    /// Repodata revision to target when writing package metadata.
    #[serde(skip_serializing, default)]
    pub repodata_revision: RepodataRevision,
}

impl BuildConfiguration {
    /// Apply the cutoff while allowing packages from this build's output channel.
    /// Explicit package cutoffs still take precedence over the output channel.
    pub fn exclude_newer_with_build_outputs(&self) -> Option<ExcludeNewer> {
        self.exclude_newer.clone().map(|policy| {
            match Channel::try_from_directory(&self.directories.output_dir) {
                Ok(channel) => policy
                    .with_channel_cutoff(channel.base_url.url().as_str(), jiff::Timestamp::MAX),
                // Rendering can resolve dependencies before creating an output
                // directory. No output channel is available in that case.
                Err(_) => policy,
            }
        })
    }

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
}

#[cfg(test)]
mod tests {
    use rattler_conda_types::utils::TimestampMs;

    use super::*;

    fn build_configuration() -> BuildConfiguration {
        let output: crate::metadata::Output = serde_yaml::from_str(include_str!(
            "../../../../test-data/rendered_recipes/rich_recipe.yaml"
        ))
        .unwrap();
        output.build_configuration
    }

    #[test]
    fn exclude_newer_exempts_only_the_build_output_channel() {
        let output_dir = tempfile::tempdir().unwrap();
        let dependency_dir = tempfile::tempdir().unwrap();
        let output_channel = Channel::try_from_directory(output_dir.path()).unwrap();
        let dependency_channel = Channel::try_from_directory(dependency_dir.path()).unwrap();
        let package: PackageName = "fresh-output".parse().unwrap();
        let cutoff: jiff::Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
        let timestamp =
            TimestampMs::from("2025-01-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap());
        let mut config = build_configuration();
        config.directories.output_dir = output_dir.path().to_path_buf();
        config.exclude_newer = Some(cutoff.into());

        let policy = config.exclude_newer_with_build_outputs().unwrap();
        assert!(!policy.is_excluded(
            &package,
            Some(output_channel.base_url.url().as_str()),
            Some(&timestamp),
        ));
        assert!(policy.is_excluded(
            &package,
            Some(dependency_channel.base_url.url().as_str()),
            Some(&timestamp),
        ));
        assert!(policy.is_excluded(
            &package,
            Some("https://example.com/channel/"),
            Some(&timestamp)
        ));
        assert!(policy.is_excluded(&package, Some(output_channel.base_url.url().as_str()), None));

        config.exclude_newer = Some(
            ExcludeNewer::from_datetime(cutoff)
                .with_package_cutoff(package.clone(), cutoff)
                .with_include_unknown_timestamp(true),
        );
        let policy = config.exclude_newer_with_build_outputs().unwrap();
        assert!(policy.is_excluded(
            &package,
            Some(output_channel.base_url.url().as_str()),
            Some(&timestamp),
        ));
        assert!(!policy.is_excluded(&package, Some(output_channel.base_url.url().as_str()), None));
    }

    #[test]
    fn exclude_newer_is_omitted_from_metadata_and_accepts_legacy_scalar() {
        let mut config = build_configuration();
        let cutoff: jiff::Timestamp = "2020-01-01T00:00:00Z".parse().unwrap();
        config.exclude_newer = Some(
            ExcludeNewer::from_datetime(cutoff)
                .with_channel_cutoff("https://example.com/channel/", jiff::Timestamp::MAX),
        );
        let mut serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("exclude_newer").is_none());
        let parsed: BuildConfiguration = serde_json::from_value(serialized.clone()).unwrap();
        assert!(parsed.exclude_newer.is_none());

        serialized["exclude_newer"] = serde_json::json!("2020-01-01T00:00:00Z");
        let parsed: BuildConfiguration = serde_json::from_value(serialized).unwrap();
        assert_eq!(
            parsed.exclude_newer,
            Some(ExcludeNewer::from_datetime(cutoff))
        );
    }
}
