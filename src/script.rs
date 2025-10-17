//! Module for running scripts in different interpreters.
//!
//! This module provides integration between rattler-build and the rattler_build_script crate,
//! specifically handling the execution of build scripts within the Output context.

use indexmap::IndexMap;
use minijinja::Value;
use rattler_build_jinja::Jinja;
use rattler_conda_types::Platform;
use std::{collections::HashMap, collections::HashSet};

// Re-export from rattler_build_script
pub use rattler_build_script::{
    Debug as ScriptDebug, ExecutionArgs, InterpreterError, ResolvedScriptContents,
    SandboxArguments, SandboxConfiguration, Script, ScriptContent,
};

use crate::{
    env_vars::{self},
    metadata::Output,
};

impl Output {
    /// Add environment variables from the variant to the environment variables.
    fn env_vars_from_variant(&self) -> HashMap<String, Option<String>> {
        let languages: HashSet<&str> =
            HashSet::from(["PERL", "LUA", "R", "NUMPY", "PYTHON", "RUBY", "NODEJS"]);
        self.variant()
            .iter()
            .filter_map(|(k, v)| {
                let key_upper = k.normalize().to_uppercase();
                if !languages.contains(key_upper.as_str()) {
                    Some((k.normalize(), Some(v.to_string())))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Helper method to prepare build script execution arguments
    async fn prepare_build_script(&self) -> Result<ExecutionArgs, std::io::Error> {
        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));
        env_vars.extend(self.env_vars_from_variant());

        let selector_config = self.build_configuration.selector_config();
        let jinja = Jinja::new(selector_config.clone()).with_context(&self.recipe.context);

        let build_prefix = if self.recipe.build().merge_build_and_host_envs {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        let work_dir = &self.build_configuration.directories.work_dir;
        let jinja_renderer = |template: &str| jinja.render_str(template).map_err(|e| e.to_string());
        Ok(ExecutionArgs {
            script: self.recipe.build().script.resolve_content(
                &self.build_configuration.directories.recipe_dir,
                Some(jinja_renderer),
                if cfg!(windows) { &["bat"] } else { &["sh"] },
            )?,
            env_vars: env_vars
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| (k, v)))
                .collect(),
            secrets: IndexMap::new(),
            build_prefix: build_prefix.map(|p| p.to_owned()),
            run_prefix: host_prefix,
            execution_platform: Platform::current(),
            work_dir: work_dir.clone(),
            sandbox_config: self.build_configuration.sandbox_config().cloned(),
            debug: ScriptDebug::new(self.build_configuration.debug.is_enabled()),
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

        let exec_args = self.prepare_build_script().await?;
        let build_prefix = if self.recipe.build().merge_build_and_host_envs {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        // Create Jinja context with environment variables
        let mut jinja = Jinja::new(self.build_configuration.selector_config())
            .with_context(&self.recipe.context);

        // Add env vars to jinja context
        for (k, v) in &exec_args.env_vars {
            jinja
                .context_mut()
                .insert(k.clone(), Value::from_safe_string(v.clone()));
        }

        let jinja_renderer = |template: &str| -> Result<String, String> {
            jinja.render_str(template).map_err(|e| e.to_string())
        };

        self.recipe
            .build()
            .script
            .run_script(
                exec_args
                    .env_vars
                    .into_iter()
                    .map(|(k, v)| (k, Some(v)))
                    .collect(),
                &self.build_configuration.directories.work_dir,
                &self.build_configuration.directories.recipe_dir,
                &self.build_configuration.directories.host_prefix,
                build_prefix,
                Some(jinja_renderer),
                self.build_configuration.sandbox_config(),
                ScriptDebug::new(self.build_configuration.debug.is_enabled()),
            )
            .await?;

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
