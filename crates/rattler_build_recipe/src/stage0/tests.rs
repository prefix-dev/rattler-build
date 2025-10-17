use serde::{Deserialize, Serialize};

use crate::stage0::types::{ConditionalList, ScriptContent, Value};

/// Python version specification for tests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PythonVersion {
    /// A single python version
    Single(Value<String>),
    /// Multiple python versions
    Multiple(Vec<Value<String>>),
}

/// A special Python test that checks if the imports are available and runs `pip check`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonTest {
    /// List of imports to test
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub imports: ConditionalList<String>,

    /// Whether to run `pip check` or not (default to true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pip_check: Option<Value<bool>>,

    /// Python version(s) to test against. If not specified, the default python version is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python_version: Option<PythonVersion>,
}

/// A special Perl test that checks if the imports are available
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerlTest {
    /// List of perl `uses` to test
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub uses: ConditionalList<String>,
}

/// A test that checks if R libraries can be loaded
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RTest {
    /// List of R libraries to test with library()
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub libraries: ConditionalList<String>,
}

/// A test that checks if Ruby gems/modules can be required
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RubyTest {
    /// List of Ruby modules to test with require
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub requires: ConditionalList<String>,
}

/// The extra requirements for the test
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestRequirements {
    /// Extra run requirements for the test.
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub run: ConditionalList<String>,

    /// Extra build requirements for the test (e.g. emulators, compilers, ...).
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub build: ConditionalList<String>,
}

/// The files that should be copied to the test directory
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestFiles {
    /// Files to be copied from the source directory to the test directory.
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub source: ConditionalList<String>,

    /// Files to be copied from the recipe directory to the test directory.
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub recipe: ConditionalList<String>,
}

/// A test that executes a script in a freshly created environment
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTest {
    /// The script to run (list of commands or script objects)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub script: ConditionalList<ScriptContent>,

    /// The (extra) requirements for the test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<CommandsTestRequirements>,

    /// Extra files to include in the test
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<CommandsTestFiles>,
}

/// A test that runs the tests of a downstream package
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownstreamTest {
    /// The name of the downstream package
    pub downstream: Value<String>,
}

/// Package content test that compares the contents of the package with the expected contents
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageContentsTest {
    /// File paths using glob patterns to check for existence or non-existence
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<PackageContentsCheckFiles>,

    /// Check existence of package init in env python site packages dir
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_packages: Option<PackageContentsCheckFiles>,

    /// Search for binary in prefix path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<PackageContentsCheckFiles>,

    /// Check for dynamic or static library file path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib: Option<PackageContentsCheckFiles>,

    /// Check if include path contains the file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<PackageContentsCheckFiles>,

    /// Whether to enable strict mode (error on non-matched files or missing files)
    #[serde(default, skip_serializing_if = "is_false")]
    pub strict: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Files to check for existence or non-existence
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageContentsCheckFiles {
    /// Files that must exist (glob patterns)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub exists: ConditionalList<String>,

    /// Files that must not exist (glob patterns)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub not_exists: ConditionalList<String>,
}

/// The test type enum
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

impl TestType {
    /// Collect all variables used in this test
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        match self {
            TestType::Python { python } => {
                for item in &python.imports {
                    if let crate::stage0::types::Item::Value(v) = item {
                        vars.extend(v.used_variables());
                    }
                }
                if let Some(pip_check) = &python.pip_check {
                    vars.extend(pip_check.used_variables());
                }
                if let Some(python_version) = &python.python_version {
                    match python_version {
                        PythonVersion::Single(v) => vars.extend(v.used_variables()),
                        PythonVersion::Multiple(versions) => {
                            for v in versions {
                                vars.extend(v.used_variables());
                            }
                        }
                    }
                }
            }
            TestType::Perl { perl } => {
                for item in &perl.uses {
                    if let crate::stage0::types::Item::Value(v) = item {
                        vars.extend(v.used_variables());
                    }
                }
            }
            TestType::R { r } => {
                for item in &r.libraries {
                    if let crate::stage0::types::Item::Value(v) = item {
                        vars.extend(v.used_variables());
                    }
                }
            }
            TestType::Ruby { ruby } => {
                for item in &ruby.requires {
                    if let crate::stage0::types::Item::Value(v) = item {
                        vars.extend(v.used_variables());
                    }
                }
            }
            TestType::Commands(commands) => {
                for item in &commands.script {
                    if let crate::stage0::types::Item::Value(v) = item {
                        vars.extend(v.used_variables());
                    }
                }
                if let Some(reqs) = &commands.requirements {
                    for item in &reqs.run {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &reqs.build {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
                if let Some(files) = &commands.files {
                    for item in &files.source {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &files.recipe {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
            }
            TestType::Downstream(downstream) => {
                vars.extend(downstream.downstream.used_variables());
            }
            TestType::PackageContents { package_contents } => {
                // Extract variables from all check files
                if let Some(files) = &package_contents.files {
                    for item in &files.exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &files.not_exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
                // Same for site_packages, bin, lib, include...
                if let Some(sp) = &package_contents.site_packages {
                    for item in &sp.exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &sp.not_exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
                if let Some(bin) = &package_contents.bin {
                    for item in &bin.exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &bin.not_exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
                if let Some(lib) = &package_contents.lib {
                    for item in &lib.exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &lib.not_exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
                if let Some(include) = &package_contents.include {
                    for item in &include.exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                    for item in &include.not_exists {
                        if let crate::stage0::types::Item::Value(v) = item {
                            vars.extend(v.used_variables());
                        }
                    }
                }
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
