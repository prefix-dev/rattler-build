use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};

/// Define tests in your recipe that are executed after successfully building the package.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Test {
    /// Try importing a python module as a sanity check
    imports: Vec<String>,
    /// Run a list of given commands
    commands: Vec<String>,
    /// Extra requirements to be installed at test time
    requires: Vec<String>,
    /// Extra files to be copied to the test environment from the source dir (can be globs)
    source_files: Vec<String>,
    /// Extra files to be copied to the test environment from the build dir (can be globs)
    files: Vec<String>,
}

impl Test {
    /// Get the imports.
    pub fn imports(&self) -> &[String] {
        self.imports.as_slice()
    }

    /// Get the commands.
    pub fn commands(&self) -> &[String] {
        self.commands.as_slice()
    }

    /// Get the requires.
    pub fn requires(&self) -> &[String] {
        self.requires.as_slice()
    }

    /// Get the source files.
    pub fn source_files(&self) -> &[String] {
        self.source_files.as_slice()
    }

    /// Get the files.
    pub fn files(&self) -> &[String] {
        self.files.as_slice()
    }

    /// Check if there is not test commands to be run
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl TryConvertNode<Test> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Test, PartialParsingError> {
        match self {
            RenderedNode::Mapping(map) => {
                let mut test = Test::default();

                for (key, value) in map.iter() {
                    match key.as_str() {
                        "imports" => test.imports = value.try_convert("imports")?,
                        "commands" => test.commands = value.try_convert("commands")?,
                        "requires" => test.requires = value.try_convert("requires")?,
                        "source_files" => test.source_files = value.try_convert("source_files")?,
                        "files" => test.files = value.try_convert("files")?,
                        invalid => Err(_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(invalid.to_string().into()),
                            help = format!("expected fields for {name} is one of `imports`, `commands`, `requires`, `source_files`, `files`")
                        ))?
                    }
                }

                Ok(test)
            }
            RenderedNode::Scalar(_) => todo!(),
            RenderedNode::Sequence(_) => todo!(),
            RenderedNode::Null(_) => todo!(),
        }
    }
}
