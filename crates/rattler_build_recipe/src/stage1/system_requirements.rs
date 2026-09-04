use serde::{Deserialize, Serialize};

/// Evaluated minimum system requirements for a package output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemRequirements {
    /// Minimum Linux kernel version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<String>,
    /// Minimum macOS version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<String>,
    /// Minimum CUDA driver version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda: Option<String>,
    /// Minimum libc version (Pixi scalar form, which means glibc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<String>,
    /// Minimum glibc version; an alias for scalar `libc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glibc: Option<String>,
    /// Required architecture specification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archspec: Option<String>,
}

impl SystemRequirements {
    /// Returns true when no system requirement was declared.
    pub fn is_empty(&self) -> bool {
        self.linux.is_none()
            && self.macos.is_none()
            && self.cuda.is_none()
            && self.libc.is_none()
            && self.glibc.is_none()
            && self.archspec.is_none()
    }
}
