//! Test parser module.

use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedSequenceNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
    validate_keys,
};

use super::{glob_vec::GlobVec, FlattenErrors, Script};

/// The extra requirements for the test
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestRequirements {
    /// Extra run requirements for the test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,

    /// Extra build requirements for the test (e.g. emulators, compilers, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<String>,
}

/// The files that should be copied to the test directory (they are stored in the package)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestFiles {
    /// Files to be copied from the source directory to the test directory.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub source: GlobVec,

    /// Files to be copied from the recipe directory to the test directory.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub recipe: GlobVec,
}

/// A test that executes a script in a freshly created environment
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTest {
    /// The script to run
    pub script: Script,
    /// The (extra) requirements for the test.
    /// Similar to the `requirements` section in the recipe the `build` requirements
    /// are of the build-computer architecture and the `run` requirements are of the
    /// target_platform architecture. The current package is implicitly added to the
    /// `run` requirements.
    #[serde(default, skip_serializing_if = "CommandsTestRequirements::is_empty")]
    pub requirements: CommandsTestRequirements,
    /// Extra files to include in the test
    #[serde(default, skip_serializing_if = "CommandsTestFiles::is_empty")]
    pub files: CommandsTestFiles,
}

impl CommandsTestRequirements {
    /// Check if the requirements are empty
    pub fn is_empty(&self) -> bool {
        self.run.is_empty() && self.build.is_empty()
    }
}

impl CommandsTestFiles {
    /// Check if the files are empty
    pub fn is_empty(&self) -> bool {
        self.source.is_empty() && self.recipe.is_empty()
    }
}

fn pip_check_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// The Python version(s) to test the imports against.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PythonVersion {
    /// A single python version
    Single(String),
    /// Multiple python versions
    Multiple(Vec<String>),
    /// No python version specified
    #[default]
    None,
}

impl PythonVersion {
    /// Check if the python version is none
    pub fn is_none(&self) -> bool {
        matches!(self, PythonVersion::None)
    }
}

/// A special Python test that checks if the imports are available and runs `pip check`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonTest {
    /// List of imports to test
    pub imports: Vec<String>,
    /// Whether to run `pip check` or not (default to true)
    #[serde(default = "pip_check_true", skip_serializing_if = "is_true")]
    pub pip_check: bool,
    /// Python version(s) to test against. If not specified, the default python version is used.
    #[serde(default, skip_serializing_if = "PythonVersion::is_none")]
    pub python_version: PythonVersion,
}

impl Default for PythonTest {
    fn default() -> Self {
        Self {
            imports: Vec::new(),
            pip_check: true,
            python_version: PythonVersion::None,
        }
    }
}

/// A special Perl test that checks if the imports are available and runs `cpanm check`.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerlTest {
    /// List of perl `uses` to test
    pub uses: Vec<String>,
}

/// A test that runs the tests of a downstream package.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownstreamTest {
    /// The name of the downstream package
    pub downstream: String,
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
    /// A test that executes multiple commands in a freshly created environment
    Command(CommandsTest),
    /// A test that runs the tests of a downstream package
    Downstream(DownstreamTest),
    /// A test that checks the contents of the package
    PackageContents {
        /// The package contents to test against
        // Note we use a struct for better serialization
        package_contents: PackageContentsTest,
    },
}

/// Package content test that compares the contents of the package with the expected contents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageContentsTest {
    /// file paths, direct and/or globs
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub files: GlobVec,
    /// checks existence of package init in env python site packages dir
    /// eg: mamba.api -> ${SITE_PACKAGES}/mamba/api/__init__.py
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub site_packages: GlobVec,
    /// search for binary in prefix path: eg, %PREFIX%/bin/mamba
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub bin: GlobVec,
    /// check for dynamic or static library file path
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub lib: GlobVec,
    /// check if include path contains the file, direct or glob?
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub include: GlobVec,
}

impl TryConvertNode<Vec<TestType>> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Vec<TestType>, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Sequence(seq) => seq.try_convert(name),
            RenderedNode::Scalar(_) | RenderedNode::Mapping(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedSequence,
            )])?,
            RenderedNode::Null(_) => Ok(vec![]),
        }
    }
}

impl TryConvertNode<Vec<TestType>> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<Vec<TestType>, Vec<PartialParsingError>> {
        let mut tests = vec![];
        for value in self.iter() {
            let test = value.try_convert(name)?;
            tests.push(test);
        }
        Ok(tests)
    }
}

impl TryConvertNode<TestType> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<TestType, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) | RenderedNode::Scalar(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
            )])?,
            RenderedNode::Null(_) => Ok(TestType::PackageContents {
                package_contents: PackageContentsTest::default(),
            }),
        }
    }
}

pub fn as_mapping(
    value: &RenderedNode,
    name: &str,
) -> Result<RenderedMappingNode, Vec<PartialParsingError>> {
    value.as_mapping().cloned().ok_or_else(|| {
        vec![_partialerror!(
            *value.span(),
            ErrorKind::ExpectedMapping,
            help = format!("expected fields for {name} to be a map")
        )]
    })
}

impl TryConvertNode<TestType> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<TestType, Vec<PartialParsingError>> {
        let mut test = TestType::PackageContents {
            package_contents: PackageContentsTest::default(),
        };

        self.iter().map(|(key, value)| {
            let key_str = key.as_str();
            match key_str {
                "python" => {
                    let python = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::Python{ python };
                }
                "script" | "requirements" | "files"  => {
                    let commands = self.try_convert(key_str)?;
                    test = TestType::Command(commands);
                }
                "downstream" => {
                    let downstream = self.try_convert(key_str)?;
                    test = TestType::Downstream(downstream);
                }
                "package_contents" => {
                    let package_contents = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::PackageContents { package_contents };
                }
                "perl" => {
                    let perl = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::Perl { perl };
                }
                invalid => Err(vec![_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `python`, `perl`, `script`, `downstream`, `package_contents`")
                )])?
            }
            Ok(())
        }).flatten_errors()?;

        Ok(test)
    }
}

///////////////////////////
/// Python Test         ///
///////////////////////////

impl TryConvertNode<PythonTest> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PythonTest, Vec<PartialParsingError>> {
        let mut python_test = PythonTest::default();

        validate_keys!(python_test, self.iter(), imports, pip_check, python_version);

        if python_test.imports.is_empty() {
            Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("imports".into()),
                help = "expected field `imports` in python test to be a list of imports"
            )])?;
        }

        Ok(python_test)
    }
}

impl TryConvertNode<PythonVersion> for RenderedNode {
    fn try_convert(&self, _name: &str) -> Result<PythonVersion, Vec<PartialParsingError>> {
        let python_version = match self {
            RenderedNode::Mapping(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::InvalidField("expected string, sequence or null".into()),
            )])?,
            RenderedNode::Scalar(version) => PythonVersion::Single(version.to_string()),
            RenderedNode::Sequence(versions) => versions
                .iter()
                .map(|v| {
                    v.as_scalar()
                        .ok_or_else(|| {
                            vec![_partialerror!(
                                *self.span(),
                                ErrorKind::InvalidField("invalid value".into()),
                            )]
                        })
                        .map(|s| s.to_string())
                })
                .collect::<Result<Vec<String>, _>>()
                .map(PythonVersion::Multiple)?,
            RenderedNode::Null(_) => PythonVersion::None,
        };

        Ok(python_version)
    }
}

///////////////////////////
/// Downstream Test     ///
///////////////////////////

impl TryConvertNode<DownstreamTest> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<DownstreamTest, Vec<PartialParsingError>> {
        let mut downstream = DownstreamTest::default();
        validate_keys!(downstream, self.iter(), downstream);
        Ok(downstream)
    }
}

///////////////////////////
/// Commands Test       ///
///////////////////////////

impl TryConvertNode<CommandsTestRequirements> for RenderedNode {
    fn try_convert(
        &self,
        name: &str,
    ) -> Result<CommandsTestRequirements, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping,)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<CommandsTestRequirements> for RenderedMappingNode {
    fn try_convert(
        &self,
        _name: &str,
    ) -> Result<CommandsTestRequirements, Vec<PartialParsingError>> {
        let mut requirements = CommandsTestRequirements::default();
        validate_keys!(requirements, self.iter(), run, build);
        Ok(requirements)
    }
}

impl TryConvertNode<CommandsTestFiles> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<CommandsTestFiles, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping,)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<CommandsTestFiles> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<CommandsTestFiles, Vec<PartialParsingError>> {
        let mut files = CommandsTestFiles::default();
        validate_keys!(files, self.iter(), source, recipe);
        Ok(files)
    }
}

impl TryConvertNode<CommandsTest> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<CommandsTest, Vec<PartialParsingError>> {
        let mut commands_test = CommandsTest::default();

        validate_keys!(commands_test, self.iter(), script, requirements, files);

        if commands_test.script.is_default() {
            Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("script".into()),
                help = "expected field `script` to be a list of commands"
            )])?;
        }

        Ok(commands_test)
    }
}

///////////////////////////
/// Perl Test           ///
///////////////////////////

impl TryConvertNode<PerlTest> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PerlTest, Vec<PartialParsingError>> {
        let mut perl_test = PerlTest::default();
        validate_keys!(perl_test, self.iter(), uses);
        Ok(perl_test)
    }
}

///////////////////////////
/// Package Contents    ///
///////////////////////////

impl TryConvertNode<PackageContentsTest> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PackageContentsTest, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) | RenderedNode::Scalar(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
            )])?,
            RenderedNode::Null(_) => Ok(PackageContentsTest::default()),
        }
    }
}

impl TryConvertNode<PackageContentsTest> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<PackageContentsTest, Vec<PartialParsingError>> {
        let mut package_contents = PackageContentsTest::default();
        validate_keys!(
            package_contents,
            self.iter(),
            files,
            site_packages,
            lib,
            bin,
            include
        );
        Ok(package_contents)
    }
}

#[cfg(test)]
#[allow(clippy::module_inception)]
mod test {
    use std::fs;

    use super::TestType;
    use insta::assert_snapshot;

    use crate::recipe::{
        custom_yaml::{RenderedNode, TryConvertNode},
        parser::test::PythonVersion,
    };

    #[test]
    fn test_parsing() {
        let test_section = r#"
        tests:
          - python:
              imports:
                - numpy.testing
                - numpy.matrix
        "#;

        // parse the YAML
        let yaml_root = RenderedNode::parse_yaml(0, test_section)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("tests").unwrap();
        let tests: Vec<TestType> = tests_node.try_convert("tests").unwrap();

        let yaml_serde = serde_yaml::to_string(&tests).unwrap();
        assert_snapshot!(yaml_serde);

        // from yaml
        let tests: Vec<TestType> = serde_yaml::from_str(&yaml_serde).unwrap();
        let t = tests.first();

        match t {
            Some(TestType::Python { python }) => {
                assert_eq!(python.imports, vec!["numpy.testing", "numpy.matrix"]);
                assert!(python.pip_check);
            }
            _ => panic!("expected python test"),
        }
    }

    #[test]
    fn test_script_parsing() {
        let test_data_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let text = fs::read_to_string(test_data_dir.join("recipes/test-section/script-test.yaml"))
            .unwrap();

        // parse the YAML
        let yaml_root = RenderedNode::parse_yaml(0, &text)
            .map_err(|err| vec![err])
            .unwrap();

        let tests_node = yaml_root.as_mapping().unwrap().get("tests").unwrap();
        let tests: Vec<TestType> = tests_node.try_convert("tests").unwrap();

        let yaml_serde = serde_yaml::to_string(&tests).unwrap();
        assert_snapshot!(yaml_serde);
    }

    #[test]
    fn test_python_parsing() {
        let test_section = r#"
        tests:
          - python:
              imports:
                - pandas
              python_version: "3.10"
          - python:
              imports:
                - pandas
              python_version: ["3.10", "3.12"]
        "#;

        // parse the YAML
        let yaml_root = RenderedNode::parse_yaml(0, test_section)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("tests").unwrap();
        let tests: Vec<TestType> = tests_node.try_convert("tests").unwrap();

        let yaml_serde = serde_yaml::to_string(&tests).unwrap();
        assert_snapshot!(yaml_serde);

        // from yaml
        let tests: Vec<TestType> = serde_yaml::from_str(&yaml_serde).unwrap();
        let t = tests.first();

        match t {
            Some(TestType::Python { python }) => {
                assert_eq!(python.imports, vec!["pandas"]);
                assert!(python.pip_check);
                assert_eq!(
                    python.python_version,
                    PythonVersion::Single("3.10".to_string())
                );
            }
            _ => panic!("expected python test"),
        }

        let t2 = tests.get(1);

        match t2 {
            Some(TestType::Python { python }) => {
                assert_eq!(python.imports, vec!["pandas"]);
                assert!(python.pip_check);
                assert_eq!(
                    python.python_version,
                    PythonVersion::Multiple(vec!["3.10".to_string(), "3.12".to_string()])
                );
            }
            _ => panic!("expected python test"),
        }
    }
}
