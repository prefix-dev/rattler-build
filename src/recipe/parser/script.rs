use crate::{
    _partialerror,
    recipe::custom_yaml::{
        HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, RenderedSequenceNode,
        TryConvertNode,
    },
    recipe::error::{ErrorKind, PartialParsingError},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{borrow::Cow, collections::BTreeMap, path::PathBuf};

/// Defines the script to run to build the package.
#[derive(Debug, Default, Clone)]
pub struct Script {
    /// The interpreter to use for the script.
    pub(super) interpreter: Option<String>,
    /// Environment variables to set in the build environment.
    pub(super) env: BTreeMap<String, String>,
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information. Use with care because this might make recipes no
    /// longer reproducible on other machines.
    pub(super) secrets: Vec<String>,
    /// The contents of the script, either a path or a list of commands.
    pub(super) content: ScriptContent,
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
                #[serde(skip_serializing_if = "BTreeMap::is_empty")]
                env: &'a BTreeMap<String, String>,
                #[serde(skip_serializing_if = "Vec::is_empty")]
                secrets: &'a Vec<String>,
                #[serde(skip_serializing_if = "Option::is_none", flatten)]
                content: Option<RawScriptContent<'a>>,
            },
        }

        let raw_script = match &self.content {
            ScriptContent::CommandOrPath(content) => RawScript::CommandOrPath(content),
            ScriptContent::Commands(content)
                if self.interpreter.is_none() && self.env.is_empty() && self.secrets.is_empty() =>
            {
                RawScript::Commands(content)
            }
            _ => RawScript::Object {
                interpreter: self.interpreter.as_ref(),
                env: &self.env,
                secrets: &self.secrets,
                content: match &self.content {
                    ScriptContent::Command(content) => Some(RawScriptContent::Command { content }),
                    ScriptContent::Commands(content) => {
                        Some(RawScriptContent::Commands { content })
                    }
                    ScriptContent::Path(file) => Some(RawScriptContent::Path { file }),
                    ScriptContent::Default => None,
                    ScriptContent::CommandOrPath(_) => unreachable!(),
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
                env: BTreeMap<String, String>,
                #[serde(default)]
                secrets: Vec<String>,
                content: Option<RawScriptContent>,
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
            } => Self {
                interpreter,
                env,
                secrets,
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
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Get the secrets environment variables.
    ///
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information.
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
        }
    }
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
    fn try_convert(&self, _name: &str) -> Result<Script, Vec<PartialParsingError>> {
        Ok(ScriptContent::CommandOrPath(self.as_str().to_owned()).into())
    }
}

impl TryConvertNode<Script> for RenderedSequenceNode {
    fn try_convert(&self, name: &str) -> Result<Script, Vec<PartialParsingError>> {
        let strings = self
            .iter()
            .map(|node| node.try_convert(name))
            .collect::<Result<Vec<String>, _>>()?;

        if strings.len() == 1 {
            Ok(ScriptContent::CommandOrPath(strings[0].clone()).into())
        } else {
            Ok(ScriptContent::Commands(strings).into())
        }
    }
}

impl TryConvertNode<Script> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Script, Vec<PartialParsingError>> {
        let invalid = self.keys().find(|k| {
            matches!(
                k.as_str(),
                "env" | "secrets" | "interpreter" | "content" | "file"
            )
        });

        if let Some(invalid) = invalid {
            return Err(vec![_partialerror!(
                *invalid.span(),
                ErrorKind::InvalidField(invalid.to_string().into()),
                help = format!("valid keys for {name} are `env`, `secrets`, `interpreter`, `content` or `file`")
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
                    help = format!("cannot specify both `content` and `file`")
                )]);
            }
            (Some(file), None) => file.try_convert("file").map(ScriptContent::Path)?,
            (None, Some(content)) => match content {
                RenderedNode::Scalar(node) => ScriptContent::Command(node.as_str().to_owned()),
                node => node.try_convert("content").map(ScriptContent::Commands)?,
            },
            (None, None) => ScriptContent::Default,
        };

        Ok(Script {
            env,
            secrets,
            interpreter,
            content,
        })
    }
}

/// Describes the contents of the script as defined in [`Script`].
#[derive(Debug, Clone, Default)]
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
