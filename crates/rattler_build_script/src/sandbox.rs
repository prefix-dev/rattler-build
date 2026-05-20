//! Sandbox configuration for the build script
// enable only on linux-64, linux-aarch64, and macos
use std::{
    fmt::{Display, Formatter},
    path::{Path, PathBuf},
};

use clap::Parser;
use serde::{Deserialize, Serialize};

/// CLI argument parser for the sandbox
#[derive(Debug, Parser, Clone, Default)]
pub struct SandboxArguments {
    /// Enable the sandbox
    #[clap(long, action, help_heading = "Sandbox arguments")]
    pub sandbox: bool,

    /// Path to a sandbox config file. Implies `--sandbox`.
    #[clap(long, help_heading = "Sandbox arguments", value_name = "PATH")]
    pub sandbox_config: Option<PathBuf>,

    /// Allow network access during build (default: false if sandbox is enabled)
    #[clap(long, action, help_heading = "Sandbox arguments")]
    pub allow_network: bool,

    /// Allow read access to the specified paths
    #[clap(long, help_heading = "Sandbox arguments")]
    pub allow_read: Vec<PathBuf>,

    /// Allow read and execute access to the specified paths
    #[clap(long, help_heading = "Sandbox arguments")]
    pub allow_read_execute: Vec<PathBuf>,

    /// Allow read and write access to the specified paths
    #[clap(long, help_heading = "Sandbox arguments")]
    pub allow_read_write: Vec<PathBuf>,

    /// Overwrite the default sandbox configuration
    #[clap(long, action, help_heading = "Sandbox arguments")]
    pub overwrite_default_sandbox_config: bool,
}

impl SandboxArguments {
    /// Returns true if either `--sandbox` or `--sandbox-config` was given.
    pub fn is_enabled(&self) -> bool {
        self.sandbox || self.sandbox_config.is_some()
    }
}

/// Recipe-declared sandbox escape request.
///
/// Applied additively on top of the host's resolved [`SandboxConfiguration`]: the recipe
/// can grant *more* access (e.g. network, extra paths) but cannot tighten the host's policy.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SandboxRequest {
    /// Request network access for the build
    #[serde(default, skip_serializing_if = "is_false")]
    pub network: bool,

    /// Additional read-only paths
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<PathBuf>,

    /// Additional read+execute paths
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_execute: Vec<PathBuf>,

    /// Additional read+write paths
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_write: Vec<PathBuf>,

    /// Human-readable reason surfaced in logs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl SandboxRequest {
    /// Returns true if no escape was requested.
    pub fn is_empty(&self) -> bool {
        !self.network
            && self.read.is_empty()
            && self.read_execute.is_empty()
            && self.read_write.is_empty()
            && self.reason.is_none()
    }
}

/// Configuration for the sandbox
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SandboxConfiguration {
    allow_network: bool,
    read: Vec<PathBuf>,
    read_execute: Vec<PathBuf>,
    read_write: Vec<PathBuf>,
}

impl SandboxConfiguration {
    /// Merge an additive [`SandboxRequest`] into this configuration.
    ///
    /// The recipe can only *expand* permissions — `allow_network` becomes true if the
    /// request asks for it, and path lists are appended. The configuration's existing
    /// permissions are never reduced.
    pub fn merge_request(&mut self, request: &SandboxRequest) {
        if request.network {
            self.allow_network = true;
        }
        self.read.extend(request.read.iter().cloned());
        self.read_execute
            .extend(request.read_execute.iter().cloned());
        self.read_write.extend(request.read_write.iter().cloned());
    }
}

impl Display for SandboxConfiguration {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} Sandbox Configuration", console::Emoji("🛡️", " "))?;
        writeln!(
            f,
            "Network Access: {}",
            if self.allow_network {
                console::Emoji("✅", " ")
            } else {
                console::Emoji("❌", " ")
            }
        )?;

        writeln!(f, "\n{} Read-only paths:", console::Emoji("📁 ", ""))?;
        for path in &self.read {
            writeln!(f, "  - {}", path.display())?;
        }

        writeln!(f, "\n{} Read-execute paths:", console::Emoji("📂 ", ""))?;
        for path in &self.read_execute {
            writeln!(f, "  - {}", path.display())?;
        }

        writeln!(f, "\n{} Read-write paths:", console::Emoji("📝 ", ""))?;
        for path in &self.read_write {
            writeln!(f, "  - {}", path.display())?;
        }

        Ok(())
    }
}

impl SandboxConfiguration {
    /// Create a default sandbox configuration for macOS
    pub fn for_macos() -> Self {
        let read_execute = vec!["/bin/", "/usr/bin/"]
            .into_iter()
            .map(Into::into)
            .collect();

        let mut read_write = Vec::new();
        // Allow writing to temp folders
        read_write.push("/tmp".into());
        read_write.push("/var/tmp".into());
        let temp_folder = std::env::var("TMPDIR").ok();
        if let Some(temp_folder) = temp_folder {
            read_write.push(temp_folder.into());
        }

        Self {
            allow_network: false,
            read: vec!["/".into()],
            read_execute,
            read_write,
        }
    }

    /// Default configuration for Linux
    pub fn for_linux() -> Self {
        let read_execute = vec![
            // System binaries
            "/bin/",
            "/usr/bin/",
            // Definitely needed for `ld` but maybe we should make it more specific
            // to only allow e.g. `/lib/ld-linux-x86-64.so.2`?
            "/lib64",
            "/usr/lib64",
            "/lib",
            "/usr/lib",
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        // For now, I am not adding `/sbin` and `/usr/sbin` to the read_execute list as
        // these commands should generally not be needed during the build process.

        let mut read_write: Vec<PathBuf> = vec![
            // Temp directories
            "/tmp", "/var/tmp",
        ]
        .into_iter()
        .map(Into::into)
        .collect();

        let temp_folder = std::env::var("TMPDIR").ok();
        if let Some(temp_folder) = temp_folder {
            read_write.push(temp_folder.into());
        }

        Self {
            allow_network: false,
            read: vec!["/".into()],
            read_execute,
            read_write,
        }
    }

    /// Add the current working directory to the list of allowed paths
    /// Adds the parent directory of the current working directory to the list of allowed paths
    /// for read_execute and read_write
    pub fn with_cwd(&self, cwd: &Path) -> Self {
        let mut read_execute = self.read_execute.clone();
        if let Some(parent) = cwd.parent() {
            read_execute.push(parent.to_path_buf());
        }

        let mut read_write = self.read_write.clone();
        if let Some(parent) = cwd.parent() {
            read_write.push(parent.to_path_buf());
        }

        Self {
            allow_network: self.allow_network,
            read: self.read.clone(),
            read_execute,
            read_write,
        }
    }

    /// Convert the sandbox configuration to command-line arguments for the rattler-sandbox executable
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        if self.allow_network {
            args.push("--network".to_string());
        }

        for path in &self.read {
            args.push("--fs-read".to_string());
            args.push(path.to_string_lossy().to_string());
        }

        for path in &self.read_execute {
            args.push("--fs-exec-and-read".to_string());
            args.push(path.to_string_lossy().to_string());
        }

        for path in &self.read_write {
            args.push("--fs-write-and-read".to_string());
            args.push(path.to_string_lossy().to_string());
        }

        args
    }
}

impl SandboxArguments {
    /// Resolve these CLI arguments into a sandbox configuration.
    ///
    /// Returns `Ok(None)` if the sandbox was not enabled (neither `--sandbox` nor
    /// `--sandbox-config` was given). Returns `Err` if a config file was specified
    /// but could not be loaded or parsed.
    #[cfg(feature = "execution")]
    pub fn try_into_configuration(self) -> std::io::Result<Option<SandboxConfiguration>> {
        if !self.is_enabled() {
            return Ok(None);
        }

        // Determine the host kind for picking the built-in baseline.
        let host_kind = if cfg!(target_os = "linux") {
            PlatformKind::Linux
        } else if cfg!(target_os = "macos") {
            PlatformKind::Osx
        } else {
            PlatformKind::Other
        };

        let mut result = if let Some(path) = &self.sandbox_config {
            // File-driven baseline.
            let file = SandboxConfigFile::from_path(path)?;
            file.resolve_for_kind(host_kind).unwrap_or_default()
        } else if !self.overwrite_default_sandbox_config {
            // Built-in baseline.
            match host_kind {
                PlatformKind::Linux => SandboxConfiguration::for_linux(),
                PlatformKind::Osx => SandboxConfiguration::for_macos(),
                PlatformKind::Other => SandboxConfiguration::default(),
            }
        } else {
            SandboxConfiguration::default()
        };

        for path in self.allow_read {
            result.read.push(path);
        }
        for path in self.allow_read_execute {
            result.read_execute.push(path);
        }
        for path in self.allow_read_write {
            result.read_write.push(path);
        }

        // CLI flag overrides config-file network policy.
        if self.allow_network {
            result.allow_network = true;
        }

        Ok(Some(result))
    }
}

impl From<SandboxArguments> for Option<SandboxConfiguration> {
    /// Infallible conversion. A failed config-file load is surfaced as `None` and
    /// a warning printed to stderr — prefer [`SandboxArguments::try_into_configuration`]
    /// when error handling matters.
    fn from(args: SandboxArguments) -> Self {
        #[cfg(feature = "execution")]
        {
            match args.try_into_configuration() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("warning: failed to load sandbox config: {e}");
                    None
                }
            }
        }
        #[cfg(not(feature = "execution"))]
        {
            if !args.sandbox {
                return None;
            }
            let mut result = if !args.overwrite_default_sandbox_config {
                #[cfg(target_os = "linux")]
                let default = SandboxConfiguration::for_linux();
                #[cfg(target_os = "macos")]
                let default = SandboxConfiguration::for_macos();
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                let default = SandboxConfiguration::default();
                default
            } else {
                SandboxConfiguration::default()
            };
            for path in args.allow_read {
                result.read.push(path);
            }
            for path in args.allow_read_execute {
                result.read_execute.push(path);
            }
            for path in args.allow_read_write {
                result.read_write.push(path);
            }
            result.allow_network = args.allow_network;
            Some(result)
        }
    }
}

/// Lightweight platform classification used by the sandbox config loader.
///
/// We deliberately classify only the families the sandbox supports. Other
/// targets fall through to [`PlatformKind::Other`] and skip per-platform
/// overrides.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PlatformKind {
    /// Any Linux target.
    Linux,
    /// Any macOS target.
    Osx,
    /// Anything else (e.g. Windows). Per-platform overrides are not applied.
    Other,
}

/// Top-level schema for the host-side sandbox config file
/// (`~/.rattler-build/sandbox.yaml` or `--sandbox-config <path>`).
///
/// Each platform block extends the built-in baseline ([`SandboxConfiguration::for_linux`]
/// / [`SandboxConfiguration::for_macos`]) unless [`PlatformOverride::replace`] is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfigFile {
    /// Schema version. Currently `1`. Reserved for future evolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,

    /// Overrides for Linux targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<PlatformOverride>,

    /// Overrides for macOS targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub osx: Option<PlatformOverride>,
}

/// A per-platform override block applied to the built-in baseline.
///
/// Path lists are appended to the baseline by default. When [`Self::replace`]
/// is set, the baseline is discarded entirely and only the fields here are used.
/// [`Self::network`] always overrides the baseline when present.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformOverride {
    /// If `true`, replace the baseline entirely instead of extending it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub replace: bool,

    /// If set, override the baseline's network policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,

    /// Additional read-only paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<PathBuf>,

    /// Additional read+execute paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_execute: Vec<PathBuf>,

    /// Additional read+write paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_write: Vec<PathBuf>,
}

impl SandboxConfigFile {
    /// Apply the per-platform override (if any) on top of `baseline`.
    pub fn apply(
        &self,
        kind: PlatformKind,
        baseline: SandboxConfiguration,
    ) -> SandboxConfiguration {
        let block = match kind {
            PlatformKind::Linux => self.linux.as_ref(),
            PlatformKind::Osx => self.osx.as_ref(),
            PlatformKind::Other => None,
        };
        match block {
            None => baseline,
            Some(o) if o.replace => SandboxConfiguration {
                allow_network: o.network.unwrap_or(false),
                read: o.read.clone(),
                read_execute: o.read_execute.clone(),
                read_write: o.read_write.clone(),
            },
            Some(o) => {
                let mut cfg = baseline;
                if let Some(n) = o.network {
                    cfg.allow_network = n;
                }
                cfg.read.extend(o.read.iter().cloned());
                cfg.read_execute.extend(o.read_execute.iter().cloned());
                cfg.read_write.extend(o.read_write.iter().cloned());
                cfg
            }
        }
    }

    /// Resolve a sandbox configuration for the given target.
    ///
    /// `linux` targets start from [`SandboxConfiguration::for_linux`], `osx` from
    /// [`SandboxConfiguration::for_macos`]. Other targets return `None` — the sandbox
    /// is not supported there.
    pub fn resolve_for_kind(&self, kind: PlatformKind) -> Option<SandboxConfiguration> {
        let baseline = match kind {
            PlatformKind::Linux => SandboxConfiguration::for_linux(),
            PlatformKind::Osx => SandboxConfiguration::for_macos(),
            PlatformKind::Other => return None,
        };
        Some(self.apply(kind, baseline))
    }
}

#[cfg(feature = "execution")]
impl SandboxConfigFile {
    /// Load a [`SandboxConfigFile`] from a YAML file on disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let text = fs_err::read_to_string(path)?;
        serde_yaml::from_str(&text).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid sandbox config at {}: {e}", path.display()),
            )
        })
    }

    /// Resolve a sandbox configuration for `platform` from this config file.
    ///
    /// Returns `None` for platforms where the sandbox is not supported (e.g. Windows).
    pub fn resolve_for(
        &self,
        platform: rattler_conda_types::Platform,
    ) -> Option<SandboxConfiguration> {
        let kind = if platform.is_linux() {
            PlatformKind::Linux
        } else if platform.is_osx() {
            PlatformKind::Osx
        } else {
            PlatformKind::Other
        };
        self.resolve_for_kind(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_extends_linux_baseline() {
        let yaml = r#"
version: 1
linux:
  read_execute:
    - /opt/intel/oneapi/bin
  read_write:
    - /home/user/.cache/sccache
"#;
        let cfg: SandboxConfigFile = serde_yaml::from_str(yaml).unwrap();
        let resolved = cfg.resolve_for_kind(PlatformKind::Linux).unwrap();

        assert!(
            !resolved.allow_network,
            "extension keeps baseline network policy"
        );
        assert!(
            resolved
                .read_execute
                .contains(&PathBuf::from("/opt/intel/oneapi/bin"))
        );
        // baseline paths are preserved
        assert!(resolved.read_execute.contains(&PathBuf::from("/bin/")));
        assert!(
            resolved
                .read_write
                .contains(&PathBuf::from("/home/user/.cache/sccache"))
        );
        assert!(resolved.read_write.contains(&PathBuf::from("/tmp")));
    }

    #[test]
    fn config_file_replace_drops_baseline() {
        let yaml = r#"
linux:
  replace: true
  network: true
  read_execute:
    - /custom/bin
"#;
        let cfg: SandboxConfigFile = serde_yaml::from_str(yaml).unwrap();
        let resolved = cfg.resolve_for_kind(PlatformKind::Linux).unwrap();

        assert!(resolved.allow_network);
        assert_eq!(resolved.read_execute, vec![PathBuf::from("/custom/bin")]);
        assert!(resolved.read.is_empty());
        assert!(resolved.read_write.is_empty());
    }

    #[test]
    fn config_file_network_override() {
        let yaml = r#"
osx:
  network: true
"#;
        let cfg: SandboxConfigFile = serde_yaml::from_str(yaml).unwrap();
        let resolved = cfg.resolve_for_kind(PlatformKind::Osx).unwrap();

        assert!(resolved.allow_network);
        // Baseline read paths are preserved.
        assert!(resolved.read_execute.contains(&PathBuf::from("/bin/")));
    }

    #[test]
    fn config_file_other_platform_returns_none() {
        let cfg = SandboxConfigFile::default();
        assert!(cfg.resolve_for_kind(PlatformKind::Other).is_none());
    }

    #[test]
    fn config_file_rejects_unknown_top_level_field() {
        let yaml = r#"
version: 1
windows:
  read_write: []
"#;
        let err = serde_yaml::from_str::<SandboxConfigFile>(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("windows"),
            "expected unknown-field error: {msg}"
        );
    }

    #[cfg(feature = "execution")]
    #[test]
    fn args_neither_flag_disables_sandbox() {
        let args = SandboxArguments::default();
        assert!(!args.is_enabled());
        assert!(args.try_into_configuration().unwrap().is_none());
    }

    #[cfg(feature = "execution")]
    #[test]
    fn args_sandbox_config_path_enables_sandbox() {
        use std::io::Write;

        let yaml = "version: 1\nlinux:\n  read_write: [/custom]\nosx:\n  read_write: [/custom]\n";
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(yaml.as_bytes()).unwrap();

        let args = SandboxArguments {
            sandbox_config: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        assert!(args.is_enabled());

        let cfg = args.try_into_configuration().unwrap();
        // On supported host platforms the config resolves; on others it falls
        // through to an empty configuration but the sandbox is still "enabled".
        assert!(cfg.is_some());
    }

    #[cfg(feature = "execution")]
    #[test]
    fn args_bad_config_path_returns_error() {
        let args = SandboxArguments {
            sandbox_config: Some(std::path::PathBuf::from("/nope/does/not/exist.yaml")),
            ..Default::default()
        };
        assert!(args.try_into_configuration().is_err());
    }

    #[cfg(feature = "execution")]
    #[test]
    fn from_path_roundtrips_through_disk() {
        use std::io::Write;

        let yaml = r#"
version: 1
linux:
  read_write:
    - /custom/cache
"#;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(yaml.as_bytes()).unwrap();
        let cfg = SandboxConfigFile::from_path(tmp.path()).unwrap();
        let resolved = cfg.resolve_for_kind(PlatformKind::Linux).unwrap();
        assert!(
            resolved
                .read_write
                .contains(&PathBuf::from("/custom/cache"))
        );
    }
}
