//! Experimental pre-solve recipe metadata step execution.

use std::{collections::HashMap, path::Component};

use miette::{IntoDiagnostic, WrapErr};
use rattler_build_recipe::stage1::{Requirements, build::BuildPlan};
use rattler_build_script::{EnvironmentIsolation, ExecutionContext, RuntimeEnv};
use serde_json::Value;
use rattler_conda_types::Platform;
use sha2::{Digest, Sha256};

use crate::{
    metadata::Output, render::resolved_dependencies::RunExportsDownload,
    tool_configuration::Configuration, types::Directories,
};

/// Metadata output retained between the bootstrap and final source compilation.
pub struct MetadataOutput {
    /// Native-valued source patch directives emitted by the bootstrap steps.
    pub contents: String,
    /// Digest of emitted directives and the compiled bootstrap action provenance.
    pub fingerprint: String,
}

impl MetadataOutput {
    /// Patch source before compiling generated actions and expanding variants.
    pub fn apply_to_source(&self, source: &Value) -> miette::Result<Value> {
        let mut generated = source.clone();
        crate::recipe_patch::apply_metadata_output(&mut generated, &self.contents)?;
        merge_authored_steps(&mut generated, source)?;
        if let Some(build) = generated.get_mut("build").and_then(Value::as_object_mut) {
            let variant = build.entry("variant").or_insert_with(|| serde_json::json!({}));
            let variant = variant.as_object_mut().ok_or_else(|| {
                miette::miette!("build.variant must be a mapping")
            })?;
            let keys = variant.entry("use_keys").or_insert_with(|| serde_json::json!([]));
            if keys.is_string() {
                *keys = Value::Array(vec![std::mem::take(keys)]);
            }
            let keys = keys.as_array_mut().ok_or_else(|| {
                miette::miette!("build.variant.use_keys must be a string or list")
            })?;
            if !keys.iter().any(|key| key.as_str() == Some("rattler_build_metadata")) {
                keys.push(Value::String("rattler_build_metadata".into()));
            }
            build.remove("metadata");
        }
        Ok(generated)
    }
}

fn step_name(step: &Value) -> Option<&str> {
    step.get("name").and_then(Value::as_str)
}

fn merge_authored_steps(generated: &mut Value, authored: &Value) -> miette::Result<()> {
    let Some(generated_steps) = generated
        .pointer_mut("/build/steps")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    let authored_steps = authored
        .pointer("/build/steps")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if generated_steps == authored_steps {
        return Ok(());
    }
    let appended = generated_steps.starts_with(authored_steps);
    let metadata_steps = if appended {
        generated_steps.drain(..authored_steps.len());
        std::mem::take(generated_steps)
    } else {
        std::mem::take(generated_steps)
    };
    let mut names = std::collections::HashSet::new();
    for step in &metadata_steps {
        let name = step_name(step).ok_or_else(|| {
            miette::miette!("build.metadata generated an unnamed build step; generated steps must have literal names so recipes can override them")
        })?;
        if !names.insert(name) {
            return Err(miette::miette!("build.metadata generated duplicate build step name `{name}`"));
        }
    }
    let mut authored_by_name = HashMap::new();
    for step in authored_steps {
        if let Some(name) = step_name(step)
            && authored_by_name.insert(name, step).is_some()
        {
            return Err(miette::miette!("duplicate recipe-authored build step name `{name}`"));
        }
    }
    if appended {
        generated_steps.extend_from_slice(authored_steps);
        generated_steps.extend(metadata_steps.into_iter().filter(|step| {
            step_name(step).is_none_or(|name| !authored_by_name.contains_key(name))
        }));
    } else {
        let mut consumed = std::collections::HashSet::new();
        for step in metadata_steps {
            if let Some(name) = step_name(&step)
                && let Some(authored_step) = authored_by_name.get(name)
            {
                consumed.insert(name.to_owned());
                generated_steps.push((*authored_step).clone());
            } else {
                generated_steps.push(step);
            }
        }
        generated_steps.extend(authored_steps.iter().filter(|step| {
            step_name(step).is_none_or(|name| !consumed.contains(name))
        }).cloned());
    }
    Ok(())
}

/// Execute the compiled metadata plan in an explicit pre-solve environment.
pub async fn run_metadata_step(
    output: &Output,
    tool_configuration: &Configuration,
) -> miette::Result<Option<MetadataOutput>> {
    let Some(plan) = &output.recipe.build.metadata else {
        return Ok(None);
    };

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
        build: plan.requirements.build.clone(),
        host: plan.requirements.host.clone(),
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
        "BUILD_PREFIX".to_string(),
        Some(directories.build_prefix.to_string_lossy().into_owned()),
    );
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

    for step in &plan.steps {
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
    let jinja = crate::script::execution_jinja(
        output.build_configuration.selector_config(),
        &output.recipe.context,
        step.action_context.as_ref(),
    );
    let renderer = |template: &str| jinja.render_str(template).map_err(|error| error.to_string());
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
            env.clone(),
            &directories.work_dir,
            recipe_dir,
            context,
            Some(renderer),
            output.build_configuration.sandbox_config(),
            EnvironmentIsolation::Strict,
        )
        .await
        .map_err(|error| miette::miette!("metadata step failed: {error}"))?;
    }

    if !output_file.is_file() {
        return Err(miette::miette!(
            "metadata step completed without creating OUTPUT_FILE at {}",
            output_file.display()
        ));
    }
    let contents = fs_err::read_to_string(&output_file)
        .into_diagnostic()
        .wrap_err("failed to read metadata-step output")?;
    let mut digest = Sha256::new();
    digest.update(contents.as_bytes());
    digest.update(
        serde_json::to_vec(&plan.provenance)
            .into_diagnostic()
            .wrap_err("failed to serialize metadata action provenance")?,
    );
    let metadata = MetadataOutput {
        contents,
        fingerprint: hex::encode(digest.finalize()),
    };

    // Keep the bootstrap output alive until execution has completely finished.
    drop(bootstrap);
    Ok(Some(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authored_steps_replace_generated_names_and_allow_unnamed_additions() {
        let authored = json!({"build": {"steps": [
            {"name": "install", "run": "authored install"},
            {"run": "authored cleanup"}
        ]}});
        let mut generated = json!({"build": {"steps": [
            {"name": "configure", "uses": "./configure.yaml", "with": {"enabled": true}},
            {"name": "install", "uses": "./install.yaml"}
        ]}});
        merge_authored_steps(&mut generated, &authored).unwrap();
        assert_eq!(generated["build"]["steps"], json!([
            {"name": "configure", "uses": "./configure.yaml", "with": {"enabled": true}},
            {"name": "install", "run": "authored install"},
            {"run": "authored cleanup"}
        ]));
    }

    #[test]
    fn appended_generated_steps_preserve_authored_order_and_do_not_duplicate_overrides() {
        let authored = json!({"build": {"steps": [
            {"name": "configure", "run": "authored configure"}
        ]}});
        let mut generated = json!({"build": {"steps": [
            {"name": "configure", "run": "authored configure"},
            {"name": "configure", "uses": "./configure.yaml"},
            {"name": "install", "run": "generated install"}
        ]}});
        merge_authored_steps(&mut generated, &authored).unwrap();
        assert_eq!(generated["build"]["steps"], json!([
            {"name": "configure", "run": "authored configure"},
            {"name": "install", "run": "generated install"}
        ]));
    }

    #[test]
    fn generated_source_preserves_native_inputs_and_does_not_repeat_bootstrap() {
        let metadata = MetadataOutput {
            contents: "build.steps [{\"name\":\"compile\",\"uses\":\"./compile.yaml\",\"with\":{\"debug\":true,\"levels\":[1,2]}}]".into(),
            fingerprint: String::new(),
        };
        let source = json!({"package": {"name": "example", "version": "1"},
            "build": {"metadata": {"uses": "./metadata.yaml"}}});
        let generated = metadata.apply_to_source(&source).unwrap();
        assert!(generated["build"].get("metadata").is_none());
        assert_eq!(generated["build"]["steps"][0]["with"], json!({"debug": true, "levels": [1, 2]}));
        assert_eq!(generated["build"]["steps"][0]["uses"], "./compile.yaml");
    }

    #[test]
    fn metadata_hash_key_preserves_authored_and_generated_variant_usage() {
        let metadata = MetadataOutput {
            contents: "build.variant.use_keys.append [\"libpng\"]".into(),
            fingerprint: String::new(),
        };
        let source = json!({"build": {"variant": {"use_keys": "zlib"}}});
        let generated = metadata.apply_to_source(&source).unwrap();
        assert_eq!(
            generated["build"]["variant"]["use_keys"],
            json!(["zlib", "libpng", "rattler_build_metadata"])
        );
    }

    #[test]
    fn generated_scalar_variant_usage_is_normalized_before_fingerprinting() {
        let metadata = MetadataOutput {
            contents: "build.variant.use_keys \"python\"".into(),
            fingerprint: String::new(),
        };
        let generated = metadata.apply_to_source(&json!({"build": {}})).unwrap();
        assert_eq!(
            generated["build"]["variant"]["use_keys"],
            json!(["python", "rattler_build_metadata"])
        );
    }

    #[test]
    fn generated_invocations_require_explicit_stable_names() {
        for steps in [
            json!([{"uses": "./compile.yaml"}]),
            json!([{"name": "compile", "run": "a"}, {"name": "compile", "run": "b"}]),
        ] {
            let mut generated = json!({"build": {"steps": steps}});
            assert!(merge_authored_steps(&mut generated, &json!({})).is_err());
        }
    }
}
