//! Module for running scripts in different interpreters.
//!
//! This module provides integration between Rattler-Build and the rattler_build_script crate,
//! specifically handling the execution of build scripts within the Output context.

use indexmap::IndexMap;
use minijinja::Value;
use rattler_build_jinja::Jinja;

// Re-export from rattler_build_script
pub use rattler_build_script::{
    BuildScriptSection, ExecutionArgs, InterpreterError, ResolvedScriptContents, RuntimeEnv,
    SandboxArguments, SandboxConfiguration, Script, ScriptContent, platform_script_extensions,
};

use crate::{
    env_vars::{self},
    metadata::Output,
};
use rattler_build_recipe::stage1::build::BuildPlan;

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
        let env_isolation = self.build_configuration.env_isolation;
        let build = self.recipe.build();
        if matches!(&build.plan, BuildPlan::Steps(_)) && !self.build_configuration.experimental {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "`build.steps` is an experimental feature: provide the `--experimental` flag to enable it",
            ));
        }

        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(
            &host_prefix,
            &target_platform,
            env_isolation,
            &self.build_configuration.directories.work_dir,
        ));
        env_vars.extend(env_vars::env_vars_from_variant(self.variant()));
        let mut env_vars: IndexMap<String, String> = env_vars
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .collect();
        if let BuildPlan::Script(script) = &build.plan {
            env_vars.extend(script.env().clone());
        }

        let build_prefix = if self.recipe.build().merge_build_and_host_envs {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        let recipe_dir = &self.build_configuration.directories.recipe_dir;

        // Unify the two build-authoring modes: `steps` are the sections, and a
        // plain `script` is a single section. When steps mode was not selected,
        // always resolve the script even if it is default so legacy build.sh /
        // build.bat auto-discovery still works.
        let scripts: Vec<(Script, Option<usize>)> = match &build.plan {
            BuildPlan::Steps(steps) => steps
                .iter()
                .enumerate()
                .map(|(index, step)| (step.to_script(), Some(index)))
                .collect(),
            BuildPlan::Script(script) => vec![(script.clone(), None)],
        };

        let runtime = RuntimeEnv::current();
        let mut secrets = IndexMap::new();
        let work_dir = self.build_configuration.directories.work_dir.clone();
        let mut sections = Vec::with_capacity(scripts.len());

        for (script, step_index) in scripts {
            // Render each section with both the whole-build environment and
            // that section's scoped env. This preserves legacy `build.script`
            // behavior and makes step-local env visible to that step's `run`
            // templates without leaking it to later steps.
            let mut section_jinja = Jinja::new(self.build_configuration.selector_config())
                .with_context(&self.recipe.context);
            for (k, v) in env_vars.iter().chain(script.env()) {
                section_jinja
                    .context_mut()
                    .insert(k.clone(), Value::from_safe_string(v.clone()));
            }
            let section_jinja_renderer = |template: &str| {
                section_jinja
                    .render_str(template)
                    .map_err(|e| e.to_string())
            };
            let content = script.resolve_content(
                recipe_dir,
                Some(&section_jinja_renderer),
                platform_script_extensions(),
            )?;

            // Secrets are whole-build (used for redaction); resolve declared
            // names from the runtime environment.
            for name in script.secrets() {
                if let Some(value) = runtime.var(name) {
                    secrets.insert(name.to_string(), value.to_string());
                } else {
                    tracing::warn!("Secret {} not found in environment", name);
                }
            }

            let cwd = script.cwd.as_ref().map(|cwd| host_prefix.join(cwd));

            sections.push(BuildScriptSection {
                interpreter: script.interpreter.clone(),
                content,
                env: step_index
                    .is_some()
                    .then(|| script.env().clone())
                    .unwrap_or_default(),
                cwd,
                label: step_index.map(|index| format!("step {index}")),
            });
        }

        Ok(ExecutionArgs {
            sections,
            env_vars,
            secrets,
            build_prefix: build_prefix.map(|p| p.to_owned()),
            run_prefix: host_prefix,
            runtime,
            work_dir,
            sandbox_config: self.build_configuration.sandbox_config().cloned(),
            env_isolation,
        })
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
