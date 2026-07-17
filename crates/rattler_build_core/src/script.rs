//! Module for running scripts in different interpreters.
//!
//! This module provides integration between Rattler-Build and the rattler_build_script crate,
//! specifically handling the execution of build scripts within the Output context.

use indexmap::IndexMap;
use minijinja::Value;
use rattler_build_jinja::Jinja;

// Re-export from rattler_build_script
pub use rattler_build_script::{
    ExecutionArgs, ExecutionContext, InterpreterError, ResolvedScriptContents, RuntimeEnv,
    SandboxArguments, SandboxConfiguration, Script, ScriptContent, platform_script_extensions,
};

use crate::{
    env_vars::{self},
    metadata::Output,
};

impl Output {
    /// Helper function to get a jinja renderer for the output's recipe context.
    pub(crate) fn jinja_renderer(&self) -> impl Fn(&str) -> Result<String, String> {
        let selector_config = self.build_configuration.selector_config();
        let jinja = Jinja::new(selector_config.clone()).with_context(&self.recipe.context);
        move |template: &str| jinja.render_str(template).map_err(|e| e.to_string())
    }

    /// Helper method to prepare build script execution arguments
    async fn prepare_build_script(&self) -> Result<ExecutionArgs, std::io::Error> {
        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let host_platform = self.host_platform().platform;
        let env_isolation = self.build_configuration.env_isolation;
        let context = if self.recipe.build().merge_build_and_host_envs {
            ExecutionContext::shared(
                RuntimeEnv::current(),
                &host_prefix,
                self.build_configuration.build_platform.platform,
                host_platform,
            )
        } else {
            ExecutionContext::separate(
                RuntimeEnv::current(),
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
        ));
        env_vars.extend(env_vars::env_vars_from_variant(self.variant()));
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

        let jinja_renderer = self.jinja_renderer();
        let work_dir = &self.build_configuration.directories.work_dir;
        Ok(ExecutionArgs {
            interpreter: self.recipe.build().script.interpreter.clone(),
            script: self.recipe.build().script.resolve_content(
                &self.build_configuration.directories.recipe_dir,
                Some(jinja_renderer),
                platform_script_extensions(),
            )?,
            env_vars: env_vars
                .into_iter()
                .filter_map(|(k, v)| v.map(|v| (k, v)))
                .collect(),
            secrets: IndexMap::new(),
            context,
            work_dir: work_dir.clone(),
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
        let context = exec_args.context.clone();

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
                context,
                Some(jinja_renderer),
                self.build_configuration.sandbox_config(),
                self.build_configuration.env_isolation,
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
