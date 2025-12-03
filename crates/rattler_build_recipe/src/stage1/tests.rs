use rattler_build_script::Script;
use serde::{Deserialize, Serialize};

use crate::stage1::Dependency;

use super::GlobVec;

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
    pub run: Vec<Dependency>,

    /// Extra build requirements for the test (e.g. emulators, compilers, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<Dependency>,
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
    /// The script to run - contains script content, interpreter, environment variables, etc.
    /// This field is required to distinguish CommandsTest from other test types during deserialization.
    pub script: Script,

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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageContentsCheckFiles {
    /// Files that must exist (validated glob patterns)
    pub exists: GlobVec,

    /// Files that must not exist (validated glob patterns)
    pub not_exists: GlobVec,
}

impl Serialize for PackageContentsCheckFiles {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // If not_exists is empty, serialize as a plain list (backward compatible)
        if self.not_exists.is_empty() {
            self.exists.serialize(serializer)
        } else {
            // Otherwise serialize as a map with both fields
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("exists", &self.exists)?;
            map.serialize_entry("not_exists", &self.not_exists)?;
            map.end()
        }
    }
}

impl<'de> Deserialize<'de> for PackageContentsCheckFiles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PackageContentsCheckFilesInput {
            Map {
                #[serde(default)]
                exists: GlobVec,
                #[serde(default)]
                not_exists: GlobVec,
            },
            // Backward compatibility: a simple list is treated as 'exists' patterns
            List(GlobVec),
        }

        let input = PackageContentsCheckFilesInput::deserialize(deserializer)?;
        match input {
            PackageContentsCheckFilesInput::Map { exists, not_exists } => {
                Ok(PackageContentsCheckFiles { exists, not_exists })
            }
            PackageContentsCheckFilesInput::List(exists) => Ok(PackageContentsCheckFiles {
                exists,
                not_exists: GlobVec::default(),
            }),
        }
    }
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

#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use super::*;
    use rattler_build_script::ScriptContent;

    /// Test serialization of all test types with insta snapshots
    #[test]
    fn test_serialization() {
        // Python test
        let python = TestType::Python {
            python: PythonTest {
                imports: vec!["numpy".to_string()],
                pip_check: true,
                python_version: PythonVersion::Single("3.11".to_string()),
            },
        };
        insta::assert_snapshot!(serde_yaml::to_string(&python).unwrap(), @r###"
        python:
          imports:
          - numpy
          python_version: '3.11'
        "###);

        // Downstream test
        let downstream = TestType::Downstream(DownstreamTest {
            downstream: "downstream-package".to_string(),
        });
        insta::assert_snapshot!(serde_yaml::to_string(&downstream).unwrap(), @r###"
        downstream: downstream-package
        "###);

        // Commands test
        let commands = TestType::Commands(CommandsTest {
            script: Script {
                content: ScriptContent::Command("echo 'test'".to_string()),
                ..Default::default()
            },
            requirements: CommandsTestRequirements::default(),
            files: CommandsTestFiles::default(),
        });
        insta::assert_snapshot!(serde_yaml::to_string(&commands).unwrap(), @r###"
        script:
          content: echo 'test'
        "###);
    }

    /// Test deserialization of all test types
    #[test]
    fn test_deserialization() {
        // Python
        let python: TestType = serde_yaml::from_str("python:\n  imports: [numpy]").unwrap();
        assert!(matches!(python, TestType::Python { .. }));

        // Downstream - this is the key fix for the reported bug
        let downstream: TestType = serde_yaml::from_str("downstream: pkg").unwrap();
        match downstream {
            TestType::Downstream(d) => assert_eq!(d.downstream, "pkg"),
            _ => panic!("Expected Downstream test"),
        }

        // Commands
        let commands: TestType = serde_yaml::from_str("script:\n  content: echo test").unwrap();
        assert!(matches!(commands, TestType::Commands(_)));
    }

    /// Test roundtrip serialization for all test types
    #[test]
    fn test_roundtrip() {
        let test_cases = vec![
            TestType::Python {
                python: PythonTest {
                    imports: vec!["numpy".to_string()],
                    pip_check: false,
                    python_version: PythonVersion::Multiple(vec!["3.10".to_string()]),
                },
            },
            TestType::Downstream(DownstreamTest {
                downstream: "my-pkg".to_string(),
            }),
            TestType::Commands(CommandsTest {
                script: Script {
                    content: ScriptContent::Commands(vec!["echo 'test'".to_string()]),
                    ..Default::default()
                },
                requirements: CommandsTestRequirements {
                    run: vec!["pytest".to_string()],
                    build: vec![],
                },
                files: CommandsTestFiles::default(),
            }),
        ];

        for original in test_cases {
            let yaml = serde_yaml::to_string(&original).unwrap();
            let deserialized: TestType = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(original, deserialized);
        }
    }

    #[test]
    fn test_downstream_not_confused_with_commands() {
        let yaml = "downstream: downstream-good";
        let test: TestType = serde_yaml::from_str(yaml).unwrap();
        match test {
            TestType::Downstream(d) => assert_eq!(d.downstream, "downstream-good"),
            other => panic!("Expected Downstream, got: {:?}", other),
        }
    }

    #[test]
    fn test_multiple_tests_vector() {
        let yaml = r#"
- script:
    content: echo "test"
- downstream: downstream-good
- python:
    imports: [numpy]
"#;
        let tests: Vec<TestType> = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(tests.len(), 3);
        assert!(matches!(tests[0], TestType::Commands(_)));
        assert!(matches!(tests[1], TestType::Downstream(_)));
        assert!(matches!(tests[2], TestType::Python { .. }));
    }
}
