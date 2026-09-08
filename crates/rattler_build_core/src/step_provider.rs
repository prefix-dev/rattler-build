//! Transport for packaged action sources consumed by the recipe compiler.

use std::{collections::HashMap, path::{Path, PathBuf}};

use miette::{IntoDiagnostic, WrapErr};
use rattler_build_recipe::actions::{ActionProvenance, ActionRequest, ActionSource, ActionSources, ResolvedProvider};
use rattler_conda_types::{ChannelUrl, MatchSpec, ParseStrictness, RepoDataRecord};
use rattler_solve::{ChannelPriority, SolveStrategy};
use sha2::{Digest, Sha256};

use crate::{
    metadata::PlatformWithVirtualPackages,
    render::solver::{install_packages_without_link_scripts, solve_environment},
    tool_configuration::Configuration,
};

#[derive(Clone)]
struct ProviderEnvironment {
    prefix: PathBuf,
    provider: ResolvedProvider,
    fingerprint: String,
}

/// Solve settings for the independent build-platform provider environment.
/// Provider dependencies are never added to recipe build or host requirements.
pub struct ProviderSolveConfig<'a> {
    pub build_platform: &'a PlatformWithVirtualPackages,
    pub channels: &'a [ChannelUrl],
    pub channel_priority: ChannelPriority,
    pub solve_strategy: SolveStrategy,
    pub exclude_newer: Option<jiff::Timestamp>,
}

/// Command-scoped cache of independently installed provider environments.
#[derive(Default)]
pub struct StepProviderResolver {
    providers: HashMap<String, ProviderEnvironment>,
}

#[derive(Debug, PartialEq, Eq)]
struct PackageReference<'a> {
    provider: &'a str,
    step: &'a str,
    version: Option<&'a str>,
}

fn package_reference(reference: &str) -> miette::Result<PackageReference<'_>> {
    let invalid = || miette::miette!(
        "invalid packaged action reference `{reference}`; expected `provider:step[@version]`"
    );
    let (provider, step_and_version) = reference.split_once(':').ok_or_else(invalid)?;
    let (step, version) = step_and_version.split_once('@')
        .map_or((step_and_version, None), |(step, version)| (step, Some(version)));
    let valid = |value: &str| !value.is_empty() && value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
    });
    if !valid(provider) || !valid(step) || version.is_some_and(|v| v.trim().is_empty() || v.contains('@')) {
        return Err(invalid());
    }
    Ok(PackageReference { provider, step, version })
}

fn provider_step_path(prefix: &Path, provider: &str, step: &str) -> miette::Result<PathBuf> {
    let path = prefix.join("etc/rattler-build/steps").join(provider).join(step);
    let path = [path.with_extension("yaml"), path.with_extension("yml")]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| miette::miette!("provider `{provider}` does not contain action `{step}` (.yaml or .yml)"))?;
    fs_err::canonicalize(path).into_diagnostic()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn resolved_provider(record: &RepoDataRecord) -> ResolvedProvider {
    let channel = record.channel.as_deref()
        .and_then(|channel| channel.parse::<url::Url>().ok())
        .map(ChannelUrl::from)
        .map(|channel| crate::packaging::metadata::clean_url(&channel))
        .unwrap_or_else(|| record.channel.clone().unwrap_or_default());
    ResolvedProvider {
        name: record.package_record.name.as_normalized().to_string(),
        version: record.package_record.version.to_string(),
        build: record.package_record.build.clone(),
        subdir: record.package_record.subdir.clone(),
        channel,
        sha256: record.package_record.sha256.map(hex::encode),
        md5: record.package_record.md5.map(hex::encode),
    }
}

fn environment_hash(platform: &str, records: &[RepoDataRecord]) -> miette::Result<String> {
    let mut identities = records.iter().map(|record| {
        // Both hashes participate: MD5-only repodata must not collapse distinct artifacts.
        serde_json::to_string(&resolved_provider(record)).into_diagnostic()
    }).collect::<miette::Result<Vec<_>>>()?;
    identities.sort();
    Ok(sha256_bytes(format!("{platform}\n{}", identities.join("\n")).as_bytes()))
}

impl StepProviderResolver {
    async fn resolve(
        &mut self,
        reference: &PackageReference<'_>,
        settings: &ProviderSolveConfig<'_>,
        tool_configuration: &Configuration,
    ) -> miette::Result<ProviderEnvironment> {
        let build_platform = settings.build_platform;
        let package_name = format!("{}-rattler-build-steps", reference.provider);
        let key = format!("{}|{}|{}@{}|{:?}|{:?}|{:?}",
            build_platform.platform,
            settings.channels.iter().map(ToString::to_string).collect::<Vec<_>>().join("|"),
            package_name, reference.version.unwrap_or("*"),
            settings.channel_priority, settings.solve_strategy, settings.exclude_newer,
        );
        if let Some(environment) = self.providers.get(&key) {
            return Ok(environment.clone());
        }
        let spec = MatchSpec::from_str(&reference.version.map_or_else(
            || package_name.clone(), |version| format!("{package_name} {version}"),
        ), ParseStrictness::Strict).into_diagnostic()?;
        let records = solve_environment(
            &format!("action provider {}", reference.provider), &[spec], build_platform,
            settings.channels, tool_configuration, settings.channel_priority,
            settings.solve_strategy, settings.exclude_newer,
        ).await?;
        let provider_record = records.iter()
            .find(|record| record.package_record.name.as_normalized() == package_name)
            .ok_or_else(|| miette::miette!("action provider solve did not return `{package_name}`"))?;
        let provider = resolved_provider(provider_record);
        let fingerprint = environment_hash(&build_platform.platform.to_string(), &records)?;
        let prefix = tool_configuration.cache_dir.join("rattler-build")
            .join("step-providers").join(&fingerprint);
        install_packages_without_link_scripts(
            &format!("action provider {}", reference.provider), &records,
            build_platform.platform, &prefix, tool_configuration,
        ).await?;
        let environment = ProviderEnvironment { prefix, provider, fingerprint };
        self.providers.insert(key, environment.clone());
        Ok(environment)
    }

    /// Fulfil exactly the external request produced by synchronous compilation.
    /// Parsing, conditions, input validation, recursion and requirements belong to
    /// the compiler; this adapter only installs packages and registers source bytes.
    pub async fn register_requested(
        &mut self,
        request: ActionRequest,
        sources: &ActionSources,
        settings: &ProviderSolveConfig<'_>,
        tool_configuration: &Configuration,
    ) -> miette::Result<()> {
        let reference = package_reference(&request.reference)?;
        let environment = self.resolve(&reference, settings, tool_configuration).await?;
        let path = provider_step_path(&environment.prefix, reference.provider, reference.step)?;
        let contents = fs_err::read_to_string(&path).into_diagnostic()
            .wrap_err_with(|| format!("failed to read packaged action `{}`", request.reference))?;
        let content_sha256 = sha256_bytes(contents.as_bytes());
        let provenance = ActionProvenance {
            reference: request.reference.clone(),
            provider: Some(environment.provider),
            content_sha256,
            fingerprint: Some(environment.fingerprint.clone()),
        };
        sources.register(request, ActionSource {
            path, contents, fingerprint: Some(environment.fingerprint), provenance: Some(provenance),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_reference_preserves_conda_constraint() {
        let parsed = package_reference("cmake:build@>=0.3,<0.4").unwrap();
        assert_eq!(parsed, PackageReference { provider: "cmake", step: "build", version: Some(">=0.3,<0.4") });
        for reference in ["cmake:../build", "../cmake:build", "cmake:build@", "cmake:build@1@2", "C:\\build.yaml"] {
            assert!(package_reference(reference).is_err(), "{reference}");
        }
    }

    #[test]
    fn package_lookup_accepts_yml() {
        let prefix = tempfile::tempdir().unwrap();
        let directory = prefix.path().join("etc/rattler-build/steps/test");
        fs_err::create_dir_all(&directory).unwrap();
        let path = directory.join("build.yml");
        fs_err::write(&path, "steps: []\n").unwrap();
        assert_eq!(provider_step_path(prefix.path(), "test", "build").unwrap(), fs_err::canonicalize(path).unwrap());
        assert!(provider_step_path(prefix.path(), "test", "missing").is_err());
    }

    #[test]
    fn md5_only_artifacts_have_distinct_environment_identities() {
        let mut value = serde_json::json!({
            "name": "test-rattler-build-steps",
            "version": "1.0.0",
            "build": "0",
            "build_number": 0,
            "subdir": "noarch",
            "fn": "test-rattler-build-steps-1.0.0-0.conda",
            "url": "https://example.com/noarch/test-rattler-build-steps-1.0.0-0.conda",
            "channel": "https://example.com/",
            "md5": "00000000000000000000000000000001"
        });
        let first: RepoDataRecord = serde_json::from_value(value.clone()).unwrap();
        value["md5"] = serde_json::json!("00000000000000000000000000000002");
        let second: RepoDataRecord = serde_json::from_value(value).unwrap();
        assert_ne!(
            environment_hash("linux-64", &[first]).unwrap(),
            environment_hash("linux-64", &[second]).unwrap(),
        );
    }
}
