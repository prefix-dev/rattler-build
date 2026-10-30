use jiff::Timestamp;
use miette::{Context, IntoDiagnostic};
use rattler_config::config::exclude_newer::ExcludeNewerConfig;
use rattler_solve::ExcludeNewer;

/// Explicit policies replace the configuration, including its exemptions.
pub(crate) fn resolve(
    config: Option<&ExcludeNewerConfig>,
    explicit: Option<ExcludeNewer>,
) -> miette::Result<Option<ExcludeNewer>> {
    if explicit.is_some() {
        return Ok(explicit);
    }
    config
        .map(|config| resolve_config(config, Timestamp::now()))
        .transpose()
}

/// Converts the configuration into a policy, resolving a duration cutoff
/// relative to `now`. The cutoffs and exemptions are parsed and validated by
/// `rattler_config` when the configuration is loaded.
fn resolve_config(config: &ExcludeNewerConfig, now: Timestamp) -> miette::Result<ExcludeNewer> {
    config
        .to_exclude_newer_with_now(now)
        .into_diagnostic()
        .wrap_err("invalid exclude-newer configuration")
}

#[cfg(test)]
mod tests {
    use rattler_conda_types::{MatchSpec, NamelessMatchSpec};

    use super::*;
    use crate::config::Config;

    fn config(exclude_newer: &str) -> ExcludeNewerConfig {
        let (config, _) =
            Config::from_toml_str(&format!("exclude-newer = {exclude_newer}")).unwrap();
        config.exclude_newer.clone().unwrap()
    }

    #[test]
    fn cutoffs() {
        let now: Timestamp = "2026-10-28T12:00:00Z".parse().unwrap();
        for (value, expected) in [
            ("2026-10-12T15:00:00+03:00", "2026-10-12T12:00:00Z"),
            // A date includes the entire day in UTC.
            ("2026-10-12", "2026-10-13T00:00:00Z"),
            ("7d", "2026-10-21T12:00:00Z"),
            ("1w", "2026-10-21T12:00:00Z"),
            ("2h30m", "2026-10-28T09:30:00Z"),
            ("0s", "2026-10-28T12:00:00Z"),
        ] {
            assert_eq!(
                resolve_config(&config(&format!("{value:?}")), now).unwrap(),
                ExcludeNewer::from_datetime(expected.parse().unwrap()),
                "{value}"
            );
        }
        for value in ["garbage", "-7d", "999999999999999999999d"] {
            assert!(
                Config::from_toml_str(&format!("exclude-newer = {value:?}")).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn config_exemptions() {
        let config = config(
            r#"{ cutoff = "7d", exemptions = { ruff = "==1.0", python = { version = "3.13.*", build = "*_cpython", cutoff = "1d" }, pytorch = { flags = ["cuda"] } } }"#,
        );
        let now: Timestamp = "2026-10-28T12:00:00Z".parse().unwrap();
        let expected = ExcludeNewer::from_datetime("2026-10-21T12:00:00Z".parse().unwrap())
            .with_exemption("ruff ==1.0".parse().unwrap())
            .unwrap()
            .with_exemption_until(
                "python 3.13.* *_cpython".parse().unwrap(),
                "2026-10-27T12:00:00Z".parse().unwrap(),
            )
            .unwrap()
            .with_exemption(MatchSpec::from_nameless(
                NamelessMatchSpec {
                    flags: Some(vec!["cuda".parse().unwrap()]),
                    ..Default::default()
                },
                "pytorch".parse().unwrap(),
            ))
            .unwrap();
        assert_eq!(resolve_config(&config, now).unwrap(), expected);
    }

    #[test]
    fn invalid_exemptions_are_not_silently_ignored() {
        for exemption in [
            r#"{ unknown_field = "==1" }"#,
            r#"{ version = "not a version!" }"#,
            r#"{ cutoff = ["7d"] }"#,
            r#"{ name = "another-package" }"#,
            r#"{ extras = ["foo"] }"#,
            r#"{ build-number = "0", build_number = "1" }"#,
        ] {
            assert!(
                Config::from_toml_str(&format!(
                    "exclude-newer = {{ cutoff = \"7d\", exemptions = {{ ruff = {exemption} }} }}"
                ))
                .is_err(),
                "{exemption}"
            );
        }
    }

    #[tokio::test]
    async fn build_uses_config_unless_explicitly_overridden() {
        use crate::opt::{BuildData, BuildOpts};
        use clap::Parser;

        let dir = tempfile::tempdir().unwrap();
        let recipe = dir.path().join("recipe.yaml");
        fs_err::write(&recipe, "package:\n  name: test\n  version: '1.0'\n").unwrap();
        let (config, _) = Config::from_toml_str("exclude-newer = '2020-01-01'").unwrap();
        let opts = BuildOpts::try_parse_from(["build"]).unwrap();
        let mut data = BuildData::from_opts_and_config(opts, Some(config));
        data.common.output_dir = dir.path().join("output");
        let tool_config = crate::get_tool_config(&data, &None).unwrap();

        for explicit in [
            None,
            Some("2021-01-01T00:00:00Z".parse::<Timestamp>().unwrap()),
        ] {
            data.exclude_newer = explicit;
            let outputs = crate::get_build_output(&data, &recipe, &tool_config)
                .await
                .unwrap();
            assert!(!outputs.is_empty());
            for output in outputs {
                assert_eq!(
                    output.build_configuration.exclude_newer,
                    Some(ExcludeNewer::from_datetime(
                        // The configured date includes the entire day.
                        explicit.unwrap_or_else(|| "2020-01-02T00:00:00Z".parse().unwrap())
                    ))
                );
            }
        }
    }

    #[test]
    fn explicit_policy_overrides_config() {
        let explicit = ExcludeNewer::from_datetime("2020-01-01T00:00:00Z".parse().unwrap());
        let config = config(r#""2020-01-01""#);
        assert_eq!(
            resolve(Some(&config), Some(explicit.clone())).unwrap(),
            Some(explicit)
        );
        assert_eq!(resolve(None, None).unwrap(), None);
        assert_eq!(
            resolve(Some(&config), None).unwrap(),
            Some(ExcludeNewer::from_datetime(
                "2020-01-02T00:00:00Z".parse().unwrap()
            ))
        );
    }
}
