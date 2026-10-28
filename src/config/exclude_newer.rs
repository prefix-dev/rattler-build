use jiff::{Span, SpanRelativeTo, Timestamp, civil::Date};
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{MatchSpec, NamelessMatchSpec};
use rattler_config::config::exclude_newer::{ExcludeNewerConfig, ExcludeNewerExemption};
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

fn parse_cutoff(value: &str, now: Timestamp) -> miette::Result<Timestamp> {
    if let Ok(timestamp) = value.parse::<Timestamp>() {
        return Ok(timestamp);
    }
    if let Ok(date) = value.parse::<Date>() {
        return date
            .to_zoned(jiff::tz::TimeZone::UTC)
            .map(|date| date.timestamp())
            .into_diagnostic();
    }
    let duration = value
        .parse::<Span>()
        .and_then(|span| span.to_duration(SpanRelativeTo::days_are_24_hours()))
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid exclude-newer cutoff {value:?}: expected an RFC3339 timestamp, date, or duration (e.g. 7d)"))?;
    if duration.is_negative() {
        miette::bail!("exclude-newer duration must not be negative: {value:?}");
    }
    now.checked_sub(duration).into_diagnostic()
}

fn resolve_config(config: &ExcludeNewerConfig, now: Timestamp) -> miette::Result<ExcludeNewer> {
    let mut policy = ExcludeNewer::from_datetime(parse_cutoff(&config.cutoff, now)?);
    for (name, exemption) in &config.exemptions {
        let (spec, cutoff) = (|| -> miette::Result<_> {
            let mut fields = match exemption {
                ExcludeNewerExemption::Version(version) => {
                    serde_json::Map::from_iter([("version".into(), version.clone().into())])
                }
                ExcludeNewerExemption::Fields(fields) => {
                    let mut normalized = serde_json::Map::new();
                    for (key, value) in fields {
                        if normalized
                            .insert(
                                key.replace('-', "_"),
                                serde_json::to_value(value).into_diagnostic()?,
                            )
                            .is_some()
                        {
                            miette::bail!("duplicate exemption field {key:?}");
                        }
                    }
                    normalized
                }
            };
            let cutoff = fields
                .remove("cutoff")
                .map(|value| {
                    let value = value
                        .as_str()
                        .ok_or_else(|| miette::miette!("cutoff must be a string"))?;
                    parse_cutoff(value, now)
                })
                .transpose()?;
            // Serde ignores unknown fields; reject typos rather than broadening an exemption.
            for field in fields.keys() {
                if !matches!(
                    field.as_str(),
                    "version"
                        | "build"
                        | "build_number"
                        | "file_name"
                        | "flags"
                        | "channel"
                        | "subdir"
                        | "md5"
                        | "sha256"
                        | "url"
                        | "license"
                        | "license_family"
                        | "track_features"
                ) {
                    miette::bail!("unsupported exemption field {field:?}");
                }
            }
            let spec: NamelessMatchSpec =
                serde_json::from_value(fields.into()).into_diagnostic()?;
            Ok((MatchSpec::from_nameless(spec, name.clone().into()), cutoff))
        })()
        .wrap_err_with(|| format!("invalid exclude-newer exemption for {}", name.as_source()))?;
        policy = match cutoff {
            Some(cutoff) => policy.with_exemption_until(spec, cutoff),
            None => policy.with_exemption(spec),
        }
        .into_diagnostic()?;
    }
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn cutoffs() {
        let now: Timestamp = "2026-10-28T12:00:00Z".parse().unwrap();
        for (value, expected) in [
            ("2026-10-12T15:00:00+03:00", "2026-10-12T12:00:00Z"),
            ("2026-10-12", "2026-10-12T00:00:00Z"),
            ("7d", "2026-10-21T12:00:00Z"),
            ("1w", "2026-10-21T12:00:00Z"),
            ("2h30m", "2026-10-28T09:30:00Z"),
            ("0s", "2026-10-28T12:00:00Z"),
        ] {
            assert_eq!(
                parse_cutoff(value, now).unwrap(),
                expected.parse::<Timestamp>().unwrap()
            );
        }
        for value in ["garbage", "-7d", "1mo", "999999999999999999999d"] {
            assert!(parse_cutoff(value, now).is_err(), "{value}");
        }
        assert!(parse_cutoff("7d", Timestamp::MIN).is_err());
    }

    #[test]
    fn config_exemptions() {
        let (config, _) = Config::from_toml_str(r#"
            [exclude-newer]
            cutoff = "7d"
            exemptions = { ruff = "==1.0", python = { version = "3.13.*", build = "*_cpython", cutoff = "1d" }, pytorch = { flags = ["cuda"] } }
        "#).unwrap();
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
        assert_eq!(
            resolve_config(config.exclude_newer.as_ref().unwrap(), now).unwrap(),
            expected
        );
    }

    #[test]
    fn invalid_exemptions_are_not_silently_ignored() {
        for exemption in [
            r#"{ verison = "==1" }"#,
            r#"{ version = "not a version!" }"#,
            r#"{ cutoff = ["7d"] }"#,
            r#"{ name = "another-package" }"#,
            r#"{ extras = ["foo"] }"#,
            r#"{ build-number = "0", build_number = "1" }"#,
        ] {
            let (config, _) = Config::from_toml_str(&format!(
                "exclude-newer = {{ cutoff = \"7d\", exemptions = {{ ruff = {exemption} }} }}"
            ))
            .unwrap();
            assert!(
                resolve(config.exclude_newer.as_ref(), None).is_err(),
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
                        explicit.unwrap_or_else(|| "2020-01-01T00:00:00Z".parse().unwrap())
                    ))
                );
            }
        }
    }

    #[test]
    fn explicit_policy_overrides_config() {
        let explicit = ExcludeNewer::from_datetime("2020-01-01T00:00:00Z".parse().unwrap());
        let config = ExcludeNewerConfig::from_cutoff("invalid");
        assert_eq!(
            resolve(Some(&config), Some(explicit.clone())).unwrap(),
            Some(explicit)
        );
        assert!(resolve(Some(&config), None).is_err());
        assert_eq!(resolve(None, None).unwrap(), None);
        let config = ExcludeNewerConfig::from_cutoff("2020-01-01");
        assert_eq!(
            resolve(Some(&config), None).unwrap(),
            Some(ExcludeNewer::from_datetime(
                "2020-01-01T00:00:00Z".parse().unwrap()
            ))
        );
    }
}
