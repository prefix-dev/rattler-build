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
//! shared/rattler-build configuration; the embedding application stays in
//! full control of where configuration comes from.

use rattler_config::config::{ConfigBase, LoadError, MergeError};
use rattler_config::locations::ConfigLocation;

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

/// All default configuration file locations, in ascending order of precedence
/// (values from later files override values from earlier files).
///
/// This is a thin wrapper around
/// [`rattler_config::locations::config_search_paths`], the shared discovery
/// logic used by all rattler based tools. It yields, lowest precedence first:
///
/// 1. the system-wide shared configuration (`/etc/rattler/config.toml`, or
///    the `C:\ProgramData\rattler\config.toml` equivalent on Windows),
/// 2. the system-wide rattler-build configuration
///    (`/etc/rattler-build/config.toml`),
/// 3. the per-user shared configuration
///    (`$XDG_CONFIG_HOME/rattler/config.toml`, plus
///    `$RATTLER_HOME/config.toml` when the variable is set),
/// 4. the per-user rattler-build configuration: the platform config
///    directory (`$XDG_CONFIG_HOME/rattler-build/config.toml`) followed by
///    the tool home (`$RATTLER_BUILD_HOME`, defaulting to
///    `~/.rattler-build`).
///
/// Shared files may only contain the keys shared by all rattler based tools;
/// tool-specific keys in them are ignored with a warning. rattler-build's
/// own files accept the shared keys plus the [`RattlerBuildConfig`] keys.
/// Configuration in pixi's own files (`~/.pixi/config.toml`, …) is no longer
/// read: settings meant for every tool belong in the shared files.
pub fn default_config_paths() -> Vec<ConfigLocation> {
    rattler_config::locations::config_search_paths("rattler-build")
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

    let locations = candidates
        .into_iter()
        .filter(|location| location.path.is_file())
        .collect::<Vec<_>>();

    if locations.is_empty() {
        tracing::debug!("No configuration file found in any default location");
        return Ok(None);
    }

    Config::load_from_locations(locations).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::NamedChannelOrUrl;
    use std::str::FromStr;

    /// The `default_config_paths` wrapper must preserve the precedence
    /// guaranteed by the shared `locations` helper (lowest precedence
    /// first): system shared, system rattler-build, user shared, user
    /// rattler-build. We assert on the positions of the paths reported by
    /// the upstream helpers rather than depending on the real home
    /// directory.
    #[test]
    fn test_default_config_paths_ordering() {
        use rattler_config::locations::{
            shared_system_config_path, shared_user_config_paths, system_config_path,
            user_config_paths,
        };

        let paths = default_config_paths();
        let position =
            |needle: &std::path::Path| paths.iter().position(|location| location.path == needle);

        let pos_system_shared =
            position(&shared_system_config_path()).expect("system shared config present");
        let pos_system_rb = position(&system_config_path("rattler-build"))
            .expect("system rattler-build config present");

        // Within the system group, rattler-build overrides the shared file.
        assert!(
            pos_system_shared < pos_system_rb,
            "system rattler-build config must override system shared config"
        );

        // The per-user shared files come after all system files…
        if let Some(first_user_shared) =
            shared_user_config_paths().first().and_then(|p| position(p))
        {
            assert!(
                pos_system_rb < first_user_shared,
                "per-user shared config must override system configs"
            );

            // …and rattler-build's own per-user files override them.
            if let Some(first_user_rb) = user_config_paths("rattler-build")
                .first()
                .and_then(|p| position(p))
            {
                assert!(
                    first_user_shared < first_user_rb,
                    "per-user rattler-build config must override per-user shared config"
                );
            }
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
