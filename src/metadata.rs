//! All the metadata that makes up a recipe file
use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Utc};
use dunce::canonicalize;
use fs_err as fs;
use indicatif::HumanBytes;
use rattler_conda_types::{
    package::{ArchiveType, PathType, PathsEntry, PathsJson},
    PackageName, Platform, RepoDataRecord,
};
use rattler_index::index;
use rattler_package_streaming::write::CompressionLevel;
use serde::{Deserialize, Serialize};

use crate::{
    console_utils::github_integration_enabled,
    hash::HashInfo,
    recipe::parser::{Recipe, Source},
    render::resolved_dependencies::FinalizedDependencies,
    system_tools::SystemTools,
};
/// A Git revision
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitRev(String);

impl FromStr for GitRev {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(GitRev(s.to_string()))
    }
}

impl Default for GitRev {
    fn default() -> Self {
        Self(String::from("HEAD"))
    }
}

impl Display for GitRev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Directories used during the build process
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Directories {
    /// The directory where the recipe is located
    #[serde(skip)]
    pub recipe_dir: PathBuf,
    /// The host prefix is the directory where host dependencies are installed
    /// Exposed as `$PREFIX` (or `%PREFIX%` on Windows) in the build script
    pub host_prefix: PathBuf,
    /// The build prefix is the directory where build dependencies are installed
    /// Exposed as `$BUILD_PREFIX` (or `%BUILD_PREFIX%` on Windows) in the build script
    pub build_prefix: PathBuf,
    /// The work directory is the directory where the source code is copied to
    pub work_dir: PathBuf,
    /// The parent directory of host, build and work directories
    pub build_dir: PathBuf,
    /// The output directory or local channel directory
    #[serde(skip)]
    pub output_dir: PathBuf,
}

fn get_build_dir(
    output_dir: &Path,
    name: &str,
    no_build_id: bool,
    timestamp: &DateTime<Utc>,
) -> Result<PathBuf, std::io::Error> {
    let since_the_epoch = timestamp.timestamp();

    let dirname = if no_build_id {
        format!("rattler-build_{}", name)
    } else {
        format!("rattler-build_{}_{:?}", name, since_the_epoch)
    };
    Ok(output_dir.join("bld").join(dirname))
}

impl Directories {
    /// Create all directories needed for the building of a package
    pub fn setup(
        name: &str,
        recipe_path: &Path,
        output_dir: &Path,
        no_build_id: bool,
        timestamp: &DateTime<Utc>,
    ) -> Result<Directories, std::io::Error> {
        if !output_dir.exists() {
            fs::create_dir(output_dir)?;
        }
        let output_dir = canonicalize(output_dir)?;

        let build_dir = get_build_dir(&output_dir, name, no_build_id, timestamp)
            .expect("Could not create build directory");
        let recipe_dir = recipe_path
            .parent()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "Parent directory not found")
            })?
            .to_path_buf();

        let host_prefix = if cfg!(target_os = "windows") {
            build_dir.join("h_env")
        } else {
            let placeholder_template = "_placehold";
            let mut placeholder = String::new();
            let placeholder_length: usize = 255;

            while placeholder.len() < placeholder_length {
                placeholder.push_str(placeholder_template);
            }

            let placeholder = placeholder
                [0..placeholder_length - build_dir.join("host_env").as_os_str().len()]
                .to_string();

            build_dir.join(format!("host_env{}", placeholder))
        };

        let directories = Directories {
            build_dir: build_dir.clone(),
            build_prefix: build_dir.join("build_env"),
            host_prefix,
            work_dir: build_dir.join("work"),
            recipe_dir,
            output_dir,
        };

        Ok(directories)
    }

    /// Creates the build directory.
    pub fn create_build_dir(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(self.build_dir.join("work"))?;
        Ok(())
    }

    /// create all directories
    pub fn recreate_directories(&self) -> Result<(), std::io::Error> {
        if self.build_dir.exists() {
            fs::remove_dir_all(&self.build_dir)?;
        }

        if !self.output_dir.exists() {
            fs::create_dir(&self.output_dir)?;
        }

        fs::create_dir_all(&self.build_dir)?;
        fs::create_dir_all(&self.work_dir)?;
        fs::create_dir_all(&self.build_prefix)?;
        fs::create_dir_all(&self.host_prefix)?;

        Ok(())
    }
}

/// Default value for store recipe for backwards compatiblity
fn default_true() -> bool {
    true
}

/// Settings when creating the package (compression etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagingSettings {
    /// The archive type, currently supported are `tar.bz2` and `conda`
    pub archive_type: ArchiveType,
    /// The compression level from 1-9 or -7-22 for `tar.bz2` and `conda` archives
    pub compression_level: i32,
    /// How many threads to use for compression (only relevant for `.conda` archives)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression_threads: Option<u32>,
}

impl PackagingSettings {
    /// Create a new `PackagingSettings` from the command line arguments
    /// and the selected archive type.
    pub fn from_args(
        archive_type: ArchiveType,
        compression_level: CompressionLevel,
        compression_threads: Option<u32>,
    ) -> Self {
        let compression_level: i32 = match archive_type {
            ArchiveType::TarBz2 => compression_level.to_bzip2_level().unwrap().level() as i32,
            ArchiveType::Conda => compression_level.to_zstd_level().unwrap(),
        };

        if compression_threads.is_some()
            && compression_threads.unwrap() > 1
            && archive_type != ArchiveType::Conda
        {
            tracing::warn!("Multi-threaded compression is only supported for conda archives");
        }

        let compression_threads = if archive_type == ArchiveType::Conda {
            Some(compression_threads.unwrap_or(1))
        } else {
            None
        };

        Self {
            archive_type,
            compression_level,
            compression_threads,
        }
    }
}

/// The configuration for a build of a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfiguration {
    /// The target platform for the build
    pub target_platform: Platform,
    /// The host platform (usually target platform, but for `noarch` it's the build platform)
    pub host_platform: Platform,
    /// The build platform (the platform that the build is running on)
    pub build_platform: Platform,
    /// The selected variant for this build
    pub variant: BTreeMap<String, String>,
    /// THe computed hash of the variant
    pub hash: HashInfo,
    /// The directories for the build (work, source, build, host, ...)
    pub directories: Directories,
    /// The channels to use when resolving environments
    pub channels: Vec<String>,
    /// The timestamp to use for the build
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// All subpackages coming from this output or other outputs from the same recipe
    pub subpackages: BTreeMap<PackageName, PackageIdentifier>,
    /// Package format (.tar.bz2 or .conda)
    pub packaging_settings: PackagingSettings,
    /// Whether to store the recipe and build instructions in the final package or not
    #[serde(skip_serializing, default = "default_true")]
    pub store_recipe: bool,
    /// Wether to set additional environment variables to force colors in the build script or not
    #[serde(skip_serializing, default = "default_true")]
    pub force_colors: bool,
}

impl BuildConfiguration {
    /// true if the build is cross-compiling
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform
    }
}

/// A package identifier
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIdentifier {
    /// The name of the package
    pub name: PackageName,
    /// The version of the package
    pub version: String,
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

/// A output. This is the central element that is passed to the `run_build` function
/// and fully specifies all the options and settings to run the build.
#[derive(Clone, Serialize, Deserialize)]
pub struct Output {
    /// The rendered recipe that is used to build this output
    pub recipe: Recipe,
    /// The build configuration for this output (e.g. target_platform, channels, and other settings)
    pub build_configuration: BuildConfiguration,
    /// The finalized dependencies for this output. If this is `None`, the dependencies have not been resolved yet.
    /// During the `run_build` functions, the dependencies are resolved and this field is filled.
    pub finalized_dependencies: Option<FinalizedDependencies>,
    /// The finalized sources for this output. Contain the exact git hashes for the sources that are used
    /// to build this output.
    pub finalized_sources: Option<Vec<Source>>,

    /// Summary of the build
    #[serde(skip)]
    pub build_summary: Arc<Mutex<BuildSummary>>,
    /// The system tools that are used to build this output
    pub system_tools: SystemTools,
}

impl Output {
    /// The name of the package
    pub fn name(&self) -> &PackageName {
        self.recipe.package().name()
    }

    /// The version of the package
    pub fn version(&self) -> &str {
        self.recipe.package().version()
    }

    /// The build string is usually set automatically as the hash of the variant configuration.
    pub fn build_string(&self) -> Option<&str> {
        self.recipe.build().string()
    }

    /// The channels to use when resolving dependencies
    pub fn reindex_channels(&self) -> Result<Vec<String>, std::io::Error> {
        let output_dir = &self.build_configuration.directories.output_dir;

        index(output_dir, Some(&self.build_configuration.target_platform))?;

        let mut channels = vec![output_dir.to_string_lossy().to_string()];
        channels.extend(self.build_configuration.channels.clone());
        Ok(channels)
    }

    /// retrieve an identifier for this output ({name}-{version}-{build_string})
    pub fn identifier(&self) -> Option<String> {
        Some(format!(
            "{}-{}-{}",
            self.name().as_normalized(),
            self.version(),
            self.build_string()?
        ))
    }

    /// Record a warning during the build
    pub fn record_warning(&self, warning: &str) {
        self.build_summary
            .lock()
            .unwrap()
            .warnings
            .push(warning.to_string());
    }

    /// Record the start of the build
    pub fn record_build_start(&self) {
        self.build_summary.lock().unwrap().build_start = Some(chrono::Utc::now());
    }

    /// Record the artifact that was created during the build
    pub fn record_artifact(&self, artifact: &Path, paths: &PathsJson) {
        let mut summary = self.build_summary.lock().unwrap();
        summary.artifact = Some(artifact.to_path_buf());
        summary.paths = Some(paths.clone());
    }

    /// Record the end of the build
    pub fn record_build_end(&self) {
        let mut summary = self.build_summary.lock().unwrap();
        summary.build_end = Some(chrono::Utc::now());
    }

    /// Shorthand to retrieve the variant configuration for this output
    pub fn variant(&self) -> &BTreeMap<String, String> {
        &self.build_configuration.variant
    }

    /// Shorthand to retrieve the host prefix for this output
    pub fn prefix(&self) -> &Path {
        &self.build_configuration.directories.host_prefix
    }

    /// Shorthand to retrieve the build prefix for this output
    pub fn build_prefix(&self) -> &Path {
        &self.build_configuration.directories.build_prefix
    }

    /// Shorthand to retrieve the target platform for this output
    pub fn target_platform(&self) -> &Platform {
        &self.build_configuration.target_platform
    }

    /// Shorthand to retrieve the target platform for this output
    pub fn host_platform(&self) -> &Platform {
        &self.build_configuration.host_platform
    }

    /// Search for the resolved package with the given name in the host prefix
    /// Returns a tuple of the package and a boolean indicating whether the package is directly requested
    pub fn find_resolved_package(&self, name: &str) -> Option<(&RepoDataRecord, bool)> {
        let host = self.finalized_dependencies.as_ref()?.host.as_ref()?;
        let record = host
            .resolved
            .iter()
            .find(|p| p.package_record.name.as_normalized() == name);

        let is_requested = host.specs.iter().any(|s| {
            s.spec()
                .name
                .as_ref()
                .map(|n| n.as_normalized() == name)
                .unwrap_or(false)
        });

        record.map(|r| (r, is_requested))
    }

    /// Print a nice summary of the build
    pub fn log_build_summary(&self) -> Result<(), std::io::Error> {
        let summary = self.build_summary.lock().unwrap();
        let identifier = self.identifier().unwrap_or_default();
        let span = tracing::info_span!("Build summary for", recipe = identifier);
        let _enter = span.enter();

        if let Some(artifact) = &summary.artifact {
            let bytes = HumanBytes(fs::metadata(artifact).map(|m| m.len()).unwrap_or(0));
            tracing::info!("Artifact: {} ({})", artifact.display(), bytes);
        } else {
            tracing::info!("No artifact was created");
        }
        tracing::info!("{}", self);

        if !summary.warnings.is_empty() {
            tracing::warn!("Warnings:");
            for warning in &summary.warnings {
                tracing::warn!("{}", warning);
            }
        }

        if let Ok(github_summary) = std::env::var("GITHUB_STEP_SUMMARY") {
            if !github_integration_enabled() {
                return Ok(());
            }
            // append to the summary file
            let mut summary_file = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(github_summary)?;

            writeln!(summary_file, "### Build summary for {}", identifier)?;
            if let Some(article) = &summary.artifact {
                let bytes = HumanBytes(fs::metadata(article).map(|m| m.len()).unwrap_or(0));
                writeln!(
                    summary_file,
                    "**Artifact**: {} ({})",
                    article.display(),
                    bytes
                )?;
            } else {
                writeln!(summary_file, "**No artifact was created**")?;
            }

            if let Some(paths) = &summary.paths {
                if paths.paths.is_empty() {
                    writeln!(summary_file, "Included files: **No files included**")?;
                } else {
                    /// Github detail expander
                    fn format_entry(entry: &PathsEntry) -> String {
                        let mut extra_info = Vec::new();
                        if entry.prefix_placeholder.is_some() {
                            extra_info.push("contains prefix");
                        }
                        if entry.no_link {
                            extra_info.push("no link");
                        }
                        match entry.path_type {
                            PathType::SoftLink => extra_info.push("soft link"),
                            // skip default
                            PathType::HardLink => {}
                            PathType::Directory => extra_info.push("directory"),
                        }
                        let bytes = entry.size_in_bytes.unwrap_or(0);

                        format!(
                            "| `{}` | {} | {} |",
                            entry.relative_path.to_string_lossy(),
                            HumanBytes(bytes),
                            extra_info.join(", ")
                        )
                    }

                    writeln!(summary_file, "<details>")?;
                    writeln!(
                        summary_file,
                        "<summary>Included files ({} files)</summary>\n",
                        paths.paths.len()
                    )?;
                    writeln!(summary_file, "| Path | Size | Extra info |")?;
                    writeln!(summary_file, "| --- | --- | --- |")?;
                    for path in &paths.paths {
                        writeln!(summary_file, "{}", format_entry(path))?;
                    }
                    writeln!(summary_file, "\n</details>\n")?;
                }
            }

            if !summary.warnings.is_empty() {
                writeln!(summary_file, "> [!WARNING]")?;
                writeln!(summary_file, "> **Warnings during build:**\n>")?;
                for warning in &summary.warnings {
                    writeln!(summary_file, "> - {}", warning)?;
                }
                writeln!(summary_file)?;
            }

            writeln!(
                summary_file,
                "<details><summary>Resolved dependencies</summary>\n\n{}\n</details>\n",
                self.format_as_markdown()
            )?;
        }
        Ok(())
    }
}

impl Output {
    /// Format the output as a markdown table
    pub fn format_as_markdown(&self) -> String {
        let mut output = String::new();
        self.format_table_with_option(&mut output, comfy_table::presets::ASCII_MARKDOWN, true)
            .expect("Could not format table");
        output
    }

    fn format_table_with_option(
        &self,
        f: &mut impl fmt::Write,
        table_format: &str,
        long: bool,
    ) -> std::fmt::Result {
        let template = || -> comfy_table::Table {
            let mut table = comfy_table::Table::new();
            if table_format == comfy_table::presets::UTF8_FULL {
                table
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
            } else {
                table.load_preset(table_format);
            }
            table
        };

        writeln!(
            f,
            "Variant configuration (hash: {}):",
            self.build_string().unwrap_or_default()
        )?;
        let mut table = template();
        if table_format != comfy_table::presets::UTF8_FULL {
            table.set_header(vec!["Key", "Value"]);
        }
        self.build_configuration.variant.iter().for_each(|(k, v)| {
            table.add_row(vec![k, v]);
        });
        writeln!(f, "{}\n", table)?;

        if let Some(finalized_dependencies) = &self.finalized_dependencies {
            if let Some(build) = &finalized_dependencies.build {
                writeln!(f, "Build dependencies:")?;
                writeln!(f, "{}\n", build.to_table(template(), long))?;
            }

            if let Some(host) = &finalized_dependencies.host {
                writeln!(f, "Host dependencies:")?;
                writeln!(f, "{}\n", host.to_table(template(), long))?;
            }

            if !finalized_dependencies.run.depends.is_empty() {
                writeln!(f, "Run dependencies:")?;
                writeln!(
                    f,
                    "{}\n",
                    finalized_dependencies.run.to_table(template(), long)
                )?;
            }
        }

        Ok(())
    }
}

impl Display for Output {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.format_table_with_option(f, comfy_table::presets::UTF8_FULL, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_build_dir_test() {
        // without build_id (aka timestamp)
        let dir = tempfile::tempdir().unwrap();
        let p1 = get_build_dir(dir.path(), "name", true, &Utc::now()).unwrap();
        let f1 = p1.file_name().unwrap();
        assert!(f1.eq("rattler-build_name"));

        // with build_id (aka timestamp)
        let timestamp = &Utc::now();
        let p2 = get_build_dir(dir.path(), "name", false, timestamp).unwrap();
        let f2 = p2.file_name().unwrap();
        let epoch = timestamp.timestamp();
        assert!(f2.eq(format!("rattler-build_name_{epoch}").as_str()));
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use chrono::TimeZone;
    use insta::assert_yaml_snapshot;
    use rattler_conda_types::{
        MatchSpec, NoArchType, PackageName, PackageRecord, RepoDataRecord, VersionWithSource,
    };
    use rattler_digest::{parse_digest_from_hex, Md5, Sha256};
    use url::Url;

    use crate::render::resolved_dependencies::{self, DependencyInfo};

    use super::{Directories, Output};

    #[test]
    fn test_directories_yaml_rendering() {
        let tempdir = tempfile::tempdir().unwrap();

        let directories = Directories::setup(
            "name",
            &tempdir.path().join("recipe"),
            &tempdir.path().join("output"),
            false,
            &chrono::Utc::now(),
        )
        .unwrap();
        directories.create_build_dir().unwrap();

        // test yaml roundtrip
        let yaml = serde_yaml::to_string(&directories).unwrap();
        let directories2: Directories = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(directories.build_dir, directories2.build_dir);
        assert_eq!(directories.build_prefix, directories2.build_prefix);
        assert_eq!(directories.host_prefix, directories2.host_prefix);
    }

    #[test]
    fn test_resolved_dependencies_rendering() {
        let resolved_dependencies = resolved_dependencies::ResolvedDependencies {
            specs: vec![DependencyInfo::Raw {
                spec: MatchSpec::from_str("python 3.12.* h12332").unwrap(),
            }],
            resolved: vec![RepoDataRecord {
                package_record: PackageRecord {
                    arch: Some("x86_64".into()),
                    build: "h123".into(),
                    build_number: 0,
                    constrains: vec![],
                    depends: vec![],
                    features: None,
                    legacy_bz2_md5: None,
                    legacy_bz2_size: None,
                    license: Some("MIT".into()),
                    license_family: None,
                    md5: parse_digest_from_hex::<Md5>("68b329da9893e34099c7d8ad5cb9c940"),
                    name: PackageName::from_str("test").unwrap(),
                    noarch: NoArchType::none(),
                    platform: Some("linux".into()),
                    sha256: parse_digest_from_hex::<Sha256>(
                        "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    ),
                    size: Some(123123),
                    subdir: "linux-64".into(),
                    timestamp: Some(chrono::Utc.timestamp_opt(123123, 0).unwrap()),
                    track_features: vec![],
                    version: VersionWithSource::from_str("1.2.3").unwrap(),
                    purls: Default::default(),
                },
                file_name: "test-1.2.3-h123.tar.bz2".into(),
                url: Url::from_str("https://test.com/test/linux-64/test-1.2.3-h123.tar.bz2")
                    .unwrap(),
                channel: "test".into(),
            }],
            run_exports: Default::default(),
        };

        // test yaml roundtrip
        assert_yaml_snapshot!(resolved_dependencies);
        let yaml = serde_yaml::to_string(&resolved_dependencies).unwrap();
        let resolved_dependencies2: resolved_dependencies::ResolvedDependencies =
            serde_yaml::from_str(&yaml).unwrap();
        let yaml2 = serde_yaml::to_string(&resolved_dependencies2).unwrap();
        assert_eq!(yaml, yaml2);

        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let yaml3 = std::fs::read_to_string(test_data_dir.join("dependencies.yaml")).unwrap();
        let parsed_yaml3: resolved_dependencies::ResolvedDependencies =
            serde_yaml::from_str(&yaml3).unwrap();

        assert_eq!("pip", parsed_yaml3.specs[0].render(false));
    }

    #[test]
    fn read_full_recipe() {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let recipe_1 = test_data_dir.join("rich_recipe.yaml");

        let recipe_1 = std::fs::read_to_string(recipe_1).unwrap();

        let output_rich: Output = serde_yaml::from_str(&recipe_1).unwrap();
        assert_yaml_snapshot!(output_rich);

        let recipe_2 = test_data_dir.join("curl_recipe.yaml");
        let recipe_2 = std::fs::read_to_string(recipe_2).unwrap();
        let output_curl: Output = serde_yaml::from_str(&recipe_2).unwrap();
        assert_yaml_snapshot!(output_curl);
    }

    #[test]
    fn read_recipe_with_sources() {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let recipe_1 = test_data_dir.join("git_source.yaml");
        let recipe_1 = std::fs::read_to_string(recipe_1).unwrap();

        let git_source_output: Output = serde_yaml::from_str(&recipe_1).unwrap();
        assert_yaml_snapshot!(git_source_output);
    }
}
