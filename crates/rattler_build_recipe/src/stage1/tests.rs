use serde::{Deserialize, Serialize};

use super::glob_vec::GlobVec;

/// Python version specification for tests (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PythonVersion {
    /// A single python version
    Single(String),
    /// Multiple python versions
    Multiple(Vec<String>),
    /// No python version specified (use default)
    None,
}

impl PythonVersion {
    /// Check if the python version is none
    pub fn is_none(&self) -> bool {
        matches!(self, PythonVersion::None)
    }
}

impl Default for PythonVersion {
    fn default() -> Self {
        Self::None
    }
}

/// A special Python test that checks if the imports are available and runs `pip check` (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonTest {
    /// List of imports to test
    pub imports: Vec<String>,

    /// Whether to run `pip check` or not (default to true)
    #[serde(default = "default_pip_check", skip_serializing_if = "is_true")]
    pub pip_check: bool,

    /// Python version(s) to test against. If not specified, the default python version is used.
    #[serde(default, skip_serializing_if = "PythonVersion::is_none")]
    pub python_version: PythonVersion,
}

fn default_pip_check() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// A special Perl test that checks if the imports are available (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerlTest {
    /// List of perl `uses` to test
    pub uses: Vec<String>,
}

/// A test that checks if R libraries can be loaded (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RTest {
    /// List of R libraries to test with library()
    pub libraries: Vec<String>,
}

/// A test that checks if Ruby gems/modules can be required (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RubyTest {
    /// List of Ruby modules to test with require
    pub requires: Vec<String>,
}

/// The extra requirements for the test (evaluated)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestRequirements {
    /// Extra run requirements for the test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,

    /// Extra build requirements for the test (e.g. emulators, compilers, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<String>,
}

impl CommandsTestRequirements {
    /// Check if the requirements are empty
    pub fn is_empty(&self) -> bool {
        self.run.is_empty() && self.build.is_empty()
    }
}

/// The files that should be copied to the test directory (evaluated)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestFiles {
    /// Files to be copied from the source directory to the test directory (validated globs)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub source: GlobVec,

    /// Files to be copied from the recipe directory to the test directory (validated globs)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub recipe: GlobVec,
}

impl CommandsTestFiles {
    /// Check if the files are empty
    pub fn is_empty(&self) -> bool {
        self.source.is_empty() && self.recipe.is_empty()
    }
}

/// A test that executes a script in a freshly created environment (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTest {
    /// The script to run (list of commands)
    pub script: Vec<String>,

    /// The (extra) requirements for the test.
    #[serde(default, skip_serializing_if = "CommandsTestRequirements::is_empty")]
    pub requirements: CommandsTestRequirements,

    /// Extra files to include in the test
    #[serde(default, skip_serializing_if = "CommandsTestFiles::is_empty")]
    pub files: CommandsTestFiles,
}

/// A test that runs the tests of a downstream package (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownstreamTest {
    /// The name of the downstream package
    pub downstream: String,
}

/// Files to check for existence or non-existence (evaluated)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageContentsCheckFiles {
    /// Files that must exist (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub exists: GlobVec,

    /// Files that must not exist (validated glob patterns)
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub not_exists: GlobVec,
}

impl PackageContentsCheckFiles {
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.exists.is_empty() && self.not_exists.is_empty()
    }
}

/// Package content test that compares the contents of the package with the expected contents (evaluated)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageContentsTest {
    /// File paths using glob patterns to check for existence or non-existence
    #[serde(default, skip_serializing_if = "PackageContentsCheckFiles::is_empty")]
    pub files: PackageContentsCheckFiles,

    /// Check existence of package init in env python site packages dir
    #[serde(default, skip_serializing_if = "PackageContentsCheckFiles::is_empty")]
    pub site_packages: PackageContentsCheckFiles,

    /// Search for binary in prefix path
    #[serde(default, skip_serializing_if = "PackageContentsCheckFiles::is_empty")]
    pub bin: PackageContentsCheckFiles,

    /// Check for dynamic or static library file path
    #[serde(default, skip_serializing_if = "PackageContentsCheckFiles::is_empty")]
    pub lib: PackageContentsCheckFiles,

    /// Check if include path contains the file
    #[serde(default, skip_serializing_if = "PackageContentsCheckFiles::is_empty")]
    pub include: PackageContentsCheckFiles,

    /// Whether to enable strict mode (error on non-matched files or missing files)
    #[serde(default, skip_serializing_if = "is_false")]
    pub strict: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The test type enum (evaluated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TestType {
    /// A Python test that will test if the imports are available and run `pip check`
    Python {
        /// The imports to test and the `pip check` flag
        python: PythonTest,
    },
    /// A Perl test that will test if the modules are available
    Perl {
        /// The modules to test
        perl: PerlTest,
    },
    /// An R test that will test if the R libraries can be loaded
    R {
        /// The R libraries to load and test
        r: RTest,
    },
    /// A Ruby test that will test if the modules can be required
    Ruby {
        /// The modules to test
        ruby: RubyTest,
    },
    /// A test that executes multiple commands in a freshly created environment
    Commands(CommandsTest),
    /// A test that runs the tests of a downstream package
    Downstream(DownstreamTest),
    /// A test that checks the contents of the package
    PackageContents {
        /// The package contents to test against
        package_contents: PackageContentsTest,
    },
}
