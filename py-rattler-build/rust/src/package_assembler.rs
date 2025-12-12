//! Python bindings for the package builder API
//!
//! This module exposes the `assemble_package` function and supporting types
//! for creating conda packages from files and metadata.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use rattler_build_package::{
    ArchiveType, FileCollector, FileEntry, PackageBuilder, PackageConfig,
};
use rattler_conda_types::package::IndexJson;
use rattler_conda_types::{NoArchType, PackageName, Platform, VersionWithSource};

use crate::error::RattlerBuildError;

/// Archive type for conda packages
#[pyclass(name = "ArchiveType", eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyArchiveType {
    /// .tar.bz2 format (legacy)
    TarBz2 = 0,
    /// .conda format (modern, preferred)
    Conda = 1,
}

#[pymethods]
impl PyArchiveType {
    /// Get the file extension for this archive type
    fn extension(&self) -> &str {
        match self {
            PyArchiveType::TarBz2 => ".tar.bz2",
            PyArchiveType::Conda => ".conda",
        }
    }

    fn __repr__(&self) -> &str {
        match self {
            PyArchiveType::TarBz2 => "ArchiveType.TarBz2",
            PyArchiveType::Conda => "ArchiveType.Conda",
        }
    }
}

impl From<PyArchiveType> for ArchiveType {
    fn from(py_type: PyArchiveType) -> Self {
        match py_type {
            PyArchiveType::TarBz2 => ArchiveType::TarBz2,
            PyArchiveType::Conda => ArchiveType::Conda,
        }
    }
}

/// Represents a file to be included in the package
#[pyclass(name = "FileEntry")]
#[derive(Clone)]
pub struct PyFileEntry {
    pub(crate) inner: FileEntry,
}

#[pymethods]
impl PyFileEntry {
    /// Create a FileEntry from source and destination paths
    #[staticmethod]
    fn from_paths(source: PathBuf, destination: PathBuf) -> PyResult<Self> {
        let inner = FileEntry::from_paths(&source, &destination)
            .map_err(|e| RattlerBuildError::Other(e.to_string()))?;
        Ok(Self { inner })
    }

    #[getter]
    fn source(&self) -> PathBuf {
        self.inner.source.clone()
    }

    #[getter]
    fn destination(&self) -> PathBuf {
        self.inner.destination.clone()
    }

    #[getter]
    fn is_symlink(&self) -> bool {
        self.inner.is_symlink
    }

    #[getter]
    fn symlink_target(&self) -> Option<PathBuf> {
        self.inner.symlink_target.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "FileEntry(source='{}', destination='{}')",
            self.inner.source.display(),
            self.inner.destination.display()
        )
    }
}

/// Collects files from a directory for packaging
#[pyclass(name = "FileCollector")]
pub struct PyFileCollector {
    source_dir: PathBuf,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    follow_symlinks: bool,
    include_hidden: bool,
}

#[pymethods]
impl PyFileCollector {
    #[new]
    fn new(source_dir: PathBuf) -> Self {
        Self {
            source_dir,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            follow_symlinks: false,
            include_hidden: false,
        }
    }

    /// Add a glob pattern to include files
    fn include_glob(&mut self, pattern: &str) {
        self.include_patterns.push(pattern.to_string());
    }

    /// Add a glob pattern to exclude files
    fn exclude_glob(&mut self, pattern: &str) {
        self.exclude_patterns.push(pattern.to_string());
    }

    /// Set whether to follow symlinks
    fn set_follow_symlinks(&mut self, follow: bool) {
        self.follow_symlinks = follow;
    }

    /// Set whether to include hidden files
    fn set_include_hidden(&mut self, include: bool) {
        self.include_hidden = include;
    }

    /// Collect all matching files
    fn collect(&self) -> PyResult<Vec<PyFileEntry>> {
        let mut collector = FileCollector::new(self.source_dir.clone())
            .follow_symlinks(self.follow_symlinks)
            .include_hidden(self.include_hidden);

        for pattern in &self.include_patterns {
            collector = collector
                .include_glob(pattern)
                .map_err(|e| RattlerBuildError::Other(e.to_string()))?;
        }

        for pattern in &self.exclude_patterns {
            collector = collector
                .exclude_glob(pattern)
                .map_err(|e| RattlerBuildError::Other(e.to_string()))?;
        }

        let files = collector
            .collect()
            .map_err(|e| RattlerBuildError::Other(e.to_string()))?;

        Ok(files.into_iter().map(|f| PyFileEntry { inner: f }).collect())
    }

    fn __repr__(&self) -> String {
        format!("FileCollector(source_dir='{}')", self.source_dir.display())
    }
}

/// Result of successful package creation
#[pyclass(name = "PackageOutput")]
pub struct PyPackageOutput {
    path: PathBuf,
    identifier: String,
}

#[pymethods]
impl PyPackageOutput {
    /// Path to the created package file
    #[getter]
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Package identifier (name-version-build)
    #[getter]
    fn identifier(&self) -> String {
        self.identifier.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "PackageOutput(path='{}', identifier='{}')",
            self.path.display(),
            self.identifier
        )
    }
}

/// Create a conda package from files and metadata.
///
/// This is a low-level function for creating conda packages without a recipe.
/// Use this when you have files staged and want to package them directly.
#[pyfunction]
#[pyo3(signature = (
    name,
    version,
    target_platform,
    build_string,
    output_dir,
    *,
    files_dir=None,
    files=None,
    homepage=None,
    license=None,
    license_family=None,
    summary=None,
    description=None,
    depends=None,
    constrains=None,
    build_number=0,
    noarch=None,
    license_files=None,
    test_files=None,
    recipe_dir=None,
    compression_level=9,
    archive_type=None,
    timestamp=None,
    compression_threads=None,
    detect_prefix=true
))]
#[allow(clippy::too_many_arguments)]
pub fn assemble_package_py(
    name: &str,
    version: &str,
    target_platform: &str,
    build_string: &str,
    output_dir: PathBuf,
    // File sources
    files_dir: Option<PathBuf>,
    files: Option<Vec<PyFileEntry>>,
    // Package metadata
    homepage: Option<String>,
    license: Option<String>,
    license_family: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    // Dependencies
    depends: Option<Vec<String>>,
    constrains: Option<Vec<String>>,
    build_number: u64,
    noarch: Option<String>,
    // Additional files
    license_files: Option<Vec<PathBuf>>,
    test_files: Option<Vec<PathBuf>>,
    recipe_dir: Option<PathBuf>,
    // Build options
    compression_level: u8,
    archive_type: Option<PyArchiveType>,
    timestamp: Option<DateTime<Utc>>,
    compression_threads: Option<usize>,
    detect_prefix: bool,
) -> PyResult<PyPackageOutput> {
    // Parse and validate inputs
    let pkg_name = PackageName::try_from(name)
        .map_err(|e| RattlerBuildError::Other(format!("Invalid package name: {}", e)))?;

    let pkg_version: VersionWithSource = version
        .parse()
        .map_err(|e: rattler_conda_types::ParseVersionError| {
            RattlerBuildError::Other(format!("Invalid version: {}", e))
        })?;

    let platform: Platform = target_platform
        .parse()
        .map_err(|e: rattler_conda_types::ParsePlatformError| {
            RattlerBuildError::Other(format!("Invalid platform: {}", e))
        })?;

    // Build config
    let config = PackageConfig {
        compression_level,
        archive_type: archive_type.map(Into::into).unwrap_or(ArchiveType::Conda),
        timestamp,
        compression_threads: compression_threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        }),
        detect_prefix,
        store_recipe: recipe_dir.is_some(),
    };

    // Create the builder
    let mut builder = PackageBuilder::new(pkg_name.clone(), pkg_version.clone(), platform, config)
        .with_build_string(build_string);

    // Add files from directory
    if let Some(dir) = files_dir {
        builder = builder
            .with_files_from_dir(&dir)
            .map_err(|e| RattlerBuildError::Other(e.to_string()))?;
    }

    // Add explicit files
    if let Some(file_entries) = files {
        let entries: Vec<FileEntry> = file_entries.into_iter().map(|f| f.inner).collect();
        builder = builder.add_files(entries);
    }

    // Build about.json if any metadata is provided
    if homepage.is_some()
        || license.is_some()
        || license_family.is_some()
        || summary.is_some()
        || description.is_some()
    {
        let mut about_builder = rattler_build_package::AboutJsonBuilder::new();

        if let Some(hp) = homepage {
            about_builder = about_builder.with_homepage(hp);
        }
        if let Some(lic) = &license {
            about_builder = about_builder.with_license(lic.clone());
        }
        if let Some(fam) = license_family.clone() {
            about_builder = about_builder.with_license_family(fam);
        }
        if let Some(sum) = summary {
            about_builder = about_builder.with_summary(sum);
        }
        if let Some(desc) = description {
            about_builder = about_builder.with_description(desc);
        }

        builder = builder.with_about(about_builder.build());
    }

    // Build index.json
    let noarch_type = match noarch.as_deref() {
        Some("python") => NoArchType::python(),
        Some("generic") => NoArchType::generic(),
        _ => NoArchType::none(),
    };

    let index = IndexJson {
        name: pkg_name,
        version: pkg_version,
        build: build_string.to_string(),
        build_number,
        arch: platform.arch().map(|a| a.to_string()),
        platform: platform.only_platform().map(|p| p.to_string()),
        subdir: Some(platform.to_string()),
        license: license.clone(),
        license_family,
        timestamp: timestamp.map(Into::into),
        depends: depends.unwrap_or_default(),
        constrains: constrains.unwrap_or_default(),
        noarch: noarch_type,
        track_features: Vec::new(),
        features: None,
        python_site_packages_path: None,
        purls: None,
        experimental_extra_depends: Default::default(),
    };
    builder = builder.with_index(index);

    // Add license files
    if let Some(lic_files) = license_files {
        builder = builder.with_license_files(lic_files);
    }

    // Add test files
    if let Some(t_files) = test_files {
        builder = builder.with_test_files(t_files);
    }

    // Add recipe directory
    if let Some(r_dir) = recipe_dir {
        builder = builder.with_recipe_dir(r_dir);
    }

    // Build the package
    let output = builder
        .build(&output_dir)
        .map_err(|e| RattlerBuildError::Other(e.to_string()))?;

    Ok(PyPackageOutput {
        path: output.path,
        identifier: output.identifier,
    })
}

/// Register the package_assembler module
pub fn register_package_assembler_module(
    py: Python<'_>,
    parent_module: &Bound<'_, PyModule>,
) -> PyResult<()> {
    let package_assembler_module = PyModule::new(py, "_package_assembler")?;

    package_assembler_module.add_class::<PyArchiveType>()?;
    package_assembler_module.add_class::<PyFileEntry>()?;
    package_assembler_module.add_class::<PyFileCollector>()?;
    package_assembler_module.add_class::<PyPackageOutput>()?;
    package_assembler_module.add_function(wrap_pyfunction!(assemble_package_py, &package_assembler_module)?)?;

    parent_module.add_submodule(&package_assembler_module)?;
    Ok(())
}
