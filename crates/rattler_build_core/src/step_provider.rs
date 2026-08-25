//! Pre-solve resolution of reusable build-step providers.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use miette::{IntoDiagnostic, WrapErr};
use rattler_build_jinja::{Jinja, Variable};
use rattler_build_recipe::stage1::{
    HashInfo,
    build::{
        BuildPlan, BuildString, ResolvedProvider, ResolvedStep,
        parse_step_package_reference_detailed,
    },
};
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{ChannelUrl, MatchSpec, ParseStrictness, RepoDataRecord};
use sha2::{Digest, Sha256};

use crate::{
    metadata::Output,
    render::solver::{install_packages_without_link_scripts, solve_environment},
    script::parse_reusable_steps,
    tool_configuration::Configuration,
};

#[derive(Clone)]
struct ProviderEnvironment {
    prefix: PathBuf,
    provider: ResolvedProvider,
}

/// Command-scoped cache for resolved provider environments.
///
/// The key includes channels and platform. The installed prefix is addressed by
/// the complete solved environment, including artifact hashes, so providers
/// cannot collide across channels, platforms, or dependency closures.
#[derive(Default)]
pub struct StepProviderResolver {
    providers: HashMap<String, ProviderEnvironment>,
}

fn local_step_path(reference: &str, recipe_dir: &Path) -> miette::Result<PathBuf> {
    let path = PathBuf::from(reference);
    if path.is_absolute() {
        return Err(miette::miette!(
            "reusable step paths must be relative to the recipe directory: `{reference}`"
        ));
    }
    let path = recipe_dir.join(path);
    [path.clone(), path.with_extension("yaml")]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| miette::miette!("reusable build step `{reference}` was not found"))
}

fn provider_step_path(prefix: &Path, provider: &str, step: &str) -> miette::Result<PathBuf> {
    let path = prefix
        .join("etc/rattler-build/steps")
        .join(provider)
        .join(step);
    [path.with_extension("yaml"), path]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| miette::miette!("provider `{provider}` does not contain step `{step}`"))
}

#[derive(serde::Deserialize, Default)]
struct ReusableHeader {
    #[serde(default)]
    inputs: IndexMap<String, InputDefinition>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum InputDefinition {
    Detailed {
        #[serde(default, rename = "type")]
        kind: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        default: Option<serde_yaml::Value>,
    },
    Shorthand(serde_yaml::Value),
}

fn input_matches_type(value: &Variable, kind: &str) -> bool {
    match kind {
        "string" => value.as_str().is_some(),
        "boolean" | "bool" => value.as_bool().is_some(),
        "integer" | "int" => value.as_i64().is_some(),
        "list" => value.is_sequence(),
        _ => false,
    }
}

fn evaluate_yaml_selectors(
    value: serde_yaml::Value,
    jinja: &Jinja,
    source: &str,
) -> miette::Result<serde_yaml::Value> {
    match value {
        serde_yaml::Value::Sequence(items) => {
            let mut selected = Vec::new();
            for item in items {
                let selector = item.as_mapping().and_then(|mapping| {
                    let then = mapping.get("then")?;
                    let condition = mapping.get("if")?.as_str()?;
                    Some((
                        condition.to_string(),
                        then.clone(),
                        mapping.get("else").cloned(),
                    ))
                });
                let item = if let Some((condition, then, otherwise)) = selector {
                    let result = jinja.eval(&condition).map_err(|error| {
                        miette::miette!(
                            "failed to evaluate selector `{condition}` in reusable step {source}: {error}"
                        )
                    })?;
                    if result.is_undefined() {
                        return Err(miette::miette!(
                            "undefined variable in selector `{condition}` in reusable step {source}"
                        ));
                    }
                    if result.is_true() {
                        Some(then)
                    } else {
                        otherwise
                    }
                } else {
                    Some(item)
                };
                let Some(item) = item else { continue };
                match evaluate_yaml_selectors(item, jinja, source)? {
                    serde_yaml::Value::Sequence(items) => selected.extend(items),
                    item => selected.push(item),
                }
            }
            Ok(serde_yaml::Value::Sequence(selected))
        }
        serde_yaml::Value::Mapping(mapping) => Ok(serde_yaml::Value::Mapping(
            mapping
                .into_iter()
                .map(|(key, value)| {
                    evaluate_yaml_selectors(value, jinja, source).map(|value| (key, value))
                })
                .collect::<miette::Result<_>>()?,
        )),
        value => Ok(value),
    }
}

fn render_reusable_steps(
    contents: &str,
    source: &str,
    supplied: &IndexMap<String, Variable>,
    output: &Output,
) -> miette::Result<Vec<rattler_build_recipe::stage1::build::Step>> {
    let header: ReusableHeader = serde_yaml::from_str(contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read inputs from reusable step {source}"))?;
    for key in supplied.keys() {
        if !header.inputs.contains_key(key) {
            return Err(miette::miette!(
                "reusable step {source} does not declare input `{key}`"
            ));
        }
    }
    let mut inputs = IndexMap::new();
    for (name, definition) in header.inputs {
        let (kind, required, default) = match definition {
            InputDefinition::Detailed {
                kind,
                required,
                default,
            } => (kind, required, default),
            InputDefinition::Shorthand(default) => (None, false, Some(default)),
        };
        let value = if let Some(value) = supplied.get(&name) {
            Some(value.clone())
        } else {
            default
                .map(|value| serde_yaml::from_value::<Variable>(value).into_diagnostic())
                .transpose()
                .wrap_err_with(|| {
                    format!("invalid default for input `{name}` in reusable step {source}")
                })?
        };
        let Some(value) = value else {
            if required {
                return Err(miette::miette!(
                    "reusable step {source} requires input `{name}`"
                ));
            }
            continue;
        };
        if let Some(kind) = kind
            && !input_matches_type(&value, &kind)
        {
            return Err(miette::miette!(
                "input `{name}` for reusable step {source} must have type `{kind}`"
            ));
        }
        inputs.insert(name, value);
    }

    let mut context = output.recipe.context.clone();
    context.insert(
        "inputs".to_string(),
        Variable::from(minijinja::Value::from_serialize(&inputs)),
    );
    // Build-directory variables are intentionally late-bound and must survive
    // provider input rendering for script execution and license collection.
    let mut protected = contents.to_string();
    let mut sentinels = Vec::new();
    for (index, variable) in rattler_build_types::late_bound_path::ALL_VARS
        .iter()
        .enumerate()
    {
        let token = format!("${{{{ {variable} }}}}");
        let sentinel = format!("__RATTLER_BUILD_LATE_BOUND_{index}__");
        protected = protected.replace(&token, &sentinel);
        sentinels.push((sentinel, token));
    }
    let jinja = Jinja::new(output.build_configuration.selector_config()).with_context(&context);
    let mut rendered = jinja
        .render_str(&protected)
        .map_err(|error| miette::miette!("failed to render reusable step {source}: {error}"))?;
    for (sentinel, token) in sentinels {
        rendered = rendered.replace(&sentinel, &token);
    }
    let yaml = serde_yaml::from_str(&rendered)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to parse reusable step {source} as YAML"))?;
    let rendered = serde_yaml::to_string(&evaluate_yaml_selectors(yaml, &jinja, source)?)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to serialize reusable step {source}"))?;
    parse_reusable_steps(&rendered, source).into_diagnostic()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn file_sha256(path: &Path) -> miette::Result<String> {
    Ok(sha256_bytes(&fs_err::read(path).into_diagnostic()?))
}

/// Reject dependencies introduced after variant expansion when their configured
/// variant value was not part of the initial render. Without this guard, a
/// metadata/provider-generated `python` or compiler dependency could silently
/// collapse several configured variants into one artifact.
pub fn validate_late_variant_dependencies<'a>(
    source: &str,
    dependencies: impl IntoIterator<Item = &'a rattler_build_recipe::stage1::Dependency>,
    output: &Output,
    configured_variant_keys: &BTreeSet<NormalizedKey>,
) -> miette::Result<()> {
    for dependency in dependencies {
        if let Some(name) = dependency.name() {
            let key = NormalizedKey::from(name.as_normalized());
            if configured_variant_keys.contains(&key)
                && !output.build_configuration.variant.contains_key(&key)
            {
                return Err(miette::miette!(
                    "{source} introduces variant dependency `{}` after recipe rendering; reference the `{}` variant in the initial recipe (or pass it through provider `with`) so it participates in variant expansion",
                    name.as_normalized(),
                    key.normalize()
                ));
            }
        }
    }
    Ok(())
}

fn apply_provider_hash(output: &mut Output, fingerprints: &[String]) {
    if fingerprints.is_empty() {
        return;
    }
    let fingerprint = sha256_bytes(fingerprints.join("\n").as_bytes());
    let old_hash = output.build_configuration.hash.clone();
    output.build_configuration.variant.insert(
        NormalizedKey::from("rattler_build_steps"),
        Variable::from(fingerprint),
    );
    let new_hash = HashInfo::from_variant(
        &output.build_configuration.variant,
        &output.recipe.build.noarch.unwrap_or_default(),
    );
    if let Some(build_string) = output.recipe.build.string.as_resolved() {
        let updated = build_string.replacen(&old_hash.to_string(), &new_hash.to_string(), 1);
        let updated = if updated == build_string {
            format!("{build_string}_{}", new_hash)
        } else {
            updated
        };
        output.recipe.build.string = BuildString::resolved(updated);
    }
    output.build_configuration.hash = new_hash;
}

fn resolved_provider(record: &RepoDataRecord) -> ResolvedProvider {
    let channel = record
        .channel
        .as_deref()
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
    }
}

fn environment_hash(platform: &str, records: &[RepoDataRecord]) -> String {
    let mut records = records
        .iter()
        .map(|record| {
            format!(
                "{}|{}|{}|{}|{}|{}",
                record.package_record.name.as_normalized(),
                record.package_record.version,
                record.package_record.build,
                record.channel.as_deref().unwrap_or_default(),
                record.package_record.subdir,
                record
                    .package_record
                    .sha256
                    .map(hex::encode)
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    records.sort();
    sha256_bytes(format!("{platform}\n{}", records.join("\n")).as_bytes())
}

impl StepProviderResolver {
    async fn resolve(
        &mut self,
        provider: &str,
        version: Option<&str>,
        output: &Output,
        tool_configuration: &Configuration,
    ) -> miette::Result<ProviderEnvironment> {
        let build_platform = &output.build_configuration.build_platform;
        let package_name = format!("{provider}-rattler-build-steps");
        let key = format!(
            "{}|{}|{}@{}",
            build_platform.platform,
            output
                .build_configuration
                .channels
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("|"),
            package_name,
            version.unwrap_or("*")
        );
        if let Some(environment) = self.providers.get(&key) {
            return Ok(environment.clone());
        }

        let spec = MatchSpec::from_str(
            &version.map_or_else(
                || package_name.clone(),
                |version| format!("{package_name} {version}"),
            ),
            ParseStrictness::Strict,
        )
        .into_diagnostic()?;
        let records = solve_environment(
            &format!("step provider {provider}"),
            &[spec],
            build_platform,
            &output.build_configuration.channels,
            tool_configuration,
            output.build_configuration.channel_priority,
            output.build_configuration.solve_strategy,
            output.build_configuration.exclude_newer,
        )
        .await?;
        let provider_record = records
            .iter()
            .find(|record| record.package_record.name.as_normalized() == package_name)
            .ok_or_else(|| {
                miette::miette!("step provider solve did not return `{package_name}`")
            })?;
        let provider_record = resolved_provider(provider_record);
        let prefix = tool_configuration
            .cache_dir
            .join("rattler-build")
            .join("step-providers")
            .join(environment_hash(
                &build_platform.platform.to_string(),
                &records,
            ));
        install_packages_without_link_scripts(
            &format!("step provider {provider}"),
            &records,
            build_platform.platform,
            &prefix,
            tool_configuration,
        )
        .await?;
        let environment = ProviderEnvironment {
            prefix,
            provider: provider_record,
        };
        self.providers.insert(key, environment.clone());
        Ok(environment)
    }
}

/// Resolve and render packaged reusable steps before the recipe's build and host
/// environments are solved. Provider packages are installed in dedicated cache
/// prefixes and never enter either recipe prefix.
pub async fn preprocess_reusable_steps(
    output: &mut Output,
    tool_configuration: &Configuration,
    resolver: &mut StepProviderResolver,
    configured_variant_keys: &BTreeSet<NormalizedKey>,
) -> miette::Result<()> {
    let references = match output.recipe.build.plan.steps() {
        Some(steps) if steps.iter().any(|step| step.uses.is_some()) => steps
            .iter()
            .map(|step| (step.uses.clone(), step.with.clone()))
            .collect::<Vec<_>>(),
        _ => return Ok(()),
    };

    let recipe_dir = output
        .build_configuration
        .directories
        .recipe_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut build_requirements = Vec::new();
    let mut host_requirements = Vec::new();
    let mut fingerprints = Vec::new();

    for (index, (reference, inputs)) in references.into_iter().enumerate() {
        let Some(reference) = reference else {
            continue;
        };
        let parsed = parse_step_package_reference_detailed(&reference)
            .map_err(|error| miette::miette!("invalid reusable step `{reference}`: {error}"))?;
        let (path, provider) = if let Some(parsed) = parsed {
            let environment = resolver
                .resolve(parsed.provider, parsed.version, output, tool_configuration)
                .await?;
            (
                provider_step_path(&environment.prefix, parsed.provider, parsed.step)?,
                Some(environment.provider),
            )
        } else {
            (local_step_path(&reference, &recipe_dir)?, None)
        };

        let content_sha256 = file_sha256(&path)?;
        let contents = fs_err::read_to_string(&path).into_diagnostic()?;
        let rendered_steps = render_reusable_steps(&contents, &reference, &inputs, output)
            .wrap_err_with(|| format!("failed to preprocess reusable step `{reference}`"))?;
        let rendered_sha256 = sha256_bytes(
            &serde_json::to_vec(&rendered_steps)
                .into_diagnostic()
                .wrap_err("failed to hash rendered reusable steps")?,
        );
        let selected = BuildPlan::Steps(rendered_steps.clone())
            .select_steps(None)
            .map_err(|error| miette::miette!("invalid reusable step `{reference}`: {error}"))?;
        for nested in selected {
            if !nested.requirements.inherit.is_default() {
                return Err(miette::miette!(
                    "reusable step `{reference}` changes requirements.inherit; inheritance must be configured on the referencing recipe step"
                ));
            }
            validate_late_variant_dependencies(
                &format!("reusable step `{reference}`"),
                nested
                    .requirements
                    .build
                    .iter()
                    .chain(&nested.requirements.host),
                output,
                configured_variant_keys,
            )?;
            build_requirements.extend(nested.requirements.build);
            host_requirements.extend(nested.requirements.host);
        }
        fingerprints.push(format!(
            "{}|{}|{}|{}",
            reference,
            provider
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .into_diagnostic()?
                .unwrap_or_default(),
            content_sha256,
            rendered_sha256
        ));
        output
            .recipe
            .build
            .plan
            .steps_mut()
            .expect("steps were present above")[index]
            .resolved = Some(Box::new(ResolvedStep {
            reference,
            provider,
            content_sha256,
            rendered_sha256,
            steps: rendered_steps,
        }));
    }

    apply_provider_hash(output, &fingerprints);
    output.recipe.requirements.build.extend(build_requirements);
    output.recipe.requirements.host.extend(host_requirements);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reusable_step_selectors_are_valid_yaml_and_flatten_selected_branch() {
        let mut context = IndexMap::new();
        context.insert("enabled".to_string(), Variable::from(false));
        let jinja = Jinja::new(Default::default()).with_context(&context);
        let yaml = serde_yaml::from_str(
            r#"
steps:
  - if: enabled
    then:
      - run: enabled
    else:
      - run: fallback
  - run: always
"#,
        )
        .unwrap();

        let selected = evaluate_yaml_selectors(yaml, &jinja, "test.yaml").unwrap();
        let rendered = serde_yaml::to_string(&selected).unwrap();
        let steps = parse_reusable_steps(&rendered, "test.yaml").unwrap();

        assert_eq!(steps.len(), 2);
        assert_eq!(
            steps[0].run,
            rattler_build_recipe::stage1::build::StepRun::Command("fallback".to_string())
        );
        assert_eq!(
            steps[1].run,
            rattler_build_recipe::stage1::build::StepRun::Command("always".to_string())
        );
    }
}
