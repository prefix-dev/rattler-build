use crate::{
    _partialerror,
    recipe::custom_yaml::{
        HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
        TryConvertNode,
    },
    recipe::error::{ErrorKind, PartialParsingError},
};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::Path;
use std::{borrow::Cow, path::PathBuf};

/// Defines the script to run to build the package.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Script {
    /// The interpreter to use for the script.
    pub interpreter: Option<String>,
    /// Environment variables to set in the build environment.
    pub env: IndexMap<String, String>,
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitive information. Use with care because this might make recipes no
    /// longer reproducible on other machines.
    pub secrets: Vec<String>,
    /// The contents of the script, either a path or a list of commands.
    pub content: ScriptContent,

    /// The current working directory for the script.
    pub cwd: Option<PathBuf>,
}

impl Serialize for Script {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        #[serde(untagged)]
        enum RawScriptContent<'a> {
            Command { content: &'a String },
            Commands { content: &'a Vec<String> },
            Path { file: &'a PathBuf },
        }

        #[derive(Serialize)]
        #[serde(untagged)]
        enum RawScript<'a> {
            CommandOrPath(&'a String),
            Commands(&'a Vec<String>),
            Object {
                #[serde(skip_serializing_if = "Option::is_none")]
                interpreter: Option<&'a String>,
                #[serde(skip_serializing_if = "IndexMap::is_empty")]
                env: &'a IndexMap<String, String>,
                #[serde(skip_serializing_if = "Vec::is_empty")]
                secrets: &'a Vec<String>,
                #[serde(skip_serializing_if = "Option::is_none", flatten)]
                content: Option<RawScriptContent<'a>>,
                #[serde(skip_serializing_if = "Option::is_none")]
                cwd: Option<&'a PathBuf>,
            },
        }

        let only_content = self.interpreter.is_none()
            && self.env.is_empty()
            && self.secrets.is_empty()
            && self.cwd.is_none();

        let raw_script = match &self.content {
            ScriptContent::CommandOrPath(content) if only_content => {
                RawScript::CommandOrPath(content)
            }
            ScriptContent::Commands(content) if only_content => RawScript::Commands(content),
            _ => RawScript::Object {
                interpreter: self.interpreter.as_ref(),
                env: &self.env,
                secrets: &self.secrets,
                cwd: self.cwd.as_ref(),
                content: match &self.content {
                    ScriptContent::Command(content) => Some(RawScriptContent::Command { content }),
                    ScriptContent::Commands(content) => {
                        Some(RawScriptContent::Commands { content })
                    }
                    ScriptContent::Path(file) => Some(RawScriptContent::Path { file }),
                    ScriptContent::Default => None,
                    ScriptContent::CommandOrPath(content) => {
                        Some(RawScriptContent::Command { content })
                    }
                },
            },
        };

        raw_script.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Script {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawScriptContent {
            Command { content: String },
            Commands { content: Vec<String> },
            Path { file: PathBuf },
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawScript {
            CommandOrPath(String),
            Commands(Vec<String>),
            Object {
                #[serde(default)]
                interpreter: Option<String>,
                #[serde(default)]
                env: IndexMap<String, String>,
                #[serde(default)]
                secrets: Vec<String>,
                #[serde(default, flatten)]
                content: Option<RawScriptContent>,
                #[serde(default)]
                cwd: Option<PathBuf>,
            },
        }

        let raw_script = RawScript::deserialize(deserializer)?;
        Ok(match raw_script {
            RawScript::CommandOrPath(str) => ScriptContent::CommandOrPath(str).into(),
            RawScript::Commands(commands) => ScriptContent::Commands(commands).into(),
            RawScript::Object {
                interpreter,
                env,
                secrets,
                content,
                cwd,
            } => Self {
                interpreter,
                env,
                secrets,
                cwd,
                content: match content {
                    Some(RawScriptContent::Command { content }) => ScriptContent::Command(content),
                    Some(RawScriptContent::Commands { content }) => {
                        ScriptContent::Commands(content)
                    }
                    Some(RawScriptContent::Path { file }) => ScriptContent::Path(file),
                    None => ScriptContent::Default,
                },
            },
        })
    }
}

impl Script {
    /// Returns the interpreter to use to execute the script
    pub fn interpreter(&self) -> Option<&str> {
        self.interpreter.as_deref()
    }

    /// Returns the script contents
    pub fn contents(&self) -> &ScriptContent {
        &self.content
    }

    /// Get the environment variables to set in the build environment.
    pub fn env(&self) -> &IndexMap<String, String> {
        &self.env
    }

    /// Get the secrets environment variables.
    ///
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitive information.
    ///
    /// # Warning
    /// Use with care because this might make recipes no longer reproducible on other machines.
    pub fn secrets(&self) -> &[String] {
        self.secrets.as_slice()
    }

    /// Returns true if the script references the default build script and has no additional
    /// configuration.
    pub fn is_default(&self) -> bool {
        self.content.is_default()
            && self.interpreter.is_none()
            && self.env.is_empty()
            && self.secrets.is_empty()
    }
}

impl From<ScriptContent> for Script {
    fn from(value: ScriptContent) -> Self {
        Self {
            interpreter: None,
            env: Default::default(),
            secrets: Default::default(),
            content: value,
            cwd: None,
        }
    }
}

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

/// Helper function to determine interpreter based on file extension
fn determine_interpreter_from_path(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| ext.to_lowercase())
        .and_then(|ext_lower| match ext_lower.as_str() {
            "py" => Some("python".to_string()),
            "pl" => Some("perl".to_string()),
            "r" => Some("rscript".to_string()),
            "sh" | "bash" => Some("bash".to_string()),
            "bat" | "cmd" => Some("cmd".to_string()),
            "ps1" => Some("powershell".to_string()),
            "nu" => Some("nushell".to_string()),
            _ => None,
        })
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

        if script.interpreter.is_none() {
            if let ScriptContent::Path(path) = &script.content {
                script.interpreter = determine_interpreter_from_path(path);
            }
        }

        Ok(script)
    }
}

/// Describes the contents of the script as defined in [`Script`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ScriptContent {
    /// Uses the default build script.
    #[default]
    Default,

    /// Either the script contents or the path to the script.
    CommandOrPath(String),

    /// A path to the script.
    Path(PathBuf),

    /// The script is given as inline code.
    Commands(Vec<String>),

    /// The script is given as a string
    Command(String),
}

impl ScriptContent {
    /// Check if the script content is the default.
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }
}
