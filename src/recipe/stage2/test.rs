use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, Node, SequenceNodeInternal},
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1,
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
    pub(super) fn from_stage1(
        test: &stage1::Test,
        jinja: &Jinja,
    ) -> Result<Self, PartialParsingError> {
        Ok(test
            .node
            .as_ref()
            .map(|node| Self::from_node(node, jinja))
            .transpose()?
            .unwrap_or_default())
    }

    fn from_node(node: &Node, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        /// Parse a [`Node`] that can be or a scalar, sequence of scalar or a conditional that results in scalar.
        fn parse(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
            match node {
                Node::Scalar(s) => {
                    let imports = jinja.render_str(s.as_str()).map_err(|err| {
                        _partialerror!(
                            *s.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering `imports`"
                        )
                    })?;
                    Ok(vec![imports])
                }
                Node::Sequence(seq) => {
                    let mut imports = Vec::new();
                    for inner in seq.iter() {
                        match inner {
                            SequenceNodeInternal::Simple(n) => imports.extend(parse(n, jinja)?),
                            SequenceNodeInternal::Conditional(if_sel) => {
                                let if_res = if_sel.process(jinja)?;
                                if let Some(if_res) = if_res {
                                    imports.extend(parse(&if_res, jinja)?)
                                }
                            }
                        }
                    }
                    Ok(imports)
                }
                Node::Mapping(_) => Err(_partialerror!(
                    *node.span(),
                    ErrorKind::Other,
                    label = "expected scalar or sequence"
                )),
            }
        }

        match node {
            Node::Mapping(map) => {
                let mut test = Self::default();

                for (key, value) in map.iter() {
                    match key.as_str() {
                        "imports" => test.imports = parse(value, jinja)?,
                        "commands" => test.commands = parse(value, jinja)?,
                        "requires" => test.requires = parse(value, jinja)?,
                        "source_files" => test.source_files = parse(value, jinja)?,
                        "files" => test.files = parse(value, jinja)?,
                        _ => Err(_partialerror!(
                            *key.span(),
                            ErrorKind::Other,
                            label = "Unknown key: expected one of `imports`, `commands`, `requires`, `source_files`, `files`"
                        ))?
                    }
                }

                Ok(test)
            }
            Node::Sequence(_) => todo!("Unimplemented: Sequence on Test"),
            Node::Scalar(_) => Err(_partialerror!(
                *node.span(),
                ErrorKind::Other,
                label = "expected mapping"
            )),
        }
    }

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
