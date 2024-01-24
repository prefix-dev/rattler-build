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
};

use super::FlattenErrors;

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

fn default_pip_check() -> bool {
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
    #[serde(default = "default_pip_check", skip_serializing_if = "is_true")]
    pub pip_check: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    PackageContents(PackageContents),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// PackageContent
pub struct PackageContents {
    /// file paths, direct and/or globs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// checks existence of package init in env python site packages dir
    /// eg: mamba.api -> ${SITE_PACKAGES}/mamba/api/__init__.py
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub site_packages: Vec<String>,
    /// search for binary in prefix path: eg, %PREFIX%/bin/mamba
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bin: Vec<String>,
    /// check for dynamic or static library file path
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lib: Vec<String>,
    /// check if include path contains the file, direct or glob?
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
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
            RenderedNode::Null(_) => Ok(TestType::PackageContents(PackageContents::default())),
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
        let mut test = TestType::PackageContents(PackageContents::default());

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
    fn try_convert(&self, name: &str) -> Result<PythonTest, Vec<PartialParsingError>> {
        let mut imports = vec![];
        let mut pip_check = true;

        self.iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "imports" => imports = value.try_convert(key_str)?,
                    "pip_check" => pip_check = value.try_convert(key_str)?,
                    invalid => Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!("expected fields for {name} is one of `imports`")
                    )])?,
                }
                Ok(())
            })
            .flatten_errors()?;

        if imports.is_empty() {
            Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField("imports".into()),
                help = "expected field `imports` in python test to be a list of imports"
            )])?;
        }

        Ok(PythonTest { imports, pip_check })
    }
}

///////////////////////////
/// Downstream Test     ///
///////////////////////////

impl TryConvertNode<DownstreamTest> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<DownstreamTest, Vec<PartialParsingError>> {
        let mut downstream = String::new();

        self.iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "downstream" => downstream = value.try_convert(key_str)?,
                    invalid => Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!("expected fields for {name} is one of `downstream`")
                    )])?,
                }
                Ok(())
            })
            .flatten_errors()?;

        Ok(DownstreamTest { downstream })
    }
}

///////////////////////////
/// Commands Test       ///
///////////////////////////

impl TryConvertNode<CommandsTestRequirements> for RenderedMappingNode {
    fn try_convert(
        &self,
        name: &str,
    ) -> Result<CommandsTestRequirements, Vec<PartialParsingError>> {
        let mut run = vec![];
        let mut build = vec![];

        self.iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "run" => run = value.try_convert(key_str)?,
                    "build" => build = value.try_convert(key_str)?,
                    invalid => Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!("expected fields for {name} is one of `run`, `build`")
                    )])?,
                }
                Ok(())
            })
            .flatten_errors()?;

        Ok(CommandsTestRequirements { run, build })
    }
}

impl TryConvertNode<CommandsTestFiles> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<CommandsTestFiles, Vec<PartialParsingError>> {
        let mut source = vec![];
        let mut recipe = vec![];

        self.iter()
            .map(|(key, value)| {
                let key_str = key.as_str();
                match key_str {
                    "source" => source = value.try_convert(key_str)?,
                    "recipe" => recipe = value.try_convert(key_str)?,
                    invalid => Err(vec![_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid.to_string().into()),
                        help = format!("expected fields for {name} is one of `source`, `build`")
                    )])?,
                }
                Ok(())
            })
            .flatten_errors()?;

        Ok(CommandsTestFiles { source, recipe })
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

impl TryConvertNode<PackageContents> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PackageContents, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) | RenderedNode::Scalar(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
            )])?,
            RenderedNode::Null(_) => Ok(PackageContents::default()),
        }
    }
}

impl TryConvertNode<PackageContents> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<PackageContents, Vec<PartialParsingError>> {
        let mut files = vec![];
        let mut site_packages = vec![];
        let mut lib = vec![];
        let mut bin = vec![];
        let mut include = vec![];

        self.iter().map(|(key, value)| {
            let key_str = key.as_str();
            match key_str {
                "files" => files = value.try_convert(key_str)?,
                "site_packages" => site_packages = value.try_convert(key_str)?,
                "lib" => lib = value.try_convert(key_str)?,
                "bin" => bin = value.try_convert(key_str)?,
                "include" => include = value.try_convert(key_str)?,
                invalid => Err(vec![_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `files`, `site_packages`, `lib`, `bin`, `include`")
                )])?
            }
            Ok(())
        }).flatten_errors()?;

        Ok(PackageContents {
            files,
            site_packages,
            bin,
            lib,
            include,
        })
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
                - import os
                - import sys
        "#;

        // parse the YAML
        let yaml_root = RenderedNode::parse_yaml(0, test_section)
            .map_err(|err| vec![err])
            .unwrap();
        let tests_node = yaml_root.as_mapping().unwrap().get("tests").unwrap();
        let tests: Vec<TestType> = tests_node.try_convert("tests").unwrap();

        assert_snapshot!(serde_yaml::to_string(&tests).unwrap());
    }
}
