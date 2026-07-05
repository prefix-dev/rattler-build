//! Configuration for `rattler-build`.
//!
//! `rattler-build` shares its configuration format with
//! [pixi](https://pixi.sh) and the other rattler based tools: the common keys
//! (default channels, mirrors, S3 options, …) come from
//! [`rattler_config::config::ConfigBase`], while everything that only makes
//! sense for `rattler-build` lives in the [`RattlerBuildConfig`] extension.
//!
//! When no `--config-file` is passed on the command line, configuration is
//! discovered from the standard pixi locations as well as `rattler-build`'s
//! own configuration paths (see [`default_config_paths`]).

use std::path::PathBuf;

use rattler_config::config::{ConfigBase, LoadError, MergeError, ValidationError};

/// rattler-build specific configuration keys.
/// Extend this struct to add configuration that only makes sense for
/// rattler-build.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RattlerBuildConfig {}

impl rattler_config::config::Config for RattlerBuildConfig {
    fn get_extension_name(&self) -> String {
        "rattler-build".to_string()
    }

    fn merge_config(self, _other: &Self) -> Result<Self, MergeError> {
        // There are no rattler-build specific keys yet, so there is nothing
        // to merge.
        Ok(self)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }

    fn keys(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The `rattler-build` configuration: the configuration shared with pixi and
/// other rattler based tools, extended with rattler-build specific keys.
pub type Config = ConfigBase<RattlerBuildConfig>;

/// The file name of the configuration file.
const CONFIG_FILE: &str = "config.toml";

/// The directory name used by pixi inside the (XDG) config directory.
const PIXI_CONFIG_DIR: &str = "pixi";

/// The directory name used by rattler-build inside the (XDG) config
/// directory.
const RATTLER_BUILD_CONFIG_DIR: &str = "rattler-build";

/// Returns the path to the system-wide pixi configuration file.
///
/// This mirrors pixi's `config_path_system`.
pub fn pixi_config_path_system() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base_path = PathBuf::from("C:\\ProgramData");
    #[cfg(not(target_os = "windows"))]
    let base_path = PathBuf::from("/etc");

    base_path.join(PIXI_CONFIG_DIR).join(CONFIG_FILE)
}

/// Get the pixi home directory, defaulting to `$HOME/.pixi`.
///
/// It may be overridden by the `PIXI_HOME` environment variable. This mirrors
/// pixi's `pixi_home`.
fn pixi_home() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PIXI_HOME") {
        Some(PathBuf::from(path))
    } else {
        dirs::home_dir().map(|path| path.join(".pixi"))
    }
}

/// Get the rattler-build home directory, defaulting to
/// `$HOME/.rattler-build`.
///
/// It may be overridden by the `RATTLER_BUILD_HOME` environment variable.
fn rattler_build_home() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RATTLER_BUILD_HOME") {
        Some(PathBuf::from(path))
    } else {
        dirs::home_dir().map(|path| path.join(".rattler-build"))
    }
}

/// The base directories in which tools look for a `<tool>/config.toml`.
fn config_base_dirs() -> Vec<PathBuf> {
    vec![
        // On macOS, also honor the XDG_CONFIG_HOME directory, although it is
        // not a standard there and not set by default (mirrors pixi).
        #[cfg(target_os = "macos")]
        std::env::var("XDG_CONFIG_HOME").ok().map(PathBuf::from),
        dirs::config_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Compute the candidate configuration file paths for a tool, in ascending
/// order of precedence (later paths override earlier ones).
fn tool_config_paths(
    config_base_dirs: Vec<PathBuf>,
    tool_home: Option<PathBuf>,
    config_dir_name: &str,
) -> Vec<PathBuf> {
    config_base_dirs
        .into_iter()
        .map(|d| d.join(config_dir_name).join(CONFIG_FILE))
        .chain(tool_home.map(|d| d.join(CONFIG_FILE)))
        .collect()
}

/// Returns the path(s) to the global pixi configuration files, in ascending
/// order of precedence.
///
/// This mirrors pixi's `config_path_global`.
pub fn pixi_config_paths_global() -> Vec<PathBuf> {
    tool_config_paths(config_base_dirs(), pixi_home(), PIXI_CONFIG_DIR)
}

/// Returns the path(s) to rattler-build's own configuration files, in
/// ascending order of precedence.
pub fn rattler_build_config_paths() -> Vec<PathBuf> {
    tool_config_paths(
        config_base_dirs(),
        rattler_build_home(),
        RATTLER_BUILD_CONFIG_DIR,
    )
}

/// All default configuration file locations, in ascending order of precedence
/// (values from later files override values from earlier files):
///
/// 1. the system-wide pixi configuration (`/etc/pixi/config.toml`, or
///    `C:\ProgramData\pixi\config.toml` on Windows),
/// 2. pixi's global configuration (`$XDG_CONFIG_HOME/pixi/config.toml` /
///    the platform config directory, and `$PIXI_HOME/config.toml` defaulting
///    to `~/.pixi/config.toml`),
/// 3. rattler-build's own configuration
///    (`$XDG_CONFIG_HOME/rattler-build/config.toml` / the platform config
///    directory, and `$RATTLER_BUILD_HOME/config.toml` defaulting to
///    `~/.rattler-build/config.toml`).
pub fn default_config_paths() -> Vec<PathBuf> {
    let mut paths = vec![pixi_config_path_system()];
    paths.extend(pixi_config_paths_global());
    paths.extend(rattler_build_config_paths());
    paths
}

/// Load the configuration from the default locations (see
/// [`default_config_paths`]), merging all files that exist. Files later in
/// the list override values from earlier files.
///
/// Returns `Ok(None)` if none of the default configuration files exist.
pub fn load_default_config() -> Result<Option<Config>, LoadError> {
    let paths = default_config_paths()
        .into_iter()
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();

    if paths.is_empty() {
        return Ok(None);
    }

    tracing::debug!("Loading configuration from: {:?}", paths);
    Config::load_from_files(&paths).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::NamedChannelOrUrl;
    use std::str::FromStr;

    #[test]
    fn test_tool_config_paths_ordering() {
        let paths = tool_config_paths(
            vec![PathBuf::from("/xdg-config"), PathBuf::from("/config")],
            Some(PathBuf::from("/home/.pixi")),
            "pixi",
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/xdg-config/pixi/config.toml"),
                PathBuf::from("/config/pixi/config.toml"),
                PathBuf::from("/home/.pixi/config.toml"),
            ]
        );
    }

    #[test]
    fn test_tool_config_paths_without_home() {
        let paths = tool_config_paths(vec![PathBuf::from("/config")], None, "rattler-build");
        assert_eq!(
            paths,
            vec![PathBuf::from("/config/rattler-build/config.toml")]
        );
    }

    #[test]
    fn test_default_config_paths_ordering() {
        let paths = default_config_paths();

        // The system-wide pixi config always comes first.
        assert_eq!(paths[0], pixi_config_path_system());

        // All pixi paths come before all rattler-build paths so that
        // rattler-build specific configuration takes precedence.
        let last_pixi = paths.iter().rposition(|p| {
            p.parent()
                .is_some_and(|p| p.ends_with(".pixi") || p.ends_with("pixi"))
        });
        let first_rattler_build = paths.iter().position(|p| {
            p.parent()
                .is_some_and(|p| p.ends_with(".rattler-build") || p.ends_with("rattler-build"))
        });
        if let (Some(last_pixi), Some(first_rattler_build)) = (last_pixi, first_rattler_build) {
            assert!(last_pixi < first_rattler_build);
        }
    }

    #[test]
    fn test_load_from_files_later_files_win() {
        let dir = tempfile::tempdir().unwrap();
        let low = dir.path().join("low.toml");
        let high = dir.path().join("high.toml");
        fs_err::write(
            &low,
            "default-channels = [\"conda-forge\"]\ntls-no-verify = true\n",
        )
        .unwrap();
        fs_err::write(&high, "default-channels = [\"bioconda\"]\n").unwrap();

        let config = Config::load_from_files([&low, &high]).unwrap();

        // The value from the later file wins…
        assert_eq!(
            config.default_channels,
            Some(vec![NamedChannelOrUrl::from_str("bioconda").unwrap()])
        );
        // …while values only present in the earlier file are kept.
        assert_eq!(config.tls_no_verify, Some(true));
    }

    #[test]
    fn test_extension_is_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs_err::write(&path, "default-channels = [\"conda-forge\"]\n").unwrap();

        let config = Config::load_from_files([&path]).unwrap();
        assert_eq!(config.extensions, RattlerBuildConfig::default());
    }
}
