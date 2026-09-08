//! Module for running scripts in different interpreters.
//!
//! This module provides integration between Rattler-Build and the rattler_build_script crate,
//! specifically handling the execution of build scripts within the Output context.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use minijinja::Value;
use rattler_build_jinja::{Jinja, JinjaConfig, UndefinedBehavior, Variable};
use sha2::{Digest, Sha256};

// Re-export from rattler_build_script
pub use rattler_build_script::{
    BuildScriptSection, ExecutionArgs, ExecutionContext, InterpreterError, ResolvedScriptContents,
    RuntimeEnv, SandboxArguments, SandboxConfiguration, Script, ScriptContent,
    platform_script_extensions,
    runner::{
        ExecSpec, ExecStatus, GuestInfo, GuestPath, HostPath, LocalRunner, Mount, OutputSink,
        OutputStream, Runner, RunnerError, Session, SessionSpec,
    },
};

use crate::{env_vars, metadata::Output};
use rattler_build_recipe::stage1::build::BuildPlan;

/// Prepare execution arguments for a stage1 build plan.
///
/// Package outputs and staging outputs intentionally share this implementation
/// so `build.script` and `build.steps` resolve content, env, cwd, secrets, and
/// labels the same way in both places.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_build_plan_execution_args(
    plan: &BuildPlan,
    recipe_context: &IndexMap<String, Variable>,
    selector_config: JinjaConfig,
    mut env_vars: HashMap<String, Option<String>>,
    work_dir: PathBuf,
    recipe_dir: &Path,
    context: ExecutionContext,
    sandbox_config: Option<SandboxConfiguration>,
    env_isolation: rattler_build_script::EnvironmentIsolation,
    experimental: bool,
) -> Result<ExecutionArgs, std::io::Error> {
    if matches!(plan, BuildPlan::Steps(_)) && !experimental {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "`build.steps` is an experimental feature: provide the `--experimental` flag to enable it",
        ));
    }

    if let Some(architecture) = context.windows_processor_architecture() {
        env_vars.insert(
            "PROCESSOR_ARCHITECTURE".to_string(),
            Some(architecture.to_string()),
        );
    }
    if let Some(wow64_architecture) = context.windows_processor_architecture_w6432() {
        env_vars.insert(
            "PROCESSOR_ARCHITEW6432".to_string(),
            Some(wow64_architecture.unwrap_or_default().to_string()),
        );
    }

    let mut env_vars: IndexMap<String, String> = env_vars
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect();
    if let BuildPlan::Script(script) = plan {
        env_vars.extend(script.env().clone());
    }

    let scripts: Vec<_> = match plan {
        BuildPlan::Steps(steps) => steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                (
                    step.to_script(),
                    Some(step.name.clone().unwrap_or_else(|| format!("step {index}"))),
                    step.action_context.as_ref(),
                )
            })
            .collect(),
        BuildPlan::Script(script) => vec![(script.clone(), None, None)],
    };

    let mut secrets = IndexMap::new();
    let mut sections = Vec::with_capacity(scripts.len());
    for (index, (mut script, step_label, action_context)) in scripts.into_iter().enumerate() {
        if matches!(plan, BuildPlan::Steps(_)) {
            let output_file = crate::recipe_patch::output_file(&work_dir, index)
                .to_string_lossy()
                .into_owned();
            script.env.insert("OUTPUT_FILE".to_string(), output_file.clone());
            script.env.insert("RATTLER_BUILD_OUTPUT_FILE".to_string(), output_file);
            let build_dir = work_dir.parent().unwrap_or(&work_dir);
            let cache_file = build_dir
                .join(crate::consts::STEP_CACHE_DIRECTORY_NAME)
                .join(format!("{index}.cache"))
                .to_string_lossy()
                .into_owned();
            script.env.insert(crate::consts::RATTLER_BUILD_STEP_CACHE.to_string(), cache_file);
        }
        let mut section_jinja =
            execution_jinja(selector_config.clone(), recipe_context, action_context);
        for (key, value) in env_vars.iter().chain(script.env()) {
            if action_context.is_some_and(|bindings| bindings.contains_key(key)) {
                continue;
            }
            section_jinja
                .context_mut()
                .insert(key.clone(), Value::from_safe_string(value.clone()));
        }
        let section_jinja_renderer = |template: &str| {
            section_jinja
                .render_str(template)
                .map_err(|error| error.to_string())
        };
        let content = script.resolve_content(
            recipe_dir,
            Some(&section_jinja_renderer),
            platform_script_extensions(),
        )?;

        for name in script.secrets() {
            if let Some(value) = context.runtime().var(name) {
                secrets.insert(name.to_string(), value.to_string());
            } else {
                tracing::warn!("Secret {} not found in environment", name);
            }
        }

        let cwd = script
            .cwd
            .as_ref()
            .map(|cwd| context.host().path().join(cwd));
        sections.push(BuildScriptSection {
            interpreter: script.interpreter.clone(),
            content,
            env: if step_label.is_some() {
                script.env().clone()
            } else {
                Default::default()
            },
            cwd,
            label: step_label,
        });
    }

    Ok(ExecutionArgs {
        sections,
        env_vars,
        secrets,
        context,
        work_dir,
        sandbox_config,
        env_isolation,
    })
}

pub(crate) fn execution_jinja(
    mut config: JinjaConfig,
    recipe_context: &IndexMap<String, Variable>,
    action_context: Option<&IndexMap<String, Variable>>,
) -> Jinja {
    if action_context.is_some() {
        config.undefined_behavior = UndefinedBehavior::Strict;
    }
    Jinja::new(config).with_context(action_context.unwrap_or(recipe_context))
}

fn cache_identity(
    section: &BuildScriptSection,
    base_env: &IndexMap<String, String>,
    secrets: &IndexMap<String, String>,
    dependencies: &[u8],
) -> String {
    fn update_map(hasher: &mut Sha256, values: &IndexMap<String, String>, skip: &[&str]) {
        let mut values = values
            .iter()
            .filter(|(key, _)| !skip.contains(&key.as_str()))
            .collect::<Vec<_>>();
        values.sort_unstable_by_key(|(key, _)| *key);
        for (key, value) in values {
            update_bytes(hasher, key.as_bytes());
            update_bytes(hasher, value.as_bytes());
        }
    }

    fn update_bytes(hasher: &mut Sha256, bytes: &[u8]) {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }

    let mut hasher = Sha256::new();
    update_bytes(&mut hasher, match &section.content {
        ResolvedScriptContents::Path(_, _) => b"path",
        ResolvedScriptContents::Inline(_) => b"inline",
        ResolvedScriptContents::Commands(_) => b"commands",
        ResolvedScriptContents::Missing => b"missing",
    });
    if let Some(path) = section.content.path() {
        update_bytes(&mut hasher, path.as_os_str().as_encoded_bytes());
    }
    let inferred_interpreter = section.content.path()
        .and_then(rattler_build_script::determine_interpreter_from_path);
    update_bytes(
        &mut hasher,
        section.interpreter.as_deref()
            .or(inferred_interpreter.as_deref())
            .unwrap_or(if cfg!(windows) { "cmd" } else { "bash" })
            .as_bytes(),
    );
    update_bytes(&mut hasher, section.content.script().as_bytes());
    update_bytes(
        &mut hasher,
        section.cwd.as_ref()
            .map(|cwd| cwd.to_string_lossy())
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    update_map(&mut hasher, &section.env, &[crate::consts::RATTLER_BUILD_STEP_CACHE]);
    // A fresh command timestamp must not invalidate an otherwise identical build.
    update_map(&mut hasher, base_env, &["SOURCE_DATE_EPOCH"]);
    update_map(&mut hasher, secrets, &[]);
    hasher.update(dependencies);
    hex::encode(hasher.finalize())
}

impl Output {
    /// Helper function to get a jinja renderer for the output's recipe context.
    pub(crate) fn jinja_renderer(&self) -> impl Fn(&str) -> Result<String, String> {
        let selector_config = self.build_configuration.selector_config();
        let jinja = Jinja::new(selector_config.clone()).with_context(&self.recipe.context);
        move |template: &str| jinja.render_str(template).map_err(|e| e.to_string())
    }

    /// Helper method to prepare build script execution arguments.
    ///
    /// The build script is always expressed as an ordered list of sections: a
    /// `build.script` is a single section, and `build.steps` are one section per
    /// step. Both go through the same execution path.
    async fn prepare_build_script(&self) -> Result<ExecutionArgs, std::io::Error> {
        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let host_platform = self.host_platform().platform;
        let env_isolation = self.build_configuration.env_isolation;
        let build = self.recipe.build();
        let runtime = RuntimeEnv::current();
        let context = if build.merge_build_and_host_envs {
            ExecutionContext::shared(
                runtime.clone(),
                &host_prefix,
                self.build_configuration.build_platform.platform,
                host_platform,
            )
        } else {
            ExecutionContext::separate(
                runtime.clone(),
                &self.build_configuration.directories.build_prefix,
                self.build_configuration.build_platform.platform,
                &host_prefix,
                host_platform,
            )
        };

        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(
            &host_prefix,
            &target_platform,
            &host_platform,
            &self.build_configuration.build_platform.platform,
            env_isolation,
            &self.build_configuration.directories.work_dir,
            context.runtime(),
        ));
        env_vars.extend(env_vars::env_vars_from_variant(self.variant()));
        let mut args = prepare_build_plan_execution_args(
            &build.plan,
            &self.recipe.context,
            self.build_configuration.selector_config(),
            env_vars,
            self.build_configuration.directories.work_dir.clone(),
            &self.build_configuration.directories.recipe_dir,
            context,
            self.build_configuration.sandbox_config().cloned(),
            env_isolation,
            self.build_configuration.experimental,
        )?;
        if let Some(source_dir) = &self.build_configuration.directories.source_dir {
            for section in &mut args.sections {
                if section.cwd.is_none() {
                    section.cwd = Some(source_dir.clone());
                }
            }
        }
        Ok(args)
    }

    /// Run the build script for the output as defined in the recipe's build section.
    ///
    /// This method executes the build script with the configured environment variables,
    /// working directory, and other build settings. The script execution respects the
    /// configured interpreter (bash/cmd/nushell) and sandbox settings.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if:
    /// - The script file cannot be read or found
    /// - The script execution fails
    /// - The interpreter is not supported or not available
    pub async fn run_build_script(&self) -> Result<(), InterpreterError> {
        let span = tracing::info_span!("Running build script");
        let _enter = span.enter();

        // Reset the package files override list before running the build
        // script. This ensures that we do not pick up paths from a previous
        // run if the script does not write to the file this time.
        let package_files_path = self
            .build_configuration
            .directories
            .package_files_list_path();
        match fs_err::remove_file(&package_files_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }

        let exec_args = self.prepare_build_script().await?;
        if self.recipe.build().plan.steps().is_none() {
            rattler_build_script::run_script(exec_args).await?;
            return Ok(());
        }

        let output_root = &exec_args.work_dir;
        crate::recipe_patch::prepare_output_directory(output_root)?;
        fs_err::create_dir_all(
            self.build_configuration.directories.build_dir
                .join(crate::consts::STEP_CACHE_DIRECTORY_NAME),
        )?;
        let dependency_identity = serde_json::to_vec(&(
            &self.finalized_dependencies,
            &self.recipe.build().plan,
            exec_args.env_isolation,
            &exec_args.sandbox_config,
            exec_args.context.build().platform(),
            exec_args.context.host().platform(),
        )).map_err(std::io::Error::other)?;
        let process_env = rattler_build_script::runner::resolve_process_env(
            exec_args.env_isolation,
            &exec_args.env_vars,
            &exec_args.secrets,
            exec_args.context.runtime(),
        );
        for (section_index, section) in exec_args.sections.iter().cloned().enumerate() {
            let cache_path = section.env
                .get(crate::consts::RATTLER_BUILD_STEP_CACHE)
                .map(PathBuf::from)
                .ok_or_else(|| std::io::Error::other("build step is missing its cache declaration path"))?;
            let root = section.cwd.clone().unwrap_or_else(|| exec_args.work_dir.clone());
            let identity = cache_identity(
                &section, &process_env, &exec_args.secrets, &dependency_identity,
            );
            let cache = crate::step_cache::StepCacheEntry::new(
                cache_path.clone(), root, identity,
                crate::recipe_patch::output_file(output_root, section_index),
            );
            let cache_hit = match cache.probe() {
                Ok(hit) => hit,
                Err(error) => {
                    tracing::warn!(
                        "Ignoring invalid build step cache {}: {}",
                        cache_path.display(), error
                    );
                    false
                }
            };
            if cache_hit {
                tracing::info!(
                    "Skipping build step {} (cache hit)",
                    section.label.as_deref().unwrap_or("unnamed")
                );
                continue;
            }
            cache.begin()?;
            let mut section_args = exec_args.clone();
            section_args.sections = vec![section];
            rattler_build_script::run_script(section_args).await?;
            cache.commit()?;
        }

        Ok(())
    }

    /// Create the build script files without executing them.
    ///
    /// This method generates the build script and environment setup files in the working
    /// directory but does not execute them. This is useful for debugging or when you want
    /// to inspect or modify the scripts before running them manually.
    ///
    /// The method creates two files:
    /// - A build environment setup file (`build_env.sh`/`build_env.bat`)
    /// - The main build script file (`conda_build.sh`/`conda_build.bat`)
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if:
    /// - The script file cannot be read or found
    /// - The script files cannot be written to the working directory
    pub async fn create_build_script(&self) -> Result<(), std::io::Error> {
        let span = tracing::info_span!("Creating build script");
        let _enter = span.enter();

        if self.recipe.build().plan.steps().is_some() {
            crate::recipe_patch::prepare_output_directory(
                &self.build_configuration.directories.work_dir,
            )?;
        }
        let exec_args = self.prepare_build_script().await?;
        rattler_build_script::create_build_script(exec_args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_identity_tracks_effective_environment_but_not_command_timestamp() {
        let section = BuildScriptSection {
            interpreter: Some("bash".to_string()),
            content: ResolvedScriptContents::Inline("build".to_string()),
            env: IndexMap::new(),
            cwd: None,
            label: Some("build".to_string()),
        };
        let mut env = IndexMap::from([
            ("SOURCE_DATE_EPOCH".to_string(), "1".to_string()),
            ("FLAGS".to_string(), "first".to_string()),
        ]);
        let mut secrets = IndexMap::from([("TOKEN".to_string(), "one".to_string())]);
        let identity = cache_identity(&section, &env, &secrets, b"solve-one");

        env.insert("SOURCE_DATE_EPOCH".to_string(), "2".to_string());
        assert_eq!(
            identity,
            cache_identity(&section, &env, &secrets, b"solve-one")
        );
        env.insert("FLAGS".to_string(), "second".to_string());
        assert_ne!(
            identity,
            cache_identity(&section, &env, &secrets, b"solve-one")
        );
        env.insert("FLAGS".to_string(), "first".to_string());
        secrets.insert("TOKEN".to_string(), "two".to_string());
        assert_ne!(
            identity,
            cache_identity(&section, &env, &secrets, b"solve-one")
        );
        secrets.insert("TOKEN".to_string(), "one".to_string());
        assert_ne!(
            identity,
            cache_identity(&section, &env, &secrets, b"solve-two")
        );
    }
}
