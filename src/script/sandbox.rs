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

/// Configuration for the sandbox
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SandboxConfiguration {
    allow_network: bool,
    read: Vec<PathBuf>,
    read_execute: Vec<PathBuf>,
    read_write: Vec<PathBuf>,
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

    #[cfg(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        target_os = "macos"
    ))]
    /// Get the list of exceptions for the sandbox
    pub fn exceptions(&self) -> Vec<rattler_sandbox::Exception> {
        let mut exceptions = Vec::new();
        if self.allow_network {
            exceptions.push(rattler_sandbox::Exception::Networking);
        }

        for path in &self.read {
            exceptions.push(rattler_sandbox::Exception::Read(
                path.to_string_lossy().to_string(),
            ));
        }

        for path in &self.read_execute {
            exceptions.push(rattler_sandbox::Exception::ExecuteAndRead(
                path.to_string_lossy().to_string(),
            ));
        }

        for path in &self.read_write {
            exceptions.push(rattler_sandbox::Exception::ReadAndWrite(
                path.to_string_lossy().to_string(),
            ));
        }

        exceptions
    }
}

impl From<SandboxArguments> for Option<SandboxConfiguration> {
    fn from(args: SandboxArguments) -> Self {
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
