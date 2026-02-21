use crate::{
    _partialerror,
    recipe::custom_yaml::{
        HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
        TryConvertNode,
    },
    recipe::error::{ErrorKind, PartialParsingError},
};
use std::path::Path;
use std::{borrow::Cow, path::PathBuf};

// Re-export the script types from rattler_build_script
pub use rattler_build_script::{Script, ScriptContent, determine_interpreter_from_path};

/// Helper function to validate a path for invalid UTF-8 characters
fn validate_path_utf8(
    path: &Path,
    span: &impl HasSpan,
    field_name: &str,
) -> Result<(), Vec<PartialParsingError>> {
    if path.to_str().is_none() {
        return Err(vec![_partialerror!(
            *span.span(),
            ErrorKind::InvalidValue((
                field_name.to_string(),
                "path contains invalid UTF-8 characters".into()
            )),
            help = "Ensure the path contains only valid UTF-8 characters"
        )]);
    }
    Ok(())
}

impl TryConvertNode<Script> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Script, Vec<PartialParsingError>> {
        match self {
            RenderedNode::Scalar(s) => s.try_convert(name),
            RenderedNode::Sequence(seq) => seq.try_convert(name),
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Null(_) => Err(vec![_partialerror!(
                *self.span(),
                ErrorKind::MissingField(Cow::Owned(name.to_owned()))
            )]),
        }
    }
}

impl TryConvertNode<Script> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<Script, Vec<PartialParsingError>> {
        let path_str = self.source();
        let path = PathBuf::from(path_str);

        validate_path_utf8(&path, self, name)?;

        let mut script: Script = ScriptContent::CommandOrPath(path_str.to_owned()).into();
        script.interpreter = determine_interpreter_from_path(&path);

        Ok(script)
    }
}

impl TryConvertNode<Script> for RenderedSequenceNode {
    fn try_convert(&self, _name: &str) -> Result<Script, Vec<PartialParsingError>> {
        let mut strings: Vec<String> = Vec::new();

        for string in self.iter() {
            if let RenderedNode::Scalar(s) = string {
                strings.push(s.source().to_owned());
            }
        }

        if strings.is_empty() {
            Ok(ScriptContent::Commands(strings).into())
        } else if strings.len() == 1 {
            Ok(ScriptContent::CommandOrPath(strings[0].clone()).into())
        } else {
            Ok(ScriptContent::Commands(strings).into())
        }
    }
}

impl TryConvertNode<Script> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Script, Vec<PartialParsingError>> {
        let invalid = self.keys().find(|k| {
            !matches!(
                k.as_str(),
                "env" | "secrets" | "interpreter" | "content" | "file"
            )
        });

        if let Some(invalid) = invalid {
            return Err(vec![_partialerror!(
                *invalid.span(),
                ErrorKind::InvalidField(invalid.to_string().into()),
                help = format!(
                    "valid keys for {name} are `env`, `secrets`, `interpreter`, `content` or `file`"
                )
            )]);
        }

        let env = self
            .get("env")
            .map(|node| node.try_convert("env"))
            .transpose()?
            .unwrap_or_default();

        let secrets = self
            .get("secrets")
            .map(|node| node.try_convert("secrets"))
            .transpose()?
            .unwrap_or_default();

        let interpreter = self
            .get("interpreter")
            .map(|node| node.try_convert("interpreter"))
            .transpose()?
            .unwrap_or_default();

        let file = self.get("file");

        let content = self.get("content");

        let content = match (file, content) {
            (Some(file), Some(content)) => {
                let (last_node, last_node_name) =
                    if file.span().start().map(|s| s.line()).unwrap_or_default()
                        < content.span().start().map(|s| s.line()).unwrap_or_default()
                    {
                        (content, "content")
                    } else {
                        (file, "file")
                    };
                return Err(vec![_partialerror!(
                    *last_node.span(),
                    ErrorKind::InvalidField(last_node_name.into()),
                    help = "cannot specify both `content` and `file`"
                )]);
            }
            (Some(file), None) => file.try_convert("file").map(ScriptContent::Path)?,
            (None, Some(content)) => match content {
                RenderedNode::Scalar(node) => ScriptContent::Command(node.source().to_owned()),
                RenderedNode::Sequence(seq) => {
                    let commands: Result<Vec<String>, _> = seq
                        .iter()
                        .map(|node| {
                            if let RenderedNode::Scalar(scalar) = node {
                                Ok(scalar.source().to_owned())
                            } else {
                                Err(vec![_partialerror!(
                                    *node.span(),
                                    ErrorKind::ExpectedScalar,
                                    label = "expected a scalar value in sequence"
                                )])
                            }
                        })
                        .collect();
                    ScriptContent::Commands(commands?)
                }
                node => node.try_convert("content").map(ScriptContent::Commands)?,
            },
            (None, None) => ScriptContent::Default,
        };

        let mut script = Script {
            env,
            secrets,
            interpreter,
            content,
            cwd: None,
        };

        if script.interpreter.is_none()
            && let ScriptContent::Path(path) = &script.content
        {
            script.interpreter = determine_interpreter_from_path(path);
        }

        Ok(script)
    }
}
