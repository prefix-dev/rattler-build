//! Common types used throughout rattler-build
//! All the metadata that makes up a recipe file
use std::{iter, path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use rattler_conda_types::{
    Channel, ChannelUrl, GenericVirtualPackage, PackageName, Platform, VersionWithSource,
    compression_level::CompressionLevel,
    package::{ArchiveType, PathsJson},
};
use rattler_index::{IndexFsConfig, index_fs};
use rattler_repodata_gateway::{CacheClearMode, SubdirSelection};
use rattler_virtual_packages::{
    DetectVirtualPackageError, VirtualPackageOverrides, VirtualPackages,
};
use serde::{Deserialize, Deserializer, Serialize};

use crate::tool_configuration;

mod build_configuration;
mod build_output;
mod directories;

pub use build_configuration::BuildConfiguration;
pub use build_output::BuildOutput as Output;
pub use directories::Directories;

/// Settings when creating the package (compression etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagingSettings {
    /// The archive type, currently supported are `tar.bz2` and `conda`
    pub archive_type: ArchiveType,
    /// The compression level from 1-9 or -7-22 for `tar.bz2` and `conda`
    /// archives
    pub compression_level: i32,
}

impl PackagingSettings {
    /// Create a new `PackagingSettings` from the command line arguments
    /// and the selected archive type.
    pub fn from_args(archive_type: ArchiveType, compression_level: CompressionLevel) -> Self {
        let compression_level: i32 = match archive_type {
            ArchiveType::TarBz2 => compression_level.to_bzip2_level().unwrap() as i32,
            ArchiveType::Conda => compression_level.to_zstd_level().unwrap(),
        };

        Self {
            archive_type,
            compression_level,
        }
    }
}

/// Defines both a platform and the virtual packages that describe the
/// capabilities of the platform.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformWithVirtualPackages {
    /// The platform
    pub platform: Platform,

    /// The virtual packages for the platform
    pub virtual_packages: Vec<GenericVirtualPackage>,
}

impl PlatformWithVirtualPackages {
    /// Returns the current platform and the virtual packages available on the
    /// current system.
    pub fn detect(overrides: &VirtualPackageOverrides) -> Result<Self, DetectVirtualPackageError> {
        let platform = Platform::current();
        Self::detect_for_platform(platform, overrides)
    }

    /// Detect the virtual packages for the given platform, filling in defaults where appropriate
    pub fn detect_for_platform(
        platform: Platform,
        overrides: &VirtualPackageOverrides,
    ) -> Result<Self, DetectVirtualPackageError> {
        let virtual_packages = VirtualPackages::detect_for_platform(platform, overrides)?
            .into_generic_virtual_packages()
            .collect();
        Ok(Self {
            platform,
            virtual_packages,
        })
    }
}

impl<'de> Deserialize<'de> for PlatformWithVirtualPackages {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        pub struct Object {
            pub platform: Platform,
            pub virtual_packages: Vec<GenericVirtualPackage>,
        }

        serde_untagged::UntaggedEnumVisitor::new()
            .string(|s| {
                Ok(Self {
                    platform: Platform::from_str(s).map_err(serde::de::Error::custom)?,
                    virtual_packages: vec![],
                })
            })
            .map(|m| {
                let object: Object = m.deserialize()?;
                Ok(Self {
                    platform: object.platform,
                    virtual_packages: object.virtual_packages,
                })
            })
            .deserialize(deserializer)
    }
}

/// A newtype wrapper around a boolean indicating whether debug output is enabled
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Debug(bool);

impl Debug {
    /// Create a new Debug instance
    pub fn new(debug: bool) -> Self {
        Self(debug)
    }

    /// Returns true if debug output is enabled
    pub fn is_enabled(&self) -> bool {
        self.0
    }
}

/// A package identifier
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIdentifier {
    /// The name of the package
    pub name: PackageName,
    /// The version of the package
    pub version: VersionWithSource,
    /// The build string of the package
    pub build_string: String,
}

/// The summary of a build
#[derive(Debug, Clone, Default)]
pub struct BuildSummary {
    /// The start time of the build
    pub build_start: Option<DateTime<Utc>>,
    /// The end time of the build
    pub build_end: Option<DateTime<Utc>>,

    /// The path to the artifact
    pub artifact: Option<PathBuf>,
    /// Any warnings that were recorded during the build
    pub warnings: Vec<String>,
    /// The paths that are packaged in the artifact
    pub paths: Option<PathsJson>,
    ///  Whether the build was successful or not
    pub failed: bool,
}

/// Builds the channel list and reindexes the output channel.
pub async fn build_reindexed_channels(
    build_configuration: &BuildConfiguration,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<Vec<ChannelUrl>, std::io::Error> {
    let output_dir = &build_configuration.directories.output_dir;
    let output_channel = Channel::from_directory(output_dir);

    // Clear the repodata gateway of any cached values for the output channel.
    tool_configuration.repodata_gateway.clear_repodata_cache(
        &output_channel,
        SubdirSelection::Some(
            [build_configuration.target_platform]
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
        // In memory is enough because this is a "file" channel
        CacheClearMode::InMemoryOnly,
    )?;

    let index_config = IndexFsConfig {
        channel: output_dir.clone(),
        target_platform: Some(build_configuration.target_platform),
        repodata_patch: None,
        write_zst: false,
        write_shards: false,
        force: false,
        max_parallel: num_cpus::get_physical(),
        multi_progress: None,
    };

    // Reindex the output channel from the files on disk
    index_fs(index_config)
        .await
        .map_err(std::io::Error::other)?;

    Ok(iter::once(output_channel.base_url)
        .chain(build_configuration.channels.iter().cloned())
        .collect())
}
