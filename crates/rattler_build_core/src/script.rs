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
use rattler_build_jinja::{Jinja, JinjaConfig, Variable};

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
use rattler_build_recipe::stage1::build::{BuildPlan, Step, parse_step_package_reference};

fn reusable_step_path(
    reference: &str,
    recipe_dir: &Path,
    context: &ExecutionContext,
) -> Result<PathBuf, std::io::Error> {
    let candidates = if let Some((provider, step)) = parse_step_package_reference(reference)
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?
    {
        [context.build().path(), context.host().path()]
            .into_iter()
            .flat_map(|prefix| {
                let base = prefix
                    .join("etc/rattler-build/steps")
                    .join(provider)
                    .join(step);
                [base.with_extension("yaml"), base]
            })
            .collect::<Vec<_>>()
    } else {
        let path = PathBuf::from(reference);
        let path = if path.is_absolute() {
            path
        } else {
            recipe_dir.join(path)
        };
        vec![path.clone(), path.with_extension("yaml")]
    };
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("reusable build step `{reference}` was not found"),
            )
        })
}

fn resolve_reusable_step(
    step: &Step,
    step_index: usize,
    recipe_dir: &Path,
    context: &ExecutionContext,
) -> Result<Vec<(Script, Option<String>)>, std::io::Error> {
    let Some(reference) = &step.uses else {
        let label = step
            .name
            .clone()
            .unwrap_or_else(|| format!("step {step_index}"));
        return Ok(vec![(step.to_script(), Some(label))]);
    };
    let path = reusable_step_path(reference, recipe_dir, context)?;
    let contents = fs_err::read_to_string(&path)?;
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum ReusableFile {
        Pipeline { steps: Vec<Step> },
        Step(Box<Step>),
    }
    let reusable: ReusableFile = serde_yaml::from_str(&contents).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to parse reusable step {}: {error}", path.display()),
        )
    })?;
    let reusable_steps =
        match reusable {
            ReusableFile::Pipeline { steps } => BuildPlan::Steps(steps)
                .select_steps(None)
                .map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid reusable pipeline {}: {error}", path.display()),
                    )
                })?,
            ReusableFile::Step(step) => {
                BuildPlan::Steps(vec![*step])
                    .select_steps(None)
                    .map_err(|error| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("invalid reusable step {}: {error}", path.display()),
                        )
                    })?
            }
        };
    if reusable_steps.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "reusable pipeline {} contains no required steps",
                path.display()
            ),
        ));
    }

    let mut scripts = Vec::with_capacity(reusable_steps.len());
    for (index, reusable) in reusable_steps.into_iter().enumerate() {
        if reusable.uses.is_some() || !reusable.requirements.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "reusable pipeline {} may not contain nested uses or requirements",
                    path.display()
                ),
            ));
        }
        if reusable.run.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("reusable step {} must define run", path.display()),
            ));
        }
        let nested_name = reusable
            .name
            .clone()
            .unwrap_or_else(|| format!("step {index}"));
        let outer = step
            .name
            .clone()
            .unwrap_or_else(|| format!("step {step_index}"));
        let label = Some(format!("{outer}/{nested_name}"));
        let mut script = reusable.to_script();
        if step.interpreter.is_some() {
            script.interpreter.clone_from(&step.interpreter);
        }
        if step.cwd.is_some() {
            script.cwd.clone_from(&step.cwd);
        }
        script.env.extend(step.env.clone());
        scripts.push((script, label));
    }
    Ok(scripts)
}

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
        BuildPlan::Steps(steps) => {
            let mut scripts = Vec::new();
            for (index, step) in steps.iter().enumerate() {
                scripts.extend(resolve_reusable_step(step, index, recipe_dir, &context)?);
            }
            scripts
        }
        BuildPlan::Script(script) => vec![(script.clone(), None)],
    };

    let mut secrets = IndexMap::new();
    let mut sections = Vec::with_capacity(scripts.len());
    for (script, step_label) in scripts {
        let mut section_jinja = Jinja::new(selector_config.clone()).with_context(recipe_context);
        for (key, value) in env_vars.iter().chain(script.env()) {
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
        rattler_build_script::run_script(exec_args).await?;

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

        let exec_args = self.prepare_build_script().await?;
        rattler_build_script::create_build_script(exec_args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::Platform;

    #[test]
    fn reusable_step_paths_resolve_local_and_packaged_files() {
        let temp = tempfile::tempdir().unwrap();
        let recipe = temp.path().join("recipe");
        let build_prefix = temp.path().join("build");
        let host_prefix = temp.path().join("host");
        fs_err::create_dir_all(recipe.join("steps")).unwrap();
        fs_err::write(
            recipe.join("steps/lint.yaml"),
            "steps:\n  - name: check\n    run: lint\n  - name: report\n    depends_on: [check]\n    run: report\n",
        )
        .unwrap();
        let packaged = build_prefix.join("etc/rattler-build/steps/cargo/build.yaml");
        fs_err::create_dir_all(packaged.parent().unwrap()).unwrap();
        fs_err::write(&packaged, "run: cargo build\n").unwrap();
        let context = ExecutionContext::separate(
            RuntimeEnv::current(),
            &build_prefix,
            Platform::current(),
            &host_prefix,
            Platform::current(),
        );

        assert_eq!(
            reusable_step_path("./steps/lint.yaml", &recipe, &context).unwrap(),
            recipe.join("steps/lint.yaml")
        );
        assert_eq!(
            reusable_step_path("cargo:build", &recipe, &context).unwrap(),
            packaged
        );
        assert!(reusable_step_path("cargo:missing", &recipe, &context).is_err());

        let wrapper: Step = serde_yaml::from_str("uses: ./steps/lint.yaml\n").unwrap();
        let scripts = resolve_reusable_step(&wrapper, 0, &recipe, &context).unwrap();
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0].1.as_deref(), Some("step 0/check"));
        assert_eq!(scripts[1].1.as_deref(), Some("step 0/report"));
    }
}
