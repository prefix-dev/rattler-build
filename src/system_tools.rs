//! System tools are installed on the system (git, patchelf, install_name_tool, etc.)
use serde::{ser::SerializeMap, Deserialize, Serialize, Serializer};
use std::{cell::RefCell, collections::HashMap, path::PathBuf, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    RattlerBuild,
    Patchelf,
    InstallNameTool,
    Git,
}

#[derive(Debug)]
pub struct SystemTool {
    rattler_build_version: String,
    used_tools: RefCell<HashMap<Tool, String>>,
    found_tools: RefCell<HashMap<Tool, PathBuf>>,
}

impl Default for SystemTool {
    fn default() -> Self {
        Self {
            rattler_build_version: env!("CARGO_PKG_VERSION").to_string(),
            used_tools: RefCell::new(HashMap::new()),
            found_tools: RefCell::new(HashMap::new()),
        }
    }
}

impl SystemTool {
    pub fn new() -> Self {
        Self::default()
    }

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
            used_tools: RefCell::new(used_tools),
            found_tools: RefCell::new(HashMap::new()),
        }
    }

    pub fn find_tool(&self, tool: Tool) -> PathBuf {
        let (tool_path, found_version) = match tool {
            Tool::Patchelf => {
                let path = which::which("patchelf").expect("patchelf not found");
                // patchelf version
                let output = std::process::Command::new(&path)
                    .arg("--version")
                    .output()
                    .expect("Failed to execute command");
                let found_version = String::from_utf8_lossy(&output.stdout);

                (path, found_version.to_string())
            }
            Tool::InstallNameTool => {
                let path = which::which("install_name_tool").expect("install_name_tool not found");
                (path, "".to_string())
            }
            Tool::Git => {
                let path = which::which("git").expect("git not found");
                let output = std::process::Command::new(&path)
                    .arg("--version")
                    .output()
                    .expect("Failed to execute command");
                let found_version = String::from_utf8_lossy(&output.stdout);

                (path, found_version.to_string())
            }
            Tool::RattlerBuild => {
                let path = std::env::current_exe().expect("Failed to get current executable path");
                (path, env!("CARGO_PKG_VERSION").to_string())
            }
        };

        let found_version = found_version.trim().to_string();

        self.found_tools
            .borrow_mut()
            .insert(tool, tool_path.clone());
        let prev_version = self.used_tools.borrow().get(&tool).cloned();

        if let Some(prev_version) = prev_version {
            if prev_version != found_version {
                tracing::warn!(
                    "Found different version of patchelf: {} and {}",
                    prev_version,
                    found_version
                );
            }
        } else {
            self.used_tools.borrow_mut().insert(tool, found_version);
        }

        tool_path
    }

    pub fn call(&self, tool: Tool, args: Vec<&str>) -> Command {
        let found_tool = self.found_tools.borrow().get(&tool).cloned();
        let tool_path = if let Some(tool) = found_tool {
            tool
        } else {
            self.find_tool(tool)
        };

        let mut command = std::process::Command::new(tool_path);
        command.args(args);
        command
    }
}

impl Serialize for SystemTool {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.found_tools.borrow().len() + 1))?;
        for (tool, path) in self.used_tools.borrow().iter() {
            map.serialize_entry(tool, path)?;
        }
        map.serialize_entry(&Tool::RattlerBuild, &self.rattler_build_version)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for SystemTool {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map = HashMap::<Tool, String>::deserialize(deserializer)?;
        // remove rattler build version
        let rattler_build_version = map.remove(&Tool::RattlerBuild).unwrap_or_else(|| {
            tracing::warn!(
                "No rattler build version found in encoded system tool configuration. Using current version {}",
                env!("CARGO_PKG_VERSION"));
            env!("CARGO_PKG_VERSION").to_string()
        });

        Ok(SystemTool::from_previous_run(rattler_build_version, map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_tool() {
        let system_tool = SystemTool::new();
        let mut output = system_tool.call(Tool::Patchelf, vec!["--version"]);
        let stdout = output.output().unwrap().stdout;
        let version = String::from_utf8_lossy(&stdout).trim().to_string();

        let found_tools = system_tool.found_tools.borrow();
        assert!(found_tools.contains_key(&Tool::Patchelf));

        let used_tools = system_tool.used_tools.borrow();
        assert!(used_tools.contains_key(&Tool::Patchelf));

        assert!(used_tools.get(&Tool::Patchelf).unwrap() == &version);

        // fix versions in used tools to test deserialization
        let mut used_tools = HashMap::new();
        used_tools.insert(Tool::Patchelf, "1.0.0".to_string());
        used_tools.insert(Tool::InstallNameTool, "2.0.0".to_string());
        used_tools.insert(Tool::Git, "3.0.0".to_string());

        let system_tool = SystemTool {
            rattler_build_version: "0.0.0".to_string(),
            used_tools: RefCell::new(used_tools),
            found_tools: RefCell::new(HashMap::new()),
        };

        let json = serde_json::to_string_pretty(&system_tool).unwrap();
        insta::assert_snapshot!(json);

        let deserialized: SystemTool = serde_json::from_str(&json).unwrap();
        assert!(deserialized
            .used_tools
            .borrow()
            .contains_key(&Tool::Patchelf));
    }
}
