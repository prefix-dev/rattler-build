// Python bindings for package inspection and testing

use std::{
    future::Future,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use crate::error::RattlerBuildError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_build_recipe::stage1::tests::{
    CommandsTest, DownstreamTest, PackageContentsCheckFiles, PackageContentsTest, PerlTest,
    PythonTest, PythonVersion, RTest, RubyTest, TestType,
};
use rattler_conda_types::{
    NamedChannelOrUrl,
    package::{
        CondaArchiveIdentifier, CondaArchiveType, IndexJson, PackageFile, PathsEntry, PathsJson,
    },
};
use rattler_package_streaming::seek::read_package_file;

// Imports for rebuild functionality
use ::rattler_build::{
    console_utils::LoggingOutputHandler,
    opt::{CommonData, PackageSource, RebuildData},
    rebuild_package_core,
    tool_configuration::TestStrategy,
};
use clap::ValueEnum;
use rattler_config::config::ConfigBase;

/// A loaded conda package for inspection and testing.
#[pyclass(name = "Package")]
pub struct PyPackage {
    /// Path to the package file
    path: PathBuf,
    /// Extracted package directory (lazily created)
    extracted_dir: Arc<Mutex<Option<tempfile::TempDir>>>,
    /// Parsed index.json metadata
    index: IndexJson,
    /// Parsed package identifier from filename
    archive_id: CondaArchiveIdentifier,
    /// Parsed tests from info/tests/tests.yaml (lazily loaded)
    tests_cache: Arc<Mutex<Option<Vec<TestType>>>>,
    /// Parsed paths.json (lazily loaded)
    paths_cache: Arc<Mutex<Option<PathsJson>>>,
}

#[pymethods]
impl PyPackage {
    /// Load a package from a .conda or .tar.bz2 file
    #[staticmethod]
    fn from_file(path: PathBuf) -> PyResult<Self> {
        // Read index.json from package without extracting
        let index: IndexJson = read_package_file(&path)
            .map_err(|e| RattlerBuildError::Other(format!("Failed to read package: {}", e)))?;

        // Parse archive identifier from filename
        let archive_id = CondaArchiveIdentifier::try_from_path(&path).ok_or_else(|| {
            RattlerBuildError::Other(format!(
                "Failed to parse package filename: {}",
                path.display()
            ))
        })?;

        Ok(Self {
            path,
            extracted_dir: Arc::new(Mutex::new(None)),
            index,
            archive_id,
            tests_cache: Arc::new(Mutex::new(None)),
            paths_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Package name
    #[getter]
    fn name(&self) -> String {
        self.index.name.as_normalized().to_string()
    }

    /// Package version
    #[getter]
    fn version(&self) -> String {
        self.index.version.version().to_string()
    }

    /// Build string (e.g., "py312_0")
    #[getter]
    fn build_string(&self) -> &str {
        &self.index.build
    }

    /// Build number
    #[getter]
    fn build_number(&self) -> u64 {
        self.index.build_number
    }

    /// Target platform subdirectory (e.g., "linux-64", "noarch")
    #[getter]
    fn subdir(&self) -> Option<String> {
        self.index.subdir.clone()
    }

    /// NoArch type (None, "python", or "generic")
    #[getter]
    fn noarch(&self) -> Option<String> {
        if self.index.noarch.is_none() {
            None
        } else if self.index.noarch.is_python() {
            Some("python".to_string())
        } else if self.index.noarch.is_generic() {
            Some("generic".to_string())
        } else {
            None
        }
    }

    /// Runtime dependencies
    #[getter]
    fn depends(&self) -> Vec<String> {
        self.index.depends.clone()
    }

    /// Dependency constraints
    #[getter]
    fn constrains(&self) -> Vec<String> {
        self.index.constrains.clone()
    }

    /// Package license
    #[getter]
    fn license(&self) -> Option<String> {
        self.index.license.clone()
    }

    /// License family
    #[getter]
    fn license_family(&self) -> Option<String> {
        self.index.license_family.clone()
    }

    /// Build timestamp as a datetime object
    #[getter]
    fn timestamp(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.index
            .timestamp
            .as_ref()
            .map(|ts| ts.datetime().to_owned())
    }

    /// Architecture (e.g., "x86_64")
    #[getter]
    fn arch(&self) -> Option<String> {
        self.index.arch.clone()
    }

    /// Platform (e.g., "linux")
    #[getter]
    fn platform(&self) -> Option<String> {
        self.index.platform.clone()
    }

    /// Path to the package file
    #[getter]
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Archive type ("conda" or "tar.bz2")
    #[getter]
    fn archive_type(&self) -> String {
        match self.archive_id.archive_type {
            CondaArchiveType::TarBz2 => "tar.bz2".to_string(),
            CondaArchiveType::Conda => "conda".to_string(),
        }
    }

    /// Filename of the package (e.g., "numpy-1.26.0-py312_0.conda")
    #[getter]
    fn filename(&self) -> String {
        self.archive_id.to_file_name()
    }

    /// List of all files in the package
    #[getter]
    fn files(&self) -> PyResult<Vec<String>> {
        let paths = self.ensure_paths_loaded()?;
        Ok(paths
            .paths
            .iter()
            .map(|p| p.relative_path.to_string_lossy().to_string())
            .collect())
    }

    /// List of tests embedded in the package
    #[getter]
    fn tests(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        use pyo3::IntoPyObject;
        let tests = self.ensure_tests_loaded()?;
        tests
            .iter()
            .enumerate()
            .map(|(index, test)| {
                let obj: Py<PyAny> = match test {
                    TestType::Python { python } => PyPythonTest {
                        inner: python.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                    TestType::Commands(cmd) => PyCommandsTest {
                        inner: cmd.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                    TestType::Perl { perl } => PyPerlTest {
                        inner: perl.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                    TestType::R { r } => PyRTest {
                        inner: r.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                    TestType::Ruby { ruby } => PyRubyTest {
                        inner: ruby.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                    TestType::Downstream(ds) => PyDownstreamTest {
                        inner: ds.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                    TestType::PackageContents { package_contents } => PyPackageContentsTest {
                        inner: package_contents.clone(),
                        index,
                    }
                    .into_pyobject(py)?
                    .into_any()
                    .unbind(),
                };
                Ok(obj)
            })
            .collect()
    }

    /// Number of tests in the package
    fn test_count(&self) -> PyResult<usize> {
        let tests = self.ensure_tests_loaded()?;
        Ok(tests.len())
    }

    /// Run a specific test by index
    #[pyo3(signature = (index, channel=None, channel_priority=None, debug=false, auth_file=None, allow_insecure_host=None, compression_threads=None, use_bz2=true, use_zstd=true, use_sharded=true))]
    #[allow(clippy::too_many_arguments)]
    fn run_test(
        &self,
        index: usize,
        channel: Option<Vec<String>>,
        channel_priority: Option<String>,
        debug: bool,
        auth_file: Option<PathBuf>,
        allow_insecure_host: Option<Vec<String>>,
        compression_threads: Option<u32>,
        use_bz2: bool,
        use_zstd: bool,
        use_sharded: bool,
    ) -> PyResult<PyTestResult> {
        self.run_test_internal(
            Some(index),
            channel,
            channel_priority,
            debug,
            auth_file,
            allow_insecure_host,
            compression_threads,
            use_bz2,
            use_zstd,
            use_sharded,
        )
        .map(|results| results.into_iter().next().unwrap())
    }

    /// Run all tests in the package
    #[pyo3(signature = (channel=None, channel_priority=None, debug=false, auth_file=None, allow_insecure_host=None, compression_threads=None, use_bz2=true, use_zstd=true, use_sharded=true))]
    #[allow(clippy::too_many_arguments)]
    fn run_tests(
        &self,
        channel: Option<Vec<String>>,
        channel_priority: Option<String>,
        debug: bool,
        auth_file: Option<PathBuf>,
        allow_insecure_host: Option<Vec<String>>,
        compression_threads: Option<u32>,
        use_bz2: bool,
        use_zstd: bool,
        use_sharded: bool,
    ) -> PyResult<Vec<PyTestResult>> {
        self.run_test_internal(
            None,
            channel,
            channel_priority,
            debug,
            auth_file,
            allow_insecure_host,
            compression_threads,
            use_bz2,
            use_zstd,
            use_sharded,
        )
    }

    /// Rebuild this package from its embedded recipe.
    ///
    /// Extracts the recipe embedded in the package and rebuilds it,
    /// then compares SHA256 hashes to verify reproducibility.
    #[pyo3(signature = (test=None, compression_threads=None, output_dir=None, auth_file=None, allow_insecure_host=None, use_bz2=true, use_zstd=true, use_sharded=true))]
    #[allow(clippy::too_many_arguments)]
    fn rebuild(
        &self,
        test: Option<String>,
        compression_threads: Option<u32>,
        output_dir: Option<PathBuf>,
        auth_file: Option<PathBuf>,
        allow_insecure_host: Option<Vec<String>>,
        use_bz2: bool,
        use_zstd: bool,
        use_sharded: bool,
    ) -> PyResult<PyRebuildResult> {
        // Parse test strategy
        let test_strategy = test
            .map(|t| TestStrategy::from_str(&t, false).unwrap())
            .unwrap_or_default();

        // Create common data
        let config = ConfigBase::<()>::default();
        let common = CommonData::new(
            output_dir,
            false,
            auth_file,
            config,
            None, // channel_priority
            allow_insecure_host,
            use_bz2,
            use_zstd,
            use_sharded,
        );

        // Create rebuild data using the package path
        let rebuild_data = RebuildData::new(
            PackageSource::Path(self.path.clone()),
            test_strategy,
            compression_threads,
            common,
        );

        // Create a simple logging handler
        let log_handler = LoggingOutputHandler::default();

        // Run the rebuild
        let result =
            run_async_task(async { rebuild_package_core(rebuild_data, log_handler).await })?;

        Ok(PyRebuildResult {
            original_path: result.original_path,
            rebuilt_path: result.rebuilt_path,
            original_sha256: result.original_sha256,
            rebuilt_sha256: result.rebuilt_sha256,
        })
    }

    /// Convert to a Python dictionary with all metadata
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("name", self.name())?;
        dict.set_item("version", self.version())?;
        dict.set_item("build_string", self.build_string())?;
        dict.set_item("build_number", self.build_number())?;
        dict.set_item("subdir", self.subdir())?;
        dict.set_item("noarch", self.noarch())?;
        dict.set_item("depends", self.depends())?;
        dict.set_item("constrains", self.constrains())?;
        dict.set_item("license", self.license())?;
        dict.set_item("license_family", self.license_family())?;
        dict.set_item("timestamp", self.timestamp())?;
        dict.set_item("arch", self.arch())?;
        dict.set_item("platform", self.platform())?;
        dict.set_item("path", self.path().to_string_lossy().to_string())?;
        dict.set_item("archive_type", self.archive_type())?;
        dict.set_item("filename", self.filename())?;
        Ok(dict.into())
    }

    fn __repr__(&self) -> String {
        format!(
            "Package({}-{}-{})",
            self.name(),
            self.version(),
            self.build_string()
        )
    }
}

impl PyPackage {
    /// Ensure the package is extracted to a temp directory
    fn ensure_extracted(&self) -> PyResult<PathBuf> {
        let mut extracted = self.extracted_dir.lock().unwrap();
        if let Some(ref dir) = *extracted {
            return Ok(dir.path().to_path_buf());
        }

        // Create a temp directory and extract the package
        let temp_dir = tempfile::tempdir()
            .map_err(|e| RattlerBuildError::Other(format!("Failed to create temp dir: {}", e)))?;

        rattler_package_streaming::fs::extract(&self.path, temp_dir.path())
            .map_err(|e| RattlerBuildError::Other(format!("Failed to extract package: {}", e)))?;

        let path = temp_dir.path().to_path_buf();
        *extracted = Some(temp_dir);
        Ok(path)
    }

    /// Load tests from info/tests/tests.yaml
    fn ensure_tests_loaded(&self) -> PyResult<Vec<TestType>> {
        let mut cache = self.tests_cache.lock().unwrap();
        if let Some(ref tests) = *cache {
            return Ok(tests.clone());
        }

        let extracted_dir = self.ensure_extracted()?;
        let tests_path = extracted_dir.join("info/tests/tests.yaml");

        let tests = if tests_path.exists() {
            let content = fs_err::read_to_string(&tests_path).map_err(|e| {
                RattlerBuildError::Other(format!("Failed to read tests.yaml: {}", e))
            })?;
            serde_yaml::from_str(&content).map_err(|e| {
                RattlerBuildError::Other(format!("Failed to parse tests.yaml: {}", e))
            })?
        } else {
            Vec::new()
        };

        *cache = Some(tests.clone());
        Ok(tests)
    }

    /// Load paths from paths.json
    fn ensure_paths_loaded(&self) -> PyResult<PathsJson> {
        let mut cache = self.paths_cache.lock().unwrap();
        if let Some(ref paths) = *cache {
            return Ok(paths.clone());
        }

        let extracted_dir = self.ensure_extracted()?;
        let paths = PathsJson::from_package_directory(&extracted_dir)
            .map_err(|e| RattlerBuildError::Other(format!("Failed to read paths.json: {}", e)))?;

        *cache = Some(paths.clone());
        Ok(paths)
    }

    /// Internal helper to run tests
    #[allow(clippy::too_many_arguments)]
    fn run_test_internal(
        &self,
        test_index: Option<usize>,
        channel: Option<Vec<String>>,
        channel_priority: Option<String>,
        debug: bool,
        auth_file: Option<PathBuf>,
        allow_insecure_host: Option<Vec<String>>,
        compression_threads: Option<u32>,
        use_bz2: bool,
        use_zstd: bool,
        use_sharded: bool,
    ) -> PyResult<Vec<PyTestResult>> {
        use ::rattler_build::{
            metadata::Debug,
            opt::{ChannelPriorityWrapper, CommonData, TestData},
            run_test,
        };
        use rattler_config::config::ConfigBase;

        let channel_priority = channel_priority
            .map(|c| ChannelPriorityWrapper::from_str(&c).map(|c| c.value))
            .transpose()
            .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))?;

        let config = ConfigBase::<()>::default();
        let common = CommonData::new(
            None,
            false,
            auth_file,
            config,
            channel_priority,
            allow_insecure_host,
            use_bz2,
            use_zstd,
            use_sharded,
        );

        let channel = match channel {
            None => None,
            Some(channel) => Some(
                channel
                    .iter()
                    .map(|c| {
                        NamedChannelOrUrl::from_str(c)
                            .map_err(|e| RattlerBuildError::ChannelPriority(e.to_string()))
                            .map_err(|e| e.into())
                    })
                    .collect::<PyResult<_>>()?,
            ),
        };

        // Get test count for results
        let tests = self.ensure_tests_loaded()?;
        let test_count = tests.len();

        // If running specific test, validate index
        if let Some(idx) = test_index
            && idx >= test_count
        {
            return Err(RattlerBuildError::Other(format!(
                "Test index {} out of range (0..{})",
                idx, test_count
            ))
            .into());
        }

        let test_data = TestData::new(
            self.path.clone(),
            channel,
            compression_threads,
            Debug::new(debug),
            test_index,
            common,
        );

        // Run the test(s)
        let result = run_async_task(async { run_test(test_data, None).await });

        match result {
            Ok(()) => {
                // Tests passed
                let results = if let Some(idx) = test_index {
                    vec![PyTestResult {
                        success: true,
                        output: Vec::new(),
                        test_index: idx,
                    }]
                } else {
                    (0..test_count)
                        .map(|idx| PyTestResult {
                            success: true,
                            output: Vec::new(),
                            test_index: idx,
                        })
                        .collect()
                };
                Ok(results)
            }
            Err(e) => {
                // Test failed
                let error_msg = e.to_string();
                if let Some(idx) = test_index {
                    Ok(vec![PyTestResult {
                        success: false,
                        output: vec![error_msg],
                        test_index: idx,
                    }])
                } else {
                    // When running all tests and one fails, we don't know which one
                    // Return a single result indicating failure
                    Ok(vec![PyTestResult {
                        success: false,
                        output: vec![error_msg],
                        test_index: 0,
                    }])
                }
            }
        }
    }
}

/// Execute async tasks in Python bindings with proper error handling
fn run_async_task<F, R>(future: F) -> PyResult<R>
where
    F: Future<Output = miette::Result<R>>,
{
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| RattlerBuildError::Other(format!("Failed to create async runtime: {}", e)))?;

    Ok(rt.block_on(async { future.await.map_err(RattlerBuildError::from) })?)
}

/// Python test - imports modules and optionally runs pip check
#[pyclass(name = "PythonTest")]
#[derive(Clone)]
pub struct PyPythonTest {
    inner: PythonTest,
    index: usize,
}

#[pymethods]
impl PyPythonTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// List of modules to import
    #[getter]
    fn imports(&self) -> Vec<String> {
        self.inner.imports.clone()
    }

    /// Whether to run pip check (default: true)
    #[getter]
    fn pip_check(&self) -> bool {
        self.inner.pip_check
    }

    /// Python version specification (single version, multiple versions, or none)
    #[getter]
    fn python_version(&self) -> Option<PyPythonVersion> {
        Some(PyPythonVersion {
            inner: self.inner.python_version.clone(),
        })
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!(
            "PythonTest(imports={:?}, pip_check={})",
            self.inner.imports, self.inner.pip_check
        )
    }
}

/// Python version specification
#[pyclass(name = "PythonVersion")]
#[derive(Clone)]
pub struct PyPythonVersion {
    inner: PythonVersion,
}

#[pymethods]
impl PyPythonVersion {
    /// Get the version as a single string (if single version)
    fn as_single(&self) -> Option<String> {
        match &self.inner {
            PythonVersion::Single(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// Get the versions as a list (if multiple versions)
    fn as_multiple(&self) -> Option<Vec<String>> {
        match &self.inner {
            PythonVersion::Multiple(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// Check if no specific version is set
    fn is_none(&self) -> bool {
        matches!(&self.inner, PythonVersion::None)
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            PythonVersion::Single(v) => format!("PythonVersion('{}')", v),
            PythonVersion::Multiple(v) => format!("PythonVersion({:?})", v),
            PythonVersion::None => "PythonVersion(None)".to_string(),
        }
    }
}

/// Commands test - runs arbitrary shell commands
#[pyclass(name = "CommandsTest")]
#[derive(Clone)]
pub struct PyCommandsTest {
    inner: CommandsTest,
    index: usize,
}

#[pymethods]
impl PyCommandsTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// The script content
    #[getter]
    fn script(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value =
            serde_json::to_value(&self.inner.script).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    /// Extra runtime requirements for the test
    #[getter]
    fn requirements_run(&self) -> Vec<String> {
        self.inner
            .requirements
            .run
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Extra build requirements for the test (e.g., emulators)
    #[getter]
    fn requirements_build(&self) -> Vec<String> {
        self.inner
            .requirements
            .build
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        "CommandsTest(...)".to_string()
    }
}

/// Perl test - tests Perl modules
#[pyclass(name = "PerlTest")]
#[derive(Clone)]
pub struct PyPerlTest {
    inner: PerlTest,
    index: usize,
}

#[pymethods]
impl PyPerlTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// List of Perl modules to load with 'use'
    #[getter]
    fn uses(&self) -> Vec<String> {
        self.inner.uses.clone()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("PerlTest(uses={:?})", self.inner.uses)
    }
}

/// R test - tests R libraries
#[pyclass(name = "RTest")]
#[derive(Clone)]
pub struct PyRTest {
    inner: RTest,
    index: usize,
}

#[pymethods]
impl PyRTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// List of R libraries to load with library()
    #[getter]
    fn libraries(&self) -> Vec<String> {
        self.inner.libraries.clone()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("RTest(libraries={:?})", self.inner.libraries)
    }
}

/// Ruby test - tests Ruby modules
#[pyclass(name = "RubyTest")]
#[derive(Clone)]
pub struct PyRubyTest {
    inner: RubyTest,
    index: usize,
}

#[pymethods]
impl PyRubyTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// List of Ruby modules to require
    #[getter]
    fn requires(&self) -> Vec<String> {
        self.inner.requires.clone()
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("RubyTest(requires={:?})", self.inner.requires)
    }
}

/// Downstream test - tests a downstream package that depends on this package
#[pyclass(name = "DownstreamTest")]
#[derive(Clone)]
pub struct PyDownstreamTest {
    inner: DownstreamTest,
    index: usize,
}

#[pymethods]
impl PyDownstreamTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// Name of the downstream package to test
    #[getter]
    fn downstream(&self) -> &str {
        &self.inner.downstream
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("DownstreamTest(downstream='{}')", self.inner.downstream)
    }
}

/// Package contents test - checks that files exist or don't exist in the package
#[pyclass(name = "PackageContentsTest")]
#[derive(Clone)]
pub struct PyPackageContentsTest {
    inner: PackageContentsTest,
    index: usize,
}

#[pymethods]
impl PyPackageContentsTest {
    /// Index of this test in the package's test list
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    /// File checks for all files
    #[getter]
    fn files(&self) -> PyFileChecks {
        PyFileChecks {
            inner: self.inner.files.clone(),
        }
    }

    /// File checks for Python site-packages
    #[getter]
    fn site_packages(&self) -> PyFileChecks {
        PyFileChecks {
            inner: self.inner.site_packages.clone(),
        }
    }

    /// File checks for binaries in bin/
    #[getter]
    fn bin(&self) -> PyFileChecks {
        PyFileChecks {
            inner: self.inner.bin.clone(),
        }
    }

    /// File checks for libraries
    #[getter]
    fn lib(&self) -> PyFileChecks {
        PyFileChecks {
            inner: self.inner.lib.clone(),
        }
    }

    /// File checks for include headers
    #[getter]
    fn include(&self) -> PyFileChecks {
        PyFileChecks {
            inner: self.inner.include.clone(),
        }
    }

    /// Whether to fail on non-matched glob patterns (strict mode)
    #[getter]
    fn strict(&self) -> bool {
        self.inner.strict
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let json_value = serde_json::to_value(&self.inner).map_err(RattlerBuildError::from)?;
        pythonize::pythonize(py, &json_value)
            .map(|obj| obj.into())
            .map_err(|e| RattlerBuildError::RecipeParse(format!("{}", e)).into())
    }

    fn __repr__(&self) -> String {
        format!("PackageContentsTest(strict={})", self.inner.strict)
    }
}

/// File existence checks (glob patterns)
#[pyclass(name = "FileChecks")]
#[derive(Clone)]
pub struct PyFileChecks {
    inner: PackageContentsCheckFiles,
}

#[pymethods]
impl PyFileChecks {
    /// Glob patterns that must match at least one file
    #[getter]
    fn exists(&self) -> Vec<String> {
        self.inner
            .exists
            .include_globs()
            .iter()
            .map(|g| g.source().to_string())
            .collect()
    }

    /// Glob patterns that must NOT match any file
    #[getter]
    fn not_exists(&self) -> Vec<String> {
        self.inner
            .not_exists
            .include_globs()
            .iter()
            .map(|g| g.source().to_string())
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "FileChecks(exists={}, not_exists={})",
            self.inner.exists.include_globs().len(),
            self.inner.not_exists.include_globs().len()
        )
    }
}

/// Result of running a test
#[pyclass(name = "TestResult")]
#[derive(Clone)]
pub struct PyTestResult {
    /// Whether the test passed
    #[pyo3(get)]
    pub success: bool,
    /// Test output/logs
    #[pyo3(get)]
    pub output: Vec<String>,
    /// Index of the test that was run
    #[pyo3(get)]
    pub test_index: usize,
}

#[pymethods]
impl PyTestResult {
    fn __repr__(&self) -> String {
        let status = if self.success { "PASS" } else { "FAIL" };
        format!("TestResult(index={}, status={})", self.test_index, status)
    }

    fn __bool__(&self) -> bool {
        self.success
    }
}

/// Path entry from paths.json
#[pyclass(name = "PathEntry")]
#[derive(Clone)]
pub struct PyPathEntry {
    inner: PathsEntry,
}

#[pymethods]
impl PyPathEntry {
    /// Relative path of the file in the package
    #[getter]
    fn relative_path(&self) -> String {
        self.inner.relative_path.to_string_lossy().to_string()
    }

    /// Whether to skip linking this file
    #[getter]
    fn no_link(&self) -> bool {
        self.inner.no_link
    }

    /// Path type: "hardlink", "softlink", or "directory"
    #[getter]
    fn path_type(&self) -> &str {
        use rattler_conda_types::package::PathType;
        match self.inner.path_type {
            PathType::HardLink => "hardlink",
            PathType::SoftLink => "softlink",
            PathType::Directory => "directory",
        }
    }

    /// Size of the file in bytes (if available)
    #[getter]
    fn size_in_bytes(&self) -> Option<u64> {
        self.inner.size_in_bytes
    }

    /// SHA256 hash of the file (if available)
    #[getter]
    fn sha256(&self) -> Option<String> {
        self.inner.sha256.as_ref().map(|h| format!("{:x}", h))
    }

    fn __repr__(&self) -> String {
        format!("PathEntry('{}')", self.inner.relative_path.display())
    }
}

/// Result of rebuilding a package
#[pyclass(name = "RebuildResult")]
pub struct PyRebuildResult {
    /// Path to the original package
    original_path: PathBuf,
    /// Path to the rebuilt package
    rebuilt_path: PathBuf,
    /// SHA256 of the original package
    original_sha256: String,
    /// SHA256 of the rebuilt package
    rebuilt_sha256: String,
}

#[pymethods]
impl PyRebuildResult {
    /// Path to the original package
    #[getter]
    fn original_path(&self) -> PathBuf {
        self.original_path.clone()
    }

    /// Path to the rebuilt package
    #[getter]
    fn rebuilt_path(&self) -> PathBuf {
        self.rebuilt_path.clone()
    }

    /// SHA256 hash of the original package (hex-encoded)
    #[getter]
    fn original_sha256(&self) -> &str {
        &self.original_sha256
    }

    /// SHA256 hash of the rebuilt package (hex-encoded)
    #[getter]
    fn rebuilt_sha256(&self) -> &str {
        &self.rebuilt_sha256
    }

    /// Returns true if the original and rebuilt packages are bit-for-bit identical
    #[getter]
    fn is_identical(&self) -> bool {
        self.original_sha256 == self.rebuilt_sha256
    }

    /// Returns a new Package object for the rebuilt package
    fn rebuilt_package(&self) -> PyResult<PyPackage> {
        PyPackage::from_file(self.rebuilt_path.clone())
    }

    fn __repr__(&self) -> String {
        let status = if self.is_identical() {
            "identical"
        } else {
            "different"
        };
        format!(
            "RebuildResult(original='{}', rebuilt='{}', status={})",
            self.original_path.display(),
            self.rebuilt_path.display(),
            status
        )
    }
}

pub fn register_package_module(
    py: Python<'_>,
    parent_module: &Bound<'_, PyModule>,
) -> PyResult<()> {
    let package_module = PyModule::new(py, "_package")?;

    package_module.add_class::<PyPackage>()?;
    package_module.add_class::<PyPythonTest>()?;
    package_module.add_class::<PyPythonVersion>()?;
    package_module.add_class::<PyCommandsTest>()?;
    package_module.add_class::<PyPerlTest>()?;
    package_module.add_class::<PyRTest>()?;
    package_module.add_class::<PyRubyTest>()?;
    package_module.add_class::<PyDownstreamTest>()?;
    package_module.add_class::<PyPackageContentsTest>()?;
    package_module.add_class::<PyFileChecks>()?;
    package_module.add_class::<PyTestResult>()?;
    package_module.add_class::<PyPathEntry>()?;
    package_module.add_class::<PyRebuildResult>()?;

    parent_module.add_submodule(&package_module)?;
    Ok(())
}
