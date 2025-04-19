//! Test parser module.

use rattler_conda_types::Platform;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
            TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
    validate_keys,
};

use super::{glob_vec::GlobVec, FlattenErrors, Script};
use rattler_conda_types::{NamelessMatchSpec, ParseStrictness};

/// The extra requirements for the test
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandsTestRequirements {
    /// Extra run requirements for the test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,

    /// Extra build requirements for the test (e.g. emulators, compilers, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<String>,
}

/// The files that should be copied to the test directory (they are stored in the package)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandsTestFiles {
    /// Files to be copied from the source directory to the test directory.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub source: GlobVec,

    /// Files to be copied from the recipe directory to the test directory.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub recipe: GlobVec,
}

/// A test that executes a script in a freshly created environment
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Runtime conditions for script evaluation
    #[serde(default, skip_serializing_if = "RuntimeConditions::is_empty")]
    pub runtime_conditions: RuntimeConditions,
}

/// Runtime conditions for script evaluation
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConditions {
    /// The condition to evaluate at runtime
    pub condition: String,
    /// The script to run if condition is true
    pub then_script: Option<Script>,
    /// The script to run if condition is false
    pub else_script: Option<Script>,
}

impl RuntimeConditions {
    /// Check if the runtime conditions are empty
    pub fn is_empty(&self) -> bool {
        self.condition.is_empty() && self.then_script.is_none() && self.else_script.is_none()
    }

    /// Evaluate the condition at runtime and return the appropriate script
    pub fn evaluate(&self, platform: &Platform) -> Option<&Script> {
        let condition = match self.condition.as_str() {
            "win" => platform == &Platform::Win64,
            "unix" => platform != &Platform::Win64,
            "osx" => platform == &Platform::Osx64 || platform == &Platform::OsxArm64,
            "linux" => platform == &Platform::Linux64 || platform == &Platform::LinuxAarch64,
            _ => false,
        };

        if condition {
            self.then_script.as_ref()
        } else {
            self.else_script.as_ref()
        }
    }
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

const fn pip_check_true() -> bool {
    true
}

const fn is_true(value: &bool) -> bool {
    *value
}

/// The Python version(s) to test the imports against.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// A special Python test that checks if the imports are available and runs `pip check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerlTest {
    /// List of perl `uses` to test
    pub uses: Vec<String>,
}

/// A test that runs the tests of a downstream package.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownstreamTest {
    /// The name of the downstream package
    pub downstream: String,
}

/// The test type enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    Command(Box<CommandsTest>),
    /// A test that runs the tests of a downstream package
    Downstream(DownstreamTest),
    /// A test that checks the contents of the package
    PackageContents {
        /// The package contents to test against
        // Note we use a struct for better serialization
        package_contents: Box<PackageContentsTest>,
    },
}

/// Package content test that compares the contents of the package with the expected contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
            Self::Sequence(seq) => seq.try_convert(name),
            Self::Scalar(_) | Self::Mapping(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedSequence,
            )])?,
            Self::Null(_) => Ok(vec![]),
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
            Self::Mapping(map) => map.try_convert(name),
            Self::Sequence(_) | Self::Scalar(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
            )])?,
            Self::Null(_) => Ok(TestType::PackageContents {
                package_contents: Box::new(PackageContentsTest::default()),
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
            package_contents: Box::new(PackageContentsTest::default()),
        };

        self.iter().map(|(key, value)| {
            let key_str = key.as_str();
            match key_str {
                "python" => {
                    let python = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::Python { python };
                }
                "script" | "requirements" | "files" => {
                    let mut commands = CommandsTest::default();
                    let mut has_script_or_condition = false;

                    if let Some(req_node) = self.get("requirements") {
                        commands.requirements = req_node.try_convert("requirements")?;
                    }
                    if let Some(files_node) = self.get("files") {
                        commands.files = files_node.try_convert("files")?;
                    }
                    if let Some(script_node) = self.get("script") {
                        match script_node {
                            RenderedNode::Mapping(_) => {
                                let map = as_mapping(script_node, "script")?;
                                if let (Some(if_node), Some(then_node)) = (map.get("if"), map.get("then")) {
                                    let condition: String = if_node.try_convert("if")?;
                                    let then_script: Script = then_node.try_convert("then")?;
                                    let else_script: Option<Script> = map.get("else").map(|n| n.try_convert("else")).transpose()?;

                                    if !["win", "unix", "osx", "linux"].contains(&condition.as_str()) {
                                        return Err(vec![_partialerror!(
                                            *if_node.span(),
                                            ErrorKind::InvalidValue((
                                                format!(
                                                    "invalid condition '{}', expected one of: win, unix, osx, linux",
                                                    condition
                                                ),
                                                Cow::Borrowed("")
                                            )),
                                        )]);
                                    }

                                    commands.runtime_conditions = RuntimeConditions {
                                        condition,
                                        then_script: Some(then_script),
                                        else_script,
                                    };
                                    has_script_or_condition = true;
                                } else {
                                    commands.script = script_node.try_convert("script")?;
                                    has_script_or_condition = !commands.script.is_default();
                                }
                            }
                            _ => {
                                commands.script = script_node.try_convert("script")?;
                                has_script_or_condition = !commands.script.is_default();
                            }
                        }
                    }

                    if has_script_or_condition || !commands.requirements.is_empty() || !commands.files.is_empty() {
                        test = TestType::Command(Box::new(commands));
                    } else if self.contains_key("files") || self.contains_key("site_packages") || self.contains_key("bin") || self.contains_key("lib") || self.contains_key("include") {
                        let package_contents: PackageContentsTest = self.try_convert("package_contents")?;
                        test = TestType::PackageContents {
                            package_contents: Box::new(package_contents),
                        };
                    } else {
                        return Err(vec![_partialerror!(
                            *self.span(),
                            ErrorKind::InvalidField("Unrecognized test type or missing required fields".into()),
                            help = "Test definition must contain one of: python, perl, script, requirements, files, downstream, package_contents"
                        )]);
                    }
                }
                "downstream" => {
                    let downstream = self.try_convert(key_str)?;
                    test = TestType::Downstream(downstream);
                }
                "package_contents" => {
                    let package_contents = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::PackageContents { package_contents: Box::new(package_contents) };
                }
                "perl" => {
                    let perl = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::Perl { perl };
                }
                invalid => Err(vec![_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `python`, `perl`, `script`, `requirements`, `files`, `downstream`, `package_contents`")
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
    fn try_convert(&self, name: &str) -> Result<PythonVersion, Vec<PartialParsingError>> {
        let python_version = match self {
            Self::Mapping(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::InvalidField("expected string, sequence or null".into()),
            )])?,
            Self::Scalar(version) => {
                let _: NamelessMatchSpec = version.try_convert(name)?;
                PythonVersion::Single(version.to_string())
            }
            Self::Sequence(versions) => {
                let version_strings = versions
                    .iter()
                    .map(|v| {
                        let scalar = v.as_scalar().ok_or_else(|| {
                            vec![_partialerror!(
                                *self.span(),
                                ErrorKind::InvalidField("invalid value".into()),
                            )]
                        })?;
                        let _: NamelessMatchSpec = scalar.try_convert(name)?;
                        Ok::<String, Vec<PartialParsingError>>(scalar.to_string())
                    })
                    .collect::<Result<Vec<String>, _>>()?;
                PythonVersion::Multiple(version_strings)
            }
            Self::Null(_) => PythonVersion::None,
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
            Self::Mapping(map) => map.try_convert(name),
            Self::Sequence(_) | Self::Scalar(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
            )])?,
            Self::Null(_) => Ok(PackageContentsTest::default()),
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

///////////////////////////
/// Python Version     ///
///////////////////////////
impl TryConvertNode<NamelessMatchSpec> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<NamelessMatchSpec, Vec<PartialParsingError>> {
        NamelessMatchSpec::from_str(self.as_str(), ParseStrictness::Strict).map_err(|err| {
            vec![_partialerror!(
                *self.span(),
                ErrorKind::from(err),
                label = format!(
                    "error parsing `{}` as a version specification",
                    self.as_str()
                )
            )]
        })
    }
}

impl TryConvertNode<NamelessMatchSpec> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<NamelessMatchSpec, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| {
                vec![_partialerror!(
                    *self.span(),
                    ErrorKind::ExpectedScalar,
                    label = format!("expected a string value for `{name}`")
                )]
            })
            .and_then(|s| s.try_convert(name))
    }
}

#[cfg(test)]
#[allow(clippy::module_inception)]
mod test {
    use fs_err as fs;

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
        let yaml_root = RenderedNode::parse_yaml(0, text.as_str())
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
              python_version: ">=3.10"
          - python:
              imports:
                - pandas
              python_version: [">=3.10", ">=3.12"]
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
                    PythonVersion::Single(">=3.10".to_string())
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
                    PythonVersion::Multiple(vec![">=3.10".to_string(), ">=3.12".to_string()])
                );
            }
            _ => panic!("expected python test"),
        }
    }
}
