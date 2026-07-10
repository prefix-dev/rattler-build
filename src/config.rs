//! Configuration for `rattler-build`.
//!
//! `rattler-build` shares its configuration format with
//! [pixi](https://pixi.sh) and the other rattler based tools: the common keys
//! (default channels, mirrors, S3 options, …) come from
//! [`rattler_config::config::ConfigBase`], while everything that only makes
//! sense for `rattler-build` lives in the [`RattlerBuildConfig`] extension.
//!
//! ## When configuration is loaded
//!
//! Configuration is discovered and loaded **only by the command-line
//! interface** ([`load_default_config`]), and only when no explicit
//! `--config-file` is given. On startup the CLI logs its version and the
//! files it loaded, so config resolution is easy to trace.
//!
//! Programmatic/library consumers of `rattler-build` — including pixi via
//! `rattler_build_core`, and the Python bindings — never load configuration
//! implicitly. They must construct a [`Config`] themselves (e.g.
//! [`Config::default`] or [`ConfigBase::load_from_files`]) and pass it in.
//! This keeps library use free of surprising reads of the user's global
//! pixi/rattler-build configuration; the embedding application stays in full
//! control of where configuration comes from.

use std::path::PathBuf;

use rattler_config::config::{ConfigBase, LoadError, MergeError};

/// rattler-build specific configuration keys.
/// Extend this struct to add configuration that only makes sense for
/// rattler-build.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RattlerBuildConfig {}

impl rattler_config::config::Config for RattlerBuildConfig {
    fn merge_config(self, _other: &Self) -> Result<Self, MergeError> {
        // There are no rattler-build specific keys yet, so there is nothing
        // to merge. `validate`, `keys` and `is_default` use the trait's
        // default implementations.
        Ok(self)
    }
}

/// The `rattler-build` configuration: the configuration shared with pixi and
/// other rattler based tools, extended with rattler-build specific keys.
pub type Config = ConfigBase<RattlerBuildConfig>;

/// The tools whose configuration `rattler-build` reads, in ascending order of
/// precedence: pixi's configuration is picked up automatically and can be
/// overridden by rattler-build specific files.
const CONFIG_TOOLS: &[&str] = &["pixi", "rattler-build"];

/// All default configuration file locations, in ascending order of precedence
/// (values from later files override values from earlier files).
///
/// This is a thin wrapper around
/// [`rattler_config::locations::config_search_paths`], the shared discovery
/// logic used by all rattler based tools. For the tools
/// `["pixi", "rattler-build"]` it yields, lowest precedence first:
///
/// 1. the system-wide configuration of every tool
///    (`/etc/pixi/config.toml`, `/etc/rattler-build/config.toml`, or the
///    `C:\ProgramData\<tool>\config.toml` equivalents on Windows),
/// 2. the per-user configuration of every tool: the platform config directory
///    (`$XDG_CONFIG_HOME/<tool>/config.toml`) followed by the tool home
///    (`$PIXI_HOME` / `$RATTLER_BUILD_HOME`, defaulting to `~/.pixi` /
///    `~/.rattler-build`).
///
/// Within each group the tools are ordered as listed, so rattler-build's
/// configuration overrides pixi's.
pub fn default_config_paths() -> Vec<PathBuf> {
    rattler_config::locations::config_search_paths(CONFIG_TOOLS)
}

/// Load the configuration from the default locations (see
/// [`default_config_paths`]), merging all files that exist. Files later in
/// the list override values from earlier files.
///
/// Returns `Ok(None)` if none of the default configuration files exist.
///
/// This is the command-line interface's discovery entry point. It is
/// intentionally **not** called by any library code path: programmatic
/// consumers construct and pass their own [`Config`] instead of having one
/// discovered from the user's environment (see the module docs).
///
/// The full candidate list (in precedence order) is logged at debug level so
/// that `-v` runs can explain why a particular file was or was not picked up;
/// the CLI separately logs the files that were actually loaded at the default
/// level on startup.
pub fn load_default_config() -> Result<Option<Config>, LoadError> {
    let candidates = default_config_paths();
    tracing::debug!("Configuration search paths (lowest precedence first): {candidates:?}");

    let paths = candidates
        .into_iter()
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();

    if paths.is_empty() {
        tracing::debug!("No configuration file found in any default location");
        return Ok(None);
    }

    Config::load_from_files(&paths).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::NamedChannelOrUrl;
    use std::str::FromStr;

    /// The `default_config_paths` wrapper must preserve the precedence
    /// guaranteed by the shared `locations` helper (lowest precedence first):
    /// all system-wide files come before all per-user files, and within each
    /// group pixi's file is overridden by rattler-build's. We assert on the
    /// positions of the paths reported by the upstream helpers rather than
    /// depending on the real home directory.
    #[test]
    fn test_default_config_paths_ordering() {
        use rattler_config::locations::{system_config_path, user_config_paths};

        let paths = default_config_paths();
        let position = |needle: &std::path::Path| paths.iter().position(|p| p == needle);

        let system_pixi = system_config_path("pixi");
        let system_rb = system_config_path("rattler-build");
        let pos_system_pixi = position(&system_pixi).expect("system pixi config present");
        let pos_system_rb = position(&system_rb).expect("system rattler-build config present");

        // Within the system group, rattler-build overrides pixi.
        assert!(
            pos_system_pixi < pos_system_rb,
            "system rattler-build config must override system pixi config"
        );

        // All system-wide files come before all per-user files.
        if let Some(first_user) = user_config_paths("pixi").first().and_then(|p| position(p)) {
            assert!(
                pos_system_rb < first_user,
                "system configs must come before per-user configs"
            );
        }

        // Within the per-user group, rattler-build overrides pixi.
        if let (Some(last_user_pixi), Some(first_user_rb)) = (
            user_config_paths("pixi").last().and_then(|p| position(p)),
            user_config_paths("rattler-build")
                .first()
                .and_then(|p| position(p)),
        ) {
            assert!(
                last_user_pixi < first_user_rb,
                "per-user rattler-build config must override per-user pixi config"
            );
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
