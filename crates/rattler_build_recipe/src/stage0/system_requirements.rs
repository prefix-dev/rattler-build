use serde::{Deserialize, Serialize};

use super::Value;

/// Minimum virtual-package versions required by a package output.
///
/// Keys mirror Pixi's system requirements. `glibc` is accepted as a convenient
/// alias for Pixi's scalar `libc` form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemRequirements {
    /// Minimum Linux kernel version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<Value<String>>,
    /// Minimum macOS version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<Value<String>>,
    /// Minimum CUDA driver version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda: Option<Value<String>>,
    /// Minimum libc version (Pixi scalar form, which means glibc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libc: Option<Value<String>>,
    /// Minimum glibc version; an alias for scalar `libc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glibc: Option<Value<String>>,
    /// Required architecture specification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archspec: Option<Value<String>>,
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

    /// Collect variables used by templated requirement values.
    pub fn used_variables(&self) -> Vec<String> {
        let mut variables = Vec::new();
        for value in [
            &self.linux,
            &self.macos,
            &self.cuda,
            &self.libc,
            &self.glibc,
            &self.archspec,
        ]
        .into_iter()
        .flatten()
        {
            variables.extend(value.used_variables());
        }
        variables.sort();
        variables.dedup();
        variables
    }
}
