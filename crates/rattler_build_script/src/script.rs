//! Core script data model types.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};

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

    /// Windows-specific script content for noarch packages.
    ///
    /// When set, this content is used instead of `content` on Windows.
    /// This allows noarch packages to carry both Unix and Windows versions
    /// of file-based test scripts.
    pub content_windows: Option<String>,

    /// The current working directory for the script.
    pub cwd: Option<PathBuf>,

    /// Whether content was explicitly specified via `content:` field.
    /// When true, serialization should always use `{content: ...}` structure.
    pub content_explicit: bool,
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
                content_windows: Option<&'a String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                cwd: Option<&'a PathBuf>,
            },
        }

        let only_content = self.interpreter.is_none()
            && self.env.is_empty()
            && self.secrets.is_empty()
            && self.cwd.is_none()
            && self.content_windows.is_none();

        // When content_explicit is true, always use the Object form with content: field
        let raw_script = match &self.content {
            ScriptContent::CommandOrPath(content) if only_content && !self.content_explicit => {
                RawScript::CommandOrPath(content)
            }
            ScriptContent::Commands(content) if only_content && !self.content_explicit => {
                RawScript::Commands(content)
            }
            _ => RawScript::Object {
                interpreter: self.interpreter.as_ref(),
                env: &self.env,
                secrets: &self.secrets,
                cwd: self.cwd.as_ref(),
                content_windows: self.content_windows.as_ref(),
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
                content_windows: Option<String>,
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
                content_windows,
                cwd,
            } => {
                // When deserializing from Object form, content was explicitly specified
                let content_explicit = content.is_some();
                Self {
                    interpreter,
                    env,
                    secrets,
                    cwd,
                    content: match content {
                        Some(RawScriptContent::Command { content }) => {
                            ScriptContent::Command(content)
                        }
                        Some(RawScriptContent::Commands { content }) => {
                            ScriptContent::Commands(content)
                        }
                        Some(RawScriptContent::Path { file }) => ScriptContent::Path(file),
                        None => ScriptContent::Default,
                    },
                    content_windows,
                    content_explicit,
                }
            }
        })
    }
}

impl Script {
    /// Returns the interpreter to use to execute the script
    pub fn interpreter(&self) -> &str {
        self.interpreter
            .as_deref()
            .unwrap_or(if cfg!(windows) { "cmd" } else { "bash" })
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
            content_windows: None,
            cwd: None,
            content_explicit: false,
        }
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

/// Helper function to determine interpreter based on file extension
pub fn determine_interpreter_from_path(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| ext.to_lowercase())
        .and_then(|ext_lower| match ext_lower.as_str() {
            "py" => Some("python".to_string()),
            "rb" => Some("ruby".to_string()),
            "js" => Some("nodejs".to_string()),
            "pl" => Some("perl".to_string()),
            "r" => Some("rscript".to_string()),
            "sh" | "bash" => Some("bash".to_string()),
            "bat" | "cmd" => Some("cmd".to_string()),
            "ps1" => Some("powershell".to_string()),
            "nu" => Some("nushell".to_string()),
            _ => None,
        })
}
