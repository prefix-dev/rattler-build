use serde::{Deserialize, Serialize};

use crate::stage0::{
    SerializableMatchSpec,
    types::{ConditionalList, ConditionalListOrItem, Script, Value},
};

/// Python version specification for tests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PythonVersion {
    /// A single python version
    /// TODO: possibly change to use `VersionSpec` for proper parsing?
    Single(Value<String>),
    /// Multiple python versions
    Multiple(Vec<Value<String>>),
}

/// A special Python test that checks if the imports are available and runs `pip check`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonTest {
    /// List of imports to test (accepts either a single import or a list)
    #[serde(default, skip_serializing_if = "ConditionalListOrItem::is_empty")]
    pub imports: ConditionalListOrItem<String>,

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
    pub run: ConditionalList<SerializableMatchSpec>,

    /// Extra build requirements for the test (e.g. emulators, compilers, ...).
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub build: ConditionalList<SerializableMatchSpec>,
}

/// The files that should be copied to the test directory
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestFiles {
    /// Files to be copied from the source directory to the test directory.
    #[serde(default, skip_serializing_if = "ConditionalListOrItem::is_empty")]
    pub source: ConditionalListOrItem<String>,

    /// Files to be copied from the recipe directory to the test directory.
    #[serde(default, skip_serializing_if = "ConditionalListOrItem::is_empty")]
    pub recipe: ConditionalListOrItem<String>,
}

/// A test that executes a script in a freshly created environment
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTest {
    /// The script to run (with optional interpreter, env, content, etc.)
    #[serde(default, skip_serializing_if = "Script::is_default")]
    pub script: Script,

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
#[allow(clippy::large_enum_variant)]
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
                vars.extend(python.imports.used_variables());
                if let Some(pip_check) = &python.pip_check {
                    vars.extend(pip_check.used_variables());
                }
                if let Some(python_version) = &python.python_version {
                    match python_version {
                        PythonVersion::Single(v) => vars.extend(v.used_variables()),
                        PythonVersion::Multiple(versions) => {
                            // TODO(refactor): move to ConditionalList as well?
                            for v in versions {
                                vars.extend(v.used_variables());
                            }
                        }
                    }
                }
            }
            TestType::Perl { perl } => {
                vars.extend(perl.uses.used_variables());
            }
            TestType::R { r } => {
                vars.extend(r.libraries.used_variables());
            }
            TestType::Ruby { ruby } => {
                vars.extend(ruby.requires.used_variables());
            }
            TestType::Commands(commands) => {
                vars.extend(commands.script.used_variables());
                if let Some(reqs) = &commands.requirements {
                    // TODO(refactor): should this parse into matchspec?
                    vars.extend(reqs.run.used_variables());
                    vars.extend(reqs.build.used_variables());
                }
                if let Some(files) = &commands.files {
                    vars.extend(files.source.used_variables());
                    vars.extend(files.recipe.used_variables());
                }
            }
            TestType::Downstream(downstream) => {
                vars.extend(downstream.downstream.used_variables());
            }
            TestType::PackageContents { package_contents } => {
                // Extract variables from all check files
                if let Some(files) = &package_contents.files {
                    vars.extend(files.exists.used_variables());
                    vars.extend(files.not_exists.used_variables());
                }
                // Same for site_packages, bin, lib, include...
                if let Some(sp) = &package_contents.site_packages {
                    vars.extend(sp.exists.used_variables());
                    vars.extend(sp.not_exists.used_variables());
                }
                if let Some(bin) = &package_contents.bin {
                    vars.extend(bin.exists.used_variables());
                    vars.extend(bin.not_exists.used_variables());
                }

                if let Some(lib) = &package_contents.lib {
                    vars.extend(lib.exists.used_variables());
                    vars.extend(lib.not_exists.used_variables());
                }

                if let Some(include) = &package_contents.include {
                    vars.extend(include.exists.used_variables());
                    vars.extend(include.not_exists.used_variables());
                }
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
