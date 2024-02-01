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

use super::{glob_vec::GlobVec, FlattenErrors};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestRequirements {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandsTestFiles {
    // TODO parse as globs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipe: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsTest {
    pub script: Vec<String>,
    #[serde(default, skip_serializing_if = "CommandsTestRequirements::is_empty")]
    pub requirements: CommandsTestRequirements,
    #[serde(default, skip_serializing_if = "CommandsTestFiles::is_empty")]
    pub files: CommandsTestFiles,
}

impl CommandsTestRequirements {
    pub fn is_empty(&self) -> bool {
        self.run.is_empty() && self.build.is_empty()
    }
}

impl CommandsTestFiles {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PythonTest {
    /// List of imports to test
    pub imports: Vec<String>,
    /// Wether to run `pip check` or not (default to true)
    #[serde(default = "pip_check_true", skip_serializing_if = "is_true")]
    pub pip_check: bool,
}

impl Default for PythonTest {
    fn default() -> Self {
        Self {
            imports: Vec::new(),
            pip_check: true,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownstreamTest {
    pub downstream: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// The test type enum
#[serde(rename_all = "snake_case", tag = "test_type")]
pub enum TestType {
    /// A Python test.
    Python(PythonTest),
    /// A test that executes multiple commands in a freshly created environment
    Command(CommandsTest),
    /// A test that runs the tests of a downstream package
    Downstream(DownstreamTest),
    /// A test that checks the contents of the package
    PackageContents(PackageContentsTest),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// PackageContent
pub struct PackageContentsTest {
    /// file paths, direct and/or globs
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub files: GlobVec,
    /// checks existence of package init in env python site packages dir
    /// eg: mamba.api -> ${SITE_PACKAGES}/mamba/api/__init__.py
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub site_packages: Vec<String>,
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
            RenderedNode::Null(_) => Ok(TestType::PackageContents(PackageContentsTest::default())),
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
        let mut test = TestType::PackageContents(PackageContentsTest::default());

        self.iter().map(|(key, value)| {
            let key_str = key.as_str();

            match key_str {
                "python" => {
                    let imports = as_mapping(value, key_str)?.try_convert(key_str)?;
                    test = TestType::Python(imports);
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
                    test = TestType::PackageContents(package_contents);
                }
                invalid => Err(vec![_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `python`, `command`, `downstream`, `package_contents`")
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

        validate_keys!(python_test, self.iter(), imports, pip_check);

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

impl TryConvertNode<CommandsTestFiles> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<CommandsTestFiles, Vec<PartialParsingError>> {
        let mut files = CommandsTestFiles::default();
        validate_keys!(files, self.iter(), source, recipe);
        Ok(files)
    }
}

impl TryConvertNode<CommandsTest> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<CommandsTest, Vec<PartialParsingError>> {
        let mut script = vec![];
        let mut requirements = CommandsTestRequirements::default();
        let mut files = CommandsTestFiles::default();

        self.iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "script" => script = value.try_convert(key_str)?,
                    "requirements" => {
                        requirements = as_mapping(value, key_str)?.try_convert(key_str)?
                    }
                    "files" => files = as_mapping(value, key_str)?.try_convert(key_str)?,
                    invalid => Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!(
                        "expected fields for {name} is one of `script`, `requirements`, `files`"
                    )
                    )])?,
                }
                Ok(())
            })
            .flatten_errors()?;

        if script.is_empty() {
            Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("script".into()),
                help = "expected field `script` to be a list of commands"
            )])?;
        }

        Ok(CommandsTest {
            script,
            requirements,
            files,
        })
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
mod test {
    use super::TestType;
    use insta::assert_snapshot;

    use crate::recipe::custom_yaml::{RenderedNode, TryConvertNode};

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
        let t = tests.get(0);

        match t {
            Some(TestType::Python(python_test)) => {
                assert_eq!(python_test.imports, vec!["numpy.testing", "numpy.matrix"]);
                assert_eq!(python_test.pip_check, true);
            }
            _ => panic!("expected python test"),
        }
    }
}
