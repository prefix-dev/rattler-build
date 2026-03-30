//! System tools are installed on the system (git, patchelf, install_name_tool, etc.)

use rattler_conda_types::Platform;
use rattler_shell::{activation::Activator, shell};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use thiserror::Error;

/// Errors that can occur when working with system tools
#[derive(Error, Debug)]
pub enum ToolError {
    /// The tool was not found on the system
    #[error("failed to find `{0}` ({1})")]
    ToolNotFound(Tool, which::Error),
}

/// Any third party tool that is used by rattler build should be added here
/// and the tool should be invoked through the system tools object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// The patch tool
    Patch,
    /// The patchelf tool (for Linux / ELF targets)
    Patchelf,
    /// The codesign tool (for macOS targets)
    Codesign,
    /// The install_name_tool (for macOS / MachO targets)
    InstallNameTool,
    /// The git tool
    Git,
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Tool::Codesign => "codesign".to_string(),
                Tool::Patch => "patch".to_string(),
                Tool::Patchelf => "patchelf".to_string(),
                Tool::InstallNameTool => "install_name_tool".to_string(),
                Tool::Git => "git".to_string(),
            }
        )
    }
}

/// Identifies the build tool (e.g. rattler-build, pixi-build-rust) by name
/// and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildToolInfo {
    /// The name of the build tool (e.g. "rattler-build", "pixi-build-rust")
    pub name: String,
    /// The version of the build tool
    pub version: String,
}

/// The system tools object is used to find and call system tools. It also keeps track of the
/// versions of the tools that are used.
#[derive(Debug, Clone)]
pub struct SystemTools {
    build_tool: BuildToolInfo,
    used_tools: Arc<Mutex<HashMap<Tool, String>>>,
    found_tools: Arc<Mutex<HashMap<Tool, PathBuf>>>,
    build_prefix: Option<PathBuf>,
}

impl SystemTools {
    /// Create a new system tools object with the given build tool name and
    /// version.
    pub fn new(build_tool_name: impl Into<String>, build_tool_version: impl Into<String>) -> Self {
        Self {
            build_tool: BuildToolInfo {
                name: build_tool_name.into(),
                version: build_tool_version.into(),
            },
            used_tools: Arc::new(Mutex::new(HashMap::new())),
            found_tools: Arc::new(Mutex::new(HashMap::new())),
            build_prefix: None,
        }
    }

    /// Create a copy of the system tools object and add a build prefix to search for tools.
    /// Tools that are found in the build prefix are not added to the used tools list.
    pub fn with_build_prefix(&self, prefix: &Path) -> Self {
        Self {
            build_prefix: Some(prefix.to_path_buf()),
            ..self.clone()
        }
    }

    /// Returns the build tool info.
    pub fn build_tool(&self) -> &BuildToolInfo {
        &self.build_tool
    }

    /// Warn if the build tool has changed compared to a previous run.
    pub fn warn_if_changed(&self, current: &BuildToolInfo) {
        if self.build_tool != *current {
            tracing::warn!(
                "build tool changed: previously built by {} {}, now {} {}",
                self.build_tool.name,
                self.build_tool.version,
                current.name,
                current.version,
            );
        }
    }

    /// Find the tool in the system and return the path to the tool
    pub fn find_tool(&self, tool: Tool) -> Result<PathBuf, which::Error> {
        let which = |tool: &str| -> Result<PathBuf, which::Error> {
            if let Some(build_prefix) = &self.build_prefix {
                let build_prefix_activator =
                    Activator::from_path(build_prefix, shell::Bash, Platform::current()).unwrap();

                let paths = std::env::join_paths(build_prefix_activator.paths).ok();
                let mut found_tool = which::which_in_global(&tool, paths)?;

                // if the tool is found in the build prefix, return it
                if let Some(found_tool) = found_tool.next() {
                    return Ok(found_tool);
                }
            }
            which::which(tool)
        };

        let (tool_path, found_version) = match tool {
            Tool::Patchelf => {
                let path = which("patchelf")?;
                // patch elf version
                let output = std::process::Command::new(&path)
                    .arg("--version")
                    .output()
                    .expect("Failed to execute command");
                let found_version = String::from_utf8_lossy(&output.stdout);

                (path, found_version.to_string())
            }
            Tool::InstallNameTool => {
                let path = which("install_name_tool")?;
                (path, "".to_string())
            }
            Tool::Codesign => {
                let path = which("codesign")?;
                (path, "".to_string())
            }
            Tool::Git => {
                let path = which("git")?;
                let output = std::process::Command::new(&path)
                    .arg("--version")
                    .output()
                    .expect("Failed to execute command");
                let found_version = String::from_utf8_lossy(&output.stdout);

                (path, found_version.to_string())
            }
            Tool::Patch => {
                let path = which("patch")?;
                let version = std::process::Command::new(&path)
                    .arg("--version")
                    .output()
                    .expect("Failed to execute `patch` command");
                let version = String::from_utf8_lossy(&version.stdout);
                (path, version.to_string())
            }
        };

        let found_version = found_version.trim().to_string();

        if let Some(build_prefix) = &self.build_prefix {
            // Do not cache tools found in the (temporary) build prefix
            if tool_path.starts_with(build_prefix) {
                return Ok(tool_path);
            }
        }

        self.found_tools
            .lock()
            .unwrap()
            .insert(tool, tool_path.clone());
        let prev_version = self.used_tools.lock().unwrap().get(&tool).cloned();

        if let Some(prev_version) = prev_version {
            if prev_version != found_version {
                tracing::warn!(
                    "Found different version of {}: {} and {}",
                    tool,
                    prev_version,
                    found_version
                );
            }
        } else {
            self.used_tools.lock().unwrap().insert(tool, found_version);
        }

        Ok(tool_path)
    }

    /// Create a new `std::process::Command` for the given tool. The command is created with the
    /// path to the tool and can be further configured with arguments and environment variables.
    pub fn call(&self, tool: Tool) -> Result<Command, ToolError> {
        let tool_path = self
            .find_tool(tool)
            .map_err(|e| ToolError::ToolNotFound(tool, e))?;
        Ok(std::process::Command::new(tool_path))
    }
}

impl Serialize for SystemTools {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let used_tools = self.used_tools.lock().unwrap();
        let is_rattler_build = self.build_tool.name == "rattler-build";

        // rattler-build: only the flat key; other tools: structured build_tool key + flat compat key
        let extra_entries = if is_rattler_build { 1 } else { 2 };
        let mut map = serializer.serialize_map(Some(used_tools.len() + extra_entries))?;

        if !is_rattler_build {
            // Emit the structured build_tool key for non-rattler-build tools
            map.serialize_entry("build_tool", &self.build_tool)?;
        }

        // Collect all flat entries into a BTreeMap for deterministic ordering
        let mut ordered_tools = BTreeMap::new();
        ordered_tools.insert(
            self.build_tool.name.clone(),
            self.build_tool.version.clone(),
        );
        for (tool, version) in used_tools.iter() {
            ordered_tools.insert(tool.to_string(), version.clone());
        }
        for (key, version) in &ordered_tools {
            map.serialize_entry(key, version)?;
        }

        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for SystemTools {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut raw_map = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;

        // Try to extract the structured build_tool key (new format)
        let build_tool = if let Some(bt) = raw_map.remove("build_tool") {
            serde_json::from_value::<BuildToolInfo>(bt).map_err(serde::de::Error::custom)?
        } else {
            // Old format: infer from the "rattler-build" flat key
            let version = raw_map
                .remove("rattler-build")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "unknown".to_string());
            BuildToolInfo {
                name: "rattler-build".to_string(),
                version,
            }
        };

        // Remove the flat compat key (it duplicates build_tool info)
        raw_map.remove(&build_tool.name);

        // Parse remaining entries as Tool -> version
        let mut used_tools = HashMap::new();
        for (key, value) in raw_map {
            if let Ok(tool) = serde_json::from_value::<Tool>(serde_json::Value::String(key))
                && let Some(version) = value.as_str()
            {
                used_tools.insert(tool, version.to_string());
            }
        }

        Ok(SystemTools {
            build_tool,
            used_tools: Arc::new(Mutex::new(used_tools)),
            found_tools: Arc::new(Mutex::new(HashMap::new())),
            build_prefix: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_system_tool() {
        let system_tool = SystemTools::new("rattler-build", "0.0.1");
        let mut cmd = system_tool.call(Tool::Patchelf).unwrap();
        let stdout = cmd.arg("--version").output().unwrap().stdout;
        let version = String::from_utf8_lossy(&stdout).trim().to_string();

        let found_tools = system_tool.found_tools.lock().unwrap();
        assert!(found_tools.contains_key(&Tool::Patchelf));

        let used_tools = system_tool.used_tools.lock().unwrap();
        assert!(used_tools.contains_key(&Tool::Patchelf));

        assert!(used_tools.get(&Tool::Patchelf).unwrap() == &version);
    }

    #[test]
    fn test_serialize() {
        // fix versions in used tools to test deserialization
        let mut used_tools = HashMap::new();
        used_tools.insert(Tool::Patchelf, "1.0.0".to_string());
        used_tools.insert(Tool::InstallNameTool, "2.0.0".to_string());
        used_tools.insert(Tool::Git, "3.0.0".to_string());

        let system_tool = SystemTools {
            build_tool: BuildToolInfo {
                name: "rattler-build".to_string(),
                version: "0.0.0".to_string(),
            },
            used_tools: Arc::new(Mutex::new(used_tools)),
            found_tools: Arc::new(Mutex::new(HashMap::new())),
            build_prefix: None,
        };

        let json = serde_json::to_string_pretty(&system_tool).unwrap();
        insta::assert_snapshot!(json);

        let deserialized: SystemTools = serde_json::from_str(&json).unwrap();
        assert!(
            deserialized
                .used_tools
                .lock()
                .unwrap()
                .contains_key(&Tool::Patchelf)
        );
    }

    #[test]
    fn test_serialize_non_rattler_build_tool() {
        let mut used_tools = HashMap::new();
        used_tools.insert(Tool::Patchelf, "1.0.0".to_string());

        let system_tool = SystemTools {
            build_tool: BuildToolInfo {
                name: "pixi-build-rust".to_string(),
                version: "0.1.0".to_string(),
            },
            used_tools: Arc::new(Mutex::new(used_tools)),
            found_tools: Arc::new(Mutex::new(HashMap::new())),
            build_prefix: None,
        };

        let json = serde_json::to_string_pretty(&system_tool).unwrap();
        insta::assert_snapshot!(json);

        let deserialized: SystemTools = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.build_tool.name, "pixi-build-rust");
        assert_eq!(deserialized.build_tool.version, "0.1.0");
        assert!(
            deserialized
                .used_tools
                .lock()
                .unwrap()
                .contains_key(&Tool::Patchelf)
        );
    }
}
