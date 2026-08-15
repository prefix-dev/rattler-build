//! Pre-solve resolution of reusable build-step providers.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, WrapErr};
use rattler_build_recipe::stage1::build::{BuildPlan, ResolvedStep, parse_step_package_reference};
use rattler_conda_types::{MatchSpec, ParseStrictness, RepoDataRecord};

use crate::{
    metadata::Output,
    render::solver::{install_packages, solve_environment},
    script::read_reusable_steps,
    tool_configuration::Configuration,
};

fn local_step_path(reference: &str, recipe_dir: &Path) -> miette::Result<PathBuf> {
    let path = PathBuf::from(reference);
    let path = if path.is_absolute() {
        path
    } else {
        recipe_dir.join(path)
    };
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

fn provider_identifier(record: &RepoDataRecord) -> String {
    format!(
        "{}-{}-{}",
        record.package_record.name.as_normalized(),
        record.package_record.version,
        record.package_record.build
    )
}

/// Resolve and render packaged reusable steps before the recipe's build and host
/// environments are solved. Provider packages are installed in dedicated cache
/// prefixes and never enter either recipe prefix.
pub async fn preprocess_reusable_steps(
    output: &mut Output,
    tool_configuration: &Configuration,
) -> miette::Result<()> {
    let Some(steps) = output.recipe.build.plan.steps_mut() else {
        return Ok(());
    };
    if !steps.iter().any(|step| step.uses.is_some()) {
        return Ok(());
    }

    let recipe_dir = output
        .build_configuration
        .directories
        .recipe_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let build_platform = output.build_configuration.build_platform.clone();
    let channels = output.build_configuration.channels.clone();
    let channel_priority = output.build_configuration.channel_priority;
    let solve_strategy = output.build_configuration.solve_strategy;
    let exclude_newer = output.build_configuration.exclude_newer;
    let cache_root = tool_configuration
        .cache_dir
        .join("rattler-build")
        .join("step-providers");
    let mut providers: HashMap<String, (PathBuf, String)> = HashMap::new();
    let mut build_requirements = Vec::new();
    let mut host_requirements = Vec::new();

    for step in steps {
        let Some(reference) = step.uses.as_deref() else {
            continue;
        };
        let parsed = parse_step_package_reference(reference)
            .map_err(|error| miette::miette!("invalid reusable step `{reference}`: {error}"))?;
        let (path, source) = if let Some((provider, step_name)) = parsed {
            let (prefix, package_identifier) = if let Some(resolved) = providers.get(provider) {
                resolved.clone()
            } else {
                let package_name = format!("{provider}-rattler-build-steps");
                let spec = MatchSpec::from_str(&package_name, ParseStrictness::Strict)
                    .into_diagnostic()?;
                let records = solve_environment(
                    &format!("step provider {provider}"),
                    &[spec],
                    &build_platform,
                    &channels,
                    tool_configuration,
                    channel_priority,
                    solve_strategy,
                    exclude_newer,
                )
                .await?;
                let provider_record = records
                    .iter()
                    .find(|record| record.package_record.name.as_normalized() == package_name)
                    .ok_or_else(|| {
                        miette::miette!("step provider solve did not return `{package_name}`")
                    })?;
                let identifier = provider_identifier(provider_record);
                let prefix = cache_root.join(&identifier);
                install_packages(
                    &format!("step provider {provider}"),
                    &records,
                    build_platform.platform,
                    &prefix,
                    tool_configuration,
                )
                .await?;
                providers.insert(provider.to_string(), (prefix.clone(), identifier.clone()));
                (prefix, identifier)
            };
            (
                provider_step_path(&prefix, provider, step_name)?,
                format!("{reference} ({package_identifier})"),
            )
        } else {
            let path = local_step_path(reference, &recipe_dir)?;
            let source = path.display().to_string();
            (path, source)
        };

        let rendered_steps = read_reusable_steps(&path)
            .into_diagnostic()
            .wrap_err_with(|| format!("failed to preprocess reusable step `{reference}`"))?;
        let selected = BuildPlan::Steps(rendered_steps.clone())
            .select_steps(None)
            .map_err(|error| miette::miette!("invalid reusable step `{reference}`: {error}"))?;
        for nested in selected {
            build_requirements.extend(nested.requirements.build);
            host_requirements.extend(nested.requirements.host);
        }
        step.resolved = Some(Box::new(ResolvedStep {
            source,
            steps: rendered_steps,
        }));
    }

    output.recipe.requirements.build.extend(build_requirements);
    output.recipe.requirements.host.extend(host_requirements);
    Ok(())
}
