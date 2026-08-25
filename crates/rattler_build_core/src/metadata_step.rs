//! Experimental pre-solve recipe metadata step execution.

use std::{collections::HashMap, path::Component};

use miette::{IntoDiagnostic, WrapErr};
use rattler_build_jinja::Variable;
use rattler_build_recipe::stage1::{
    HashInfo, Requirements,
    build::{BuildPlan, BuildString},
};
use rattler_build_script::{EnvironmentIsolation, ExecutionContext, RuntimeEnv};
use rattler_build_types::NormalizedKey;
use rattler_conda_types::Platform;
use sha2::{Digest, Sha256};

use crate::{
    metadata::Output, render::resolved_dependencies::RunExportsDownload,
    tool_configuration::Configuration, types::Directories,
};

fn merge_authored_steps(generated: &mut BuildPlan, authored: &BuildPlan) -> miette::Result<()> {
    if generated == authored {
        return Ok(());
    }
    let BuildPlan::Steps(generated_steps) = generated else {
        return Ok(());
    };
    let authored_steps = match authored {
        BuildPlan::Steps(steps) => steps.as_slice(),
        BuildPlan::Script(_) => &[],
    };

    // `build.steps.append` applies to the already rendered recipe, so the
    // resulting list begins with the authored steps. Strip that unchanged
    // prefix before merging, otherwise an appended generated override would be
    // mistaken for a duplicate of the authored step it is intended to replace.
    let appended_to_authored = generated_steps.starts_with(authored_steps);
    let mut metadata_steps = if appended_to_authored {
        generated_steps.drain(..authored_steps.len());
        std::mem::take(generated_steps)
    } else {
        std::mem::take(generated_steps)
    };

    let mut generated_names = std::collections::HashSet::new();
    for step in &mut metadata_steps {
        if step.name.is_none() {
            step.name.clone_from(&step.uses);
        }
        let name = step.name.as_deref().ok_or_else(|| {
            miette::miette!("build.metadata generated an unnamed build step; generated steps must have names so recipes can override them")
        })?;
        if !generated_names.insert(name.to_string()) {
            return Err(miette::miette!(
                "build.metadata generated duplicate build step name `{name}`"
            ));
        }
    }

    let mut authored_by_name = HashMap::new();
    for step in authored_steps {
        let Some(name) = step.name.as_deref() else {
            continue;
        };
        if authored_by_name.insert(name, step).is_some() {
            return Err(miette::miette!(
                "duplicate recipe-authored build step name `{name}`"
            ));
        }
    }

    if appended_to_authored {
        let mut merged = authored_steps.to_vec();
        merged.extend(metadata_steps.into_iter().filter(|step| {
            step.name
                .as_ref()
                .is_none_or(|name| !authored_by_name.contains_key(name.as_str()))
        }));
        *generated_steps = merged;
        return Ok(());
    }

    let mut merged = Vec::with_capacity(metadata_steps.len() + authored_steps.len());
    let mut consumed = std::collections::HashSet::new();
    for step in metadata_steps {
        let name = step
            .name
            .as_deref()
            .expect("generated step names were validated above");
        if let Some(authored_step) = authored_by_name.get(name) {
            merged.push((*authored_step).clone());
            consumed.insert(name.to_string());
        } else {
            merged.push(step);
        }
    }
    merged.extend(
        authored_steps
            .iter()
            .filter(|step| {
                step.name
                    .as_ref()
                    .is_none_or(|name| !consumed.contains(name))
            })
            .cloned(),
    );
    *generated_steps = merged;
    Ok(())
}

fn apply_metadata_hash(output: &mut Output, contents: &[u8]) {
    let fingerprint = hex::encode(Sha256::digest(contents));
    let old_hash = output.build_configuration.hash.clone();
    output.build_configuration.variant.insert(
        NormalizedKey::from("rattler_build_metadata"),
        Variable::from(fingerprint),
    );
    let new_hash = HashInfo::from_variant(
        &output.build_configuration.variant,
        &output.recipe.build.noarch.unwrap_or_default(),
    );
    if let Some(build_string) = output.recipe.build.string.as_resolved() {
        let updated = build_string.replacen(&old_hash.to_string(), &new_hash.to_string(), 1);
        let updated = if updated == build_string {
            format!("{build_string}_{new_hash}")
        } else {
            updated
        };
        output.recipe.build.string = BuildString::resolved(updated);
    }
    output.build_configuration.hash = new_hash;
}

/// Serialize the mutable recipe sections after metadata and provider processing.
pub fn generated_metadata_yaml(output: &Output) -> miette::Result<String> {
    #[derive(serde::Serialize)]
    struct GeneratedMetadata<'a> {
        build: &'a rattler_build_recipe::stage1::Build,
        requirements: &'a Requirements,
        about: &'a rattler_build_recipe::stage1::About,
    }

    let mut generated_recipe = output.recipe.clone();
    generated_recipe.build.metadata = None;
    serde_yaml::to_string(&GeneratedMetadata {
        build: &generated_recipe.build,
        requirements: &generated_recipe.requirements,
        about: &generated_recipe.about,
    })
    .into_diagnostic()
    .wrap_err("failed to serialize generated recipe metadata")
}

/// Run `build.metadata` in a bootstrap environment, then apply its output before
/// reusable-step resolution and the final build/host solve.
pub async fn run_metadata_step(
    output: &mut Output,
    tool_configuration: &Configuration,
) -> miette::Result<()> {
    let Some(step) = output.recipe.build.metadata.clone() else {
        return Ok(());
    };
    if step.uses.is_some() {
        return Err(miette::miette!(
            "`build.metadata.uses` is not supported yet; metadata steps must contain an inline `run` command"
        ));
    }
    if step.optional || !step.depends_on.is_empty() {
        return Err(miette::miette!(
            "`build.metadata` cannot be optional or depend on normal build steps"
        ));
    }

    let span = tracing::info_span!("Running pre-solve metadata step");
    let _entered = span.enter();
    let temporary = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("failed to create metadata-step workspace")?;
    let recipe_path = output.build_configuration.directories.recipe_path.clone();
    let timestamp = output.build_configuration.timestamp;
    let mut directories = Directories::builder(
        "metadata",
        &recipe_path,
        temporary.path(),
        &timestamp,
        Platform::current(),
    )
    .no_build_id(true)
    .merge_build_and_host(false)
    .build()
    .into_diagnostic()?;
    // Keep the real local output channel available to bootstrap requirements;
    // only prefixes and work files belong in the temporary directory.
    directories.output_dir = output.build_configuration.directories.output_dir.clone();
    fs_err::create_dir_all(&directories.work_dir)
        .into_diagnostic()
        .wrap_err("failed to create metadata-step work directory")?;

    let mut bootstrap_config = tool_configuration.clone();
    bootstrap_config.environments_externally_managed = false;
    let mut bootstrap = output.clone();
    bootstrap.build_configuration.directories = directories.clone();
    bootstrap.finalized_dependencies = None;
    bootstrap.finalized_sources = None;
    bootstrap.recipe.requirements = Requirements {
        build: step.requirements.build.clone(),
        host: step.requirements.host.clone(),
        ..Requirements::default()
    };
    bootstrap.recipe.build.plan = BuildPlan::default();
    bootstrap.recipe.build.metadata = None;
    bootstrap.recipe.build.merge_build_and_host_envs = false;
    let bootstrap = bootstrap
        .resolve_dependencies(&bootstrap_config, RunExportsDownload::DownloadMissing)
        .await
        .into_diagnostic()?;
    bootstrap
        .install_environments(&bootstrap_config)
        .await
        .into_diagnostic()?;

    let output_file = temporary.path().join("metadata-output.txt");
    let mut env = HashMap::new();
    env.insert(
        "OUTPUT_FILE".to_string(),
        Some(output_file.to_string_lossy().into_owned()),
    );
    env.insert(
        "RATTLER_BUILD_OUTPUT_FILE".to_string(),
        Some(output_file.to_string_lossy().into_owned()),
    );
    env.insert(
        "RECIPE_DIR".to_string(),
        Some(
            output
                .build_configuration
                .directories
                .recipe_dir
                .to_string_lossy()
                .into_owned(),
        ),
    );
    let source_dir = output
        .build_configuration
        .directories
        .source_dir
        .as_ref()
        .unwrap_or(&output.build_configuration.directories.work_dir);
    env.insert(
        "SRC_DIR".to_string(),
        Some(source_dir.to_string_lossy().into_owned()),
    );
    env.insert(
        "BUILD_PLATFORM".to_string(),
        Some(
            output
                .build_configuration
                .build_platform
                .platform
                .to_string(),
        ),
    );
    env.insert(
        "HOST_PLATFORM".to_string(),
        Some(
            output
                .build_configuration
                .host_platform
                .platform
                .to_string(),
        ),
    );
    env.insert(
        "TARGET_PLATFORM".to_string(),
        Some(output.build_configuration.target_platform.to_string()),
    );
    env.insert(
        "PKG_NAME".to_string(),
        Some(output.recipe.package.name.as_normalized().to_string()),
    );
    env.insert(
        "PKG_VERSION".to_string(),
        Some(output.recipe.package.version.to_string()),
    );

    let context = ExecutionContext::separate(
        RuntimeEnv::current(),
        &directories.build_prefix,
        output.build_configuration.build_platform.platform,
        &directories.host_prefix,
        output.build_configuration.host_platform.platform,
    );
    let recipe_dir = &output.build_configuration.directories.recipe_dir;
    let work_dir = if let Some(cwd) = &step.cwd {
        if cwd.is_absolute()
            || cwd
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(miette::miette!(
                "`build.metadata.cwd` must stay within the source directory"
            ));
        }
        source_dir.join(cwd)
    } else {
        source_dir.clone()
    };
    let reserved_env_keys = env.keys().cloned().collect::<Vec<_>>();
    let mut script = step.to_script();
    // Executor-provided metadata variables are reserved. `Script::run_script`
    // normally lets script-local values override its base environment, so
    // remove collisions before execution.
    for key in env.keys() {
        script.env.shift_remove(key);
    }
    // Keep generated wrappers in the temporary workspace while running the
    // actual command in the local project directory.
    script.cwd = Some(work_dir);
    script
        .run_script(
            env,
            &directories.work_dir,
            recipe_dir,
            context,
            Some(output.jinja_renderer()),
            output.build_configuration.sandbox_config(),
            EnvironmentIsolation::Strict,
        )
        .await
        .map_err(|error| miette::miette!("metadata step failed: {error}"))?;

    if !output_file.is_file() {
        return Err(miette::miette!(
            "metadata step completed without creating OUTPUT_FILE at {}",
            output_file.display()
        ));
    }
    let contents = fs_err::read(&output_file)
        .into_diagnostic()
        .wrap_err("failed to read metadata-step output")?;
    let authored_plan = output.recipe.build.plan.clone();
    crate::recipe_patch::apply_metadata_output(&mut output.recipe, &output_file)?;
    merge_authored_steps(&mut output.recipe.build.plan, &authored_plan)?;
    apply_metadata_hash(output, &contents);

    // Keep portable provider provenance, but do not serialize this machine's
    // absolute provider cache path into the rendered recipe stored in packages.
    if let Some(metadata) = &mut output.recipe.build.metadata {
        for key in &reserved_env_keys {
            metadata.env.shift_remove(key);
        }
        metadata.env.shift_remove("RATTLER_BUILD_PROVIDER_PREFIX");
    }

    // Keep the bootstrap output alive until execution has completely finished.
    drop(bootstrap);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_build_recipe::stage1::build::{Step, StepRun};

    fn step(name: Option<&str>, command: &str) -> Step {
        Step {
            name: name.map(str::to_string),
            run: StepRun::Command(command.to_string()),
            ..Step::default()
        }
    }

    #[test]
    fn authored_steps_replace_generated_names_and_allow_unnamed_additions() {
        let authored_install = step(Some("install"), "authored install");
        let unnamed = step(None, "authored cleanup");
        let authored = BuildPlan::Steps(vec![authored_install.clone(), unnamed.clone()]);
        let mut generated = BuildPlan::Steps(vec![
            step(Some("configure"), "generated configure"),
            step(Some("install"), "generated install"),
        ]);

        merge_authored_steps(&mut generated, &authored).unwrap();

        assert_eq!(
            generated,
            BuildPlan::Steps(vec![
                step(Some("configure"), "generated configure"),
                authored_install,
                unnamed,
            ])
        );
    }

    #[test]
    fn appended_generated_steps_preserve_authored_order_and_do_not_duplicate_overrides() {
        let authored_configure = step(Some("configure"), "authored configure");
        let authored = BuildPlan::Steps(vec![authored_configure.clone()]);
        let mut generated = BuildPlan::Steps(vec![
            authored_configure.clone(),
            step(Some("configure"), "generated configure"),
            step(Some("install"), "generated install"),
        ]);

        merge_authored_steps(&mut generated, &authored).unwrap();

        assert_eq!(
            generated,
            BuildPlan::Steps(vec![
                authored_configure,
                step(Some("install"), "generated install"),
            ])
        );
    }
}
