//! Package builder - main API for creating conda packages

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rattler_conda_types::package::{AboutJson, IndexJson, PathsJson, RunExportsJson};
use rattler_conda_types::{PackageName, Platform, VersionWithSource};

use crate::files::FileEntry;
use crate::{ArchiveType, PackageError, Result};

/// Configuration for package creation
#[derive(Debug, Clone)]
pub struct PackageConfig {
    /// Compression level (0-9, higher = better compression but slower)
    pub compression_level: u8,

    /// Archive type to create
    pub archive_type: ArchiveType,

    /// Timestamp for reproducible builds
    pub timestamp: Option<DateTime<Utc>>,

    /// Number of threads to use for compression
    pub compression_threads: usize,

    /// Whether to detect and record prefix placeholders
    pub detect_prefix: bool,

    /// Whether to include recipe files in info/recipe/
    pub store_recipe: bool,
}

impl Default for PackageConfig {
    fn default() -> Self {
        Self {
            compression_level: 9,
            archive_type: ArchiveType::default(),
            timestamp: None,
            compression_threads: num_cpus_get(),
            detect_prefix: true,
            store_recipe: true,
        }
    }
}

/// Builder for creating conda packages
///
/// This is the main entry point for creating conda packages. It supports two modes:
/// 1. Building from a recipe (requires the `recipe` feature)
/// 2. Building from manual metadata
///
/// # Examples
///
/// ## From metadata
/// ```rust,no_run
/// use rattler_build_package::{PackageBuilder, PackageConfig};
/// use rattler_conda_types::{PackageName, Platform};
/// use std::path::Path;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let config = PackageConfig::default();
/// let output = PackageBuilder::new(
///         PackageName::new_unchecked("mypackage"),
///         "1.0.0".parse()?,
///         Platform::Linux64,
///         config
///     )
///     .with_files_from_dir(Path::new("/files"))?
///     .build(Path::new("/output"))?;
/// # Ok(())
/// # }
/// ```
pub struct PackageBuilder {
    // Required fields
    name: PackageName,
    version: VersionWithSource,
    target_platform: Platform,
    config: PackageConfig,

    // Metadata (can be set or derived)
    about: Option<AboutJson>,
    index: Option<IndexJson>,
    run_exports: Option<RunExportsJson>,

    // Files to include
    files: Vec<FileEntry>,

    // Optional components
    test_files: Vec<PathBuf>,
    license_files: Vec<PathBuf>,
    recipe_files: Option<PathBuf>,

    // Build string
    build_string: Option<String>,
}

impl PackageBuilder {
    /// Create a new package builder with the minimum required information
    ///
    /// # Arguments
    /// * `name` - Package name
    /// * `version` - Package version
    /// * `target_platform` - Target platform for this package
    /// * `config` - Package creation configuration
    pub fn new(
        name: PackageName,
        version: VersionWithSource,
        target_platform: Platform,
        config: PackageConfig,
    ) -> Self {
        Self {
            name,
            version,
            target_platform,
            config,
            about: None,
            index: None,
            run_exports: None,
            files: Vec::new(),
            test_files: Vec::new(),
            license_files: Vec::new(),
            recipe_files: None,
            build_string: None,
        }
    }

    /// Set the build string for this package
    ///
    /// If not set, it will be computed from the hash and build number
    pub fn with_build_string(mut self, build_string: impl Into<String>) -> Self {
        self.build_string = Some(build_string.into());
        self
    }

    /// Set the about.json metadata
    pub fn with_about(mut self, about: AboutJson) -> Self {
        self.about = Some(about);
        self
    }

    /// Set the index.json metadata
    pub fn with_index(mut self, index: IndexJson) -> Self {
        self.index = Some(index);
        self
    }

    /// Set the run_exports.json metadata
    pub fn with_run_exports(mut self, run_exports: RunExportsJson) -> Self {
        self.run_exports = Some(run_exports);
        self
    }

    /// Add files from a directory
    ///
    /// This will scan the directory and add all files found. Files can be filtered
    /// using the FileCollector API.
    pub fn with_files_from_dir(mut self, dir: &Path) -> Result<Self> {
        let collector = crate::files::FileCollector::new(dir.to_path_buf());
        let files = collector.collect()?;
        self.files.extend(files);
        Ok(self)
    }

    /// Add a single file to the package
    ///
    /// # Arguments
    /// * `src` - Source path on disk
    /// * `dest` - Destination path within the package
    pub fn add_file(mut self, src: &Path, dest: &Path) -> Result<Self> {
        let entry = FileEntry::from_paths(src, dest)?;
        self.files.push(entry);
        Ok(self)
    }

    /// Add multiple files to the package
    pub fn add_files(mut self, files: Vec<FileEntry>) -> Self {
        self.files.extend(files);
        self
    }

    /// Add test files to the package
    pub fn with_test_files(mut self, test_files: Vec<PathBuf>) -> Self {
        self.test_files = test_files;
        self
    }

    /// Add license files to the package
    pub fn with_license_files(mut self, license_files: Vec<PathBuf>) -> Self {
        self.license_files = license_files;
        self
    }

    /// Add recipe directory to the package
    ///
    /// This will include the recipe files in info/recipe/ if store_recipe is enabled
    pub fn with_recipe_dir(mut self, recipe_dir: PathBuf) -> Self {
        self.recipe_files = Some(recipe_dir);
        self
    }

    /// Build the package and write it to the output directory
    ///
    /// The package will be named according to conda conventions:
    /// `{name}-{version}-{build_string}.{ext}`
    pub fn build(self, output_dir: &Path) -> Result<PackageOutput> {
        // Validate that we have minimum requirements
        self.validate()?;

        // Build the package
        self.build_internal(output_dir)
    }

    /// Validate that the builder has all required components
    fn validate(&self) -> Result<()> {
        if self.files.is_empty() {
            tracing::warn!("Building package with no files");
        }

        if self.build_string.is_none() {
            return Err(PackageError::BuildStringNotSet);
        }

        Ok(())
    }

    /// Internal build implementation
    fn build_internal(self, output_dir: &Path) -> Result<PackageOutput> {
        use crate::archiver::PackageWriter;
        use crate::metadata::{PathsJsonBuilder, PrefixDetectionConfig};

        // Create temporary directory for staging
        let temp_dir = tempfile::TempDir::new()?;
        let temp_path = temp_dir.path();

        tracing::info!(
            "Staging files in temporary directory: {}",
            temp_path.display()
        );

        // Step 1: Stage all files in the temporary directory
        let mut staged_files = Vec::new();
        for file_entry in &self.files {
            let dest_path = temp_path.join(&file_entry.destination);

            // Create parent directories
            if let Some(parent) = dest_path.parent() {
                fs_err::create_dir_all(parent)?;
            }

            // Copy or create symlink
            if file_entry.is_symlink {
                if let Some(target) = &file_entry.symlink_target {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(target, &dest_path)?;
                    }
                    #[cfg(windows)]
                    {
                        if target.is_dir() {
                            std::os::windows::fs::symlink_dir(target, &dest_path)?;
                        } else {
                            std::os::windows::fs::symlink_file(target, &dest_path)?;
                        }
                    }
                }
            } else {
                fs_err::copy(&file_entry.source, &dest_path)?;
            }

            staged_files.push(dest_path);
        }

        tracing::info!("Staged {} files", staged_files.len());

        // Step 2: Create info directory and generate metadata
        let info_dir = temp_path.join("info");
        fs_err::create_dir_all(&info_dir)?;

        tracing::info!("Generating metadata files");

        // Generate paths.json
        let prefix_detection = if self.config.detect_prefix {
            PrefixDetectionConfig::default()
        } else {
            PrefixDetectionConfig {
                detect_binary: false,
                detect_text: false,
                ignore_patterns: Vec::new(),
            }
        };

        let paths_json = PathsJsonBuilder::new(temp_path.to_path_buf(), self.target_platform)
            .add_files(self.files.clone())
            .with_prefix_detection(prefix_detection)
            .build()?;

        // Write paths.json
        let paths_json_path = info_dir.join("paths.json");
        let paths_json_file = fs_err::File::create(&paths_json_path)?;
        serde_json::to_writer_pretty(paths_json_file, &paths_json)?;
        staged_files.push(paths_json_path);

        // Write index.json if provided
        if let Some(index) = self.index {
            let index_json_path = info_dir.join("index.json");
            let index_json_file = fs_err::File::create(&index_json_path)?;
            serde_json::to_writer_pretty(index_json_file, &index)?;
            staged_files.push(index_json_path);
        } else {
            tracing::warn!("No index.json provided - package may be incomplete");
        }

        // Write about.json if provided
        if let Some(about) = self.about {
            let about_json_path = info_dir.join("about.json");
            let about_json_file = fs_err::File::create(&about_json_path)?;
            serde_json::to_writer_pretty(about_json_file, &about)?;
            staged_files.push(about_json_path);
        }

        // Write run_exports.json if provided
        if let Some(run_exports) = self.run_exports {
            if !run_exports.is_empty() {
                let run_exports_path = info_dir.join("run_exports.json");
                let run_exports_file = fs_err::File::create(&run_exports_path)?;
                serde_json::to_writer_pretty(run_exports_file, &run_exports)?;
                staged_files.push(run_exports_path);
            }
        }

        tracing::info!("Metadata files generated");

        // Step 3: Copy license files if provided
        if !self.license_files.is_empty() {
            let licenses_dir = info_dir.join("licenses");
            fs_err::create_dir_all(&licenses_dir)?;

            tracing::info!("Copying {} license files", self.license_files.len());
            for license_file in &self.license_files {
                if license_file.exists() {
                    let file_name = license_file.file_name().ok_or_else(|| {
                        PackageError::InvalidMetadata(format!(
                            "Invalid license file path: {:?}",
                            license_file
                        ))
                    })?;
                    let dest = licenses_dir.join(file_name);
                    fs_err::copy(license_file, &dest)?;
                    staged_files.push(dest);
                } else {
                    tracing::warn!("License file not found: {:?}", license_file);
                }
            }
        }

        // Step 4: Copy test files if provided
        if !self.test_files.is_empty() {
            tracing::info!("Copying {} test files", self.test_files.len());
            for test_file in &self.test_files {
                if test_file.exists() {
                    let file_name = test_file.file_name().ok_or_else(|| {
                        PackageError::InvalidMetadata(format!(
                            "Invalid test file path: {:?}",
                            test_file
                        ))
                    })?;
                    let dest = info_dir.join(file_name);
                    fs_err::copy(test_file, &dest)?;
                    staged_files.push(dest);
                } else {
                    tracing::warn!("Test file not found: {:?}", test_file);
                }
            }
        }

        // Step 5: Copy recipe files if provided and store_recipe is enabled
        if self.config.store_recipe {
            if let Some(recipe_dir) = &self.recipe_files {
                let recipe_info_dir = info_dir.join("recipe");
                fs_err::create_dir_all(&recipe_info_dir)?;

                tracing::info!("Copying recipe files from {:?}", recipe_dir);

                // Copy all files from recipe directory
                if recipe_dir.is_dir() {
                    for entry in walkdir::WalkDir::new(recipe_dir)
                        .follow_links(false)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() {
                            let relative = path.strip_prefix(recipe_dir)?;
                            let dest = recipe_info_dir.join(relative);

                            if let Some(parent) = dest.parent() {
                                fs_err::create_dir_all(parent)?;
                            }

                            fs_err::copy(path, &dest)?;
                            staged_files.push(dest);
                        }
                    }
                } else {
                    tracing::warn!("Recipe directory not found: {:?}", recipe_dir);
                }
            }
        }

        // Step 6: Create the archive
        let identifier = format!(
            "{}-{}-{}",
            self.name.as_normalized(),
            self.version,
            self.build_string.as_ref().unwrap()
        );

        let output_path = output_dir.join(format!(
            "{}{}",
            identifier,
            self.config.archive_type.extension()
        ));

        tracing::info!("Creating package archive: {}", output_path.display());

        let mut writer =
            PackageWriter::new(self.config.archive_type, self.config.compression_level);

        if let Some(timestamp) = self.config.timestamp {
            writer = writer.with_timestamp(timestamp);
        }

        writer = writer.with_compression_threads(self.config.compression_threads);

        // Write the package
        writer.write(&output_path, temp_path, &staged_files, &identifier)?;

        tracing::info!("Package created successfully: {}", output_path.display());

        Ok(PackageOutput {
            path: output_path,
            paths_json,
            identifier,
        })
    }
}

/// Build from recipe (only available with recipe feature)
#[cfg(feature = "recipe")]
impl PackageBuilder {
    /// Create a PackageBuilder from a recipe
    ///
    /// This extracts all metadata from the recipe and sets up the builder
    /// appropriately. Note that dependencies must be finalized separately.
    ///
    /// # Arguments
    /// * `recipe` - The Stage1Recipe to build from
    /// * `target_platform` - The platform to build for
    /// * `build_string` - The build string (e.g., "h12345_0")
    /// * `config` - Package creation configuration
    pub fn from_recipe(
        recipe: &rattler_build_recipe::Stage1Recipe,
        target_platform: Platform,
        build_string: String,
        config: PackageConfig,
    ) -> Self {
        use crate::metadata::AboutJsonBuilder;

        let name = recipe.package().name().clone();
        let version = recipe.package().version().clone();

        let mut builder = Self::new(name.clone(), version.clone(), target_platform, config);
        builder.build_string = Some(build_string.clone());

        // Extract about.json from recipe
        let mut about = AboutJsonBuilder::new();

        if let Some(homepage) = &recipe.about().homepage {
            about = about.with_homepage(homepage.to_string());
        }

        if let Some(license) = &recipe.about().license {
            about = about.with_license(license.to_string());
        }

        if let Some(license_family) = &recipe.about().license_family {
            about = about.with_license_family(license_family.clone());
        }

        if let Some(summary) = &recipe.about().summary {
            about = about.with_summary(summary.clone());
        }

        if let Some(description) = &recipe.about().description {
            about = about.with_description(description.clone());
        }

        if let Some(doc_url) = &recipe.about().documentation {
            about = about.with_doc_url(doc_url.to_string());
        }

        if let Some(repo_url) = &recipe.about().repository {
            about = about.with_dev_url(repo_url.to_string());
        }

        builder.about = Some(about.build());

        // Note: index.json will need to be set separately with finalized dependencies
        // Users should call with_index() after resolving dependencies

        builder
    }
}

/// Result of successful package creation
#[derive(Debug)]
pub struct PackageOutput {
    /// Path to the created package file
    pub path: PathBuf,

    /// The paths.json data for this package
    pub paths_json: PathsJson,

    /// Package identifier (name-version-build)
    pub identifier: String,
}

// Helper to get number of CPUs
fn num_cpus_get() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
