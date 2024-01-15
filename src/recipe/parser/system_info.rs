use serde::{Deserialize, Serialize};

/// The system information for the rendered recipe.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemInfo {
    /// `rattler-build` version.
    rattler_build_version: String,
    /// Git version.
    git_version: Option<String>,
    /// `patchelf` version (Linux).
    patchelf_version: Option<String>,
    /// `install_name_tool` version (macOS).
    install_name_tool_version: Option<String>,
}

impl SystemInfo {
    /// Collects the system information.
    #[cfg(not(test))]
    pub fn new() -> Self {
        let mut system_info = SystemInfo {
            rattler_build_version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        };
        system_info.git_version = Self::run_os_command("git", "version");
        if cfg!(target_os = "linux") {
            system_info.patchelf_version = Self::run_os_command("patchelf", "--version");
        } else if cfg!(target_os = "macos") {
            system_info.install_name_tool_version =
                Self::run_os_command("install_name_tool", "--version");
        }
        system_info
    }

    /// Returns dummy information for tests.
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            rattler_build_version: String::from("test"),
            git_version: Some(String::from("2.43.0")),
            patchelf_version: Some(String::from("0.18.0")),
            install_name_tool_version: Some(String::from("0.0.0")),
        }
    }

    /// Runs the given OS command and returns the result.
    #[cfg(not(test))]
    fn run_os_command(bin: &str, arg: &str) -> Option<String> {
        match std::process::Command::new(bin).arg(arg).output() {
            Ok(output) => {
                if output.status.success() {
                    String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .to_string()
                        .split_whitespace()
                        .last()
                        .map(String::from)
                } else {
                    tracing::error!(
                        "calling {bin} failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    None
                }
            }
            Err(e) => {
                tracing::error!("calling git failed: {e}");
                None
            }
        }
    }
}
