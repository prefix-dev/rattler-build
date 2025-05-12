//! System tools are installed on the system (git, patchelf, install_name_tool, etc.)

use rattler_conda_types::Platform;
use rattler_shell::{activation::Activator, shell};
use serde::{Deserialize, Serialize, Serializer};
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
    /// The rattler build tool itself
    #[serde(rename = "rattler-build")]
    RattlerBuild,
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
                Tool::RattlerBuild => "rattler-build".to_string(),
                Tool::Codesign => "codesign".to_string(),
                Tool::Patch => "patch".to_string(),
                Tool::Patchelf => "patchelf".to_string(),
                Tool::InstallNameTool => "install_name_tool".to_string(),
                Tool::Git => "git".to_string(),
            }
        )
    }
}

/// The system tools object is used to find and call system tools. It also keeps track of the
/// versions of the tools that are used.
#[derive(Debug, Clone)]
pub struct SystemTools {
    rattler_build_version: String,
    used_tools: Arc<Mutex<HashMap<Tool, String>>>,
    found_tools: Arc<Mutex<HashMap<Tool, PathBuf>>>,
    build_prefix: Option<PathBuf>,
}

impl Default for SystemTools {
    fn default() -> Self {
        Self {
            rattler_build_version: env!("CARGO_PKG_VERSION").to_string(),
            used_tools: Arc::new(Mutex::new(HashMap::new())),
            found_tools: Arc::new(Mutex::new(HashMap::new())),
            build_prefix: None,
        }
    }
}

impl SystemTools {
    /// Create a new system tools object
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a copy of the system tools object and add a build prefix to search for tools.
    /// Tools that are found in the build prefix are not added to the used tools list.
    pub fn with_build_prefix(&self, prefix: &Path) -> Self {
        Self {
            build_prefix: Some(prefix.to_path_buf()),
            ..self.clone()
        }
    }

    /// Create a new system tools object from a previous run so that we can warn if the versions
    /// of the tools have changed
    pub fn from_previous_run(
        rattler_build_version: String,
        used_tools: HashMap<Tool, String>,
    ) -> Self {
        if rattler_build_version != env!("CARGO_PKG_VERSION") {
            tracing::warn!(
                "Found different version of rattler build: {} and {}",
                rattler_build_version,
                env!("CARGO_PKG_VERSION")
            );
        }

        Self {
            rattler_build_version,
            used_tools: Arc::new(Mutex::new(used_tools)),
            found_tools: Arc::new(Mutex::new(HashMap::new())),
            build_prefix: None,
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
            Tool::RattlerBuild => {
                let path = std::env::current_exe().expect("Failed to get current executable path");
                (path, env!("CARGO_PKG_VERSION").to_string())
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
                    "Found different version of patchelf: {} and {}",
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
        let mut ordered_map = BTreeMap::new();
        let used_tools = self.used_tools.lock().unwrap();
        for (tool, version) in used_tools.iter() {
            ordered_map.insert(tool.to_string(), version);
        }
        ordered_map.insert(Tool::RattlerBuild.to_string(), &self.rattler_build_version);

        ordered_map.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SystemTools {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map = HashMap::<Tool, String>::deserialize(deserializer)?;
        // remove rattler build version
        let rattler_build_version = map.remove(&Tool::RattlerBuild).unwrap_or_else(|| {
            tracing::warn!(
                "No rattler build version found in encoded system tool configuration. Using current version {}",
                env!("CARGO_PKG_VERSION"));
            env!("CARGO_PKG_VERSION").to_string()
        });

        Ok(SystemTools::from_previous_run(rattler_build_version, map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn test_system_tool() {
        let system_tool = SystemTools::new();
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
            rattler_build_version: "0.0.0".to_string(),
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
}
