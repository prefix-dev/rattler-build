//! Module for running scripts in different interpreters.
//!
//! This module provides integration between Rattler-Build and the rattler_build_script crate,
//! specifically handling the execution of build scripts within the Output context.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::Instrument;

use indexmap::IndexMap;
use minijinja::Value;
use rattler_build_jinja::{Jinja, JinjaConfig, Variable};

// Re-export from rattler_build_script
pub use rattler_build_script::{
    BuildScriptSection, ExecutionArgs, ExecutionContext, InterpreterError, ResolvedScriptContents,
    RuntimeEnv, SandboxArguments, SandboxConfiguration, Script, ScriptContent, ScriptOutcome,
    platform_script_extensions,
    runner::{
        ExecOutcome, ExecSpec, ExecStatus, GuestInfo, GuestPath, HostPath, LocalRunner, Mount,
        Mounts, OutputSink, OutputStream, Runner, RunnerError, SandboxRunner, Session, SessionSpec,
    },
};

use crate::{consts, env_vars, metadata::Output, packaging::parse_package_files_list};
use rattler_build_recipe::stage1::build::BuildPlan;

/// The `env` of a plan's `build.script`, which is merged into the script
/// environment. `build.steps` set their `env` per step instead, which already
/// takes precedence.
pub(crate) fn plan_script_env(plan: &BuildPlan) -> Option<&IndexMap<String, String>> {
    match plan {
        BuildPlan::Script(script) => Some(script.env()),
        BuildPlan::Steps(_) => None,
    }
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
    scratch_dir: PathBuf,
    recipe_dir: &Path,
    context: ExecutionContext,
    mounts: Mounts,
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

    let scripts: Vec<(Script, Option<usize>)> = match plan {
        BuildPlan::Steps(steps) => steps
            .iter()
            .enumerate()
            .map(|(index, step)| (step.to_script(), Some(index)))
            .collect(),
        BuildPlan::Script(script) => vec![(script.clone(), None)],
    };

    let mut secrets = IndexMap::new();
    let mut sections = Vec::with_capacity(scripts.len());
    for (script, step_index) in scripts {
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
            env: if step_index.is_some() {
                script.env().clone()
            } else {
                Default::default()
            },
            cwd,
            label: step_index.map(|index| format!("step {index}")),
        });
    }

    Ok(ExecutionArgs {
        sections,
        env_vars,
        secrets,
        context,
        work_dir,
        scratch_dir,
        mounts,
        env_isolation,
        channels: Vec::new(),
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
    /// step. Both go through the same execution path. The build directory is
    /// the session's scratch directory, so the session's own files stay in
    /// the directory the user chose for the build.
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
        prepare_build_plan_execution_args(
            &build.plan,
            &self.recipe.context,
            self.build_configuration.selector_config(),
            env_vars,
            self.build_configuration.directories.work_dir.clone(),
            self.build_configuration.directories.build_dir.clone(),
            &self.build_configuration.directories.recipe_dir,
            context,
            self.build_configuration.directories.session_mounts(),
            env_isolation,
            self.build_configuration.experimental,
        )
    }

    /// Run the build script for the output as defined in the recipe's build section.
    ///
    /// This method executes the build script with the configured environment variables,
    /// working directory, and other build settings. The script execution respects the
    /// configured interpreter (bash/cmd/nushell) and runs through the runner of the
    /// tool configuration with the build directory writable and the recipe
    /// directory read-only.
    ///
    /// The script receives the `RATTLER_BUILD_PACKAGE_FILES` channel: a fresh,
    /// empty file it may append package paths to, one per line. The returned
    /// list holds those paths when the script wrote any, and is `None` when
    /// the script left the file empty, so the package falls back to the new
    /// files in the host prefix (see [`Output::create_package`]).
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` if:
    /// - The script file cannot be read or found
    /// - The script execution fails
    /// - The interpreter is not supported or not available
    pub async fn run_build_script(
        &self,
        tool_configuration: &crate::tool_configuration::Configuration,
    ) -> Result<Option<Vec<PathBuf>>, InterpreterError> {
        let span = tracing::info_span!("Running build script");
        async {
            let mut exec_args = self.prepare_build_script().await?;
            env_vars::apply_jobserver(
                &mut exec_args.env_vars,
                tool_configuration.jobserver.as_ref(),
                plan_script_env(&self.recipe.build().plan),
            );
            exec_args.channels = vec![consts::RATTLER_BUILD_PACKAGE_FILES.to_string()];
            let outcome =
                rattler_build_script::run_script(exec_args, tool_configuration.runner.as_ref())
                    .await?;

            Ok(outcome
                .channels
                .get(consts::RATTLER_BUILD_PACKAGE_FILES)
                .and_then(|contents| parse_package_files_list(contents)))
        }
        .instrument(span)
        .await
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
        async {
            let exec_args = self.prepare_build_script().await?;
            rattler_build_script::create_build_script(exec_args).await
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use jiff::Timestamp;
    use rattler_build_script::runner::{ExecOutcome, GuestInfo, OutputSink};
    use rattler_conda_types::Subdir;

    use super::*;
    use crate::tool_configuration::Configuration;
    use crate::types::Directories;

    /// A [`Runner`] whose session records the channels every command
    /// declares and answers each of them with `contents`.
    struct ChannelRunner {
        local: LocalRunner,
        contents: Vec<u8>,
        declared: Arc<Mutex<Vec<String>>>,
    }

    impl ChannelRunner {
        fn new(contents: &[u8]) -> Self {
            Self {
                local: LocalRunner::new(),
                contents: contents.to_vec(),
                declared: Arc::default(),
            }
        }

        fn declared(&self) -> Vec<String> {
            self.declared.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Runner for ChannelRunner {
        fn name(&self) -> &str {
            "channel"
        }

        fn execution_platform(&self) -> Subdir {
            self.local.execution_platform()
        }

        async fn check_usable(&self) -> Result<(), RunnerError> {
            Ok(())
        }

        async fn probe(&self) -> Result<GuestInfo, RunnerError> {
            self.local.probe().await
        }

        async fn start_session(&self, _spec: SessionSpec) -> Result<Box<dyn Session>, RunnerError> {
            Ok(Box::new(ChannelSession {
                contents: self.contents.clone(),
                declared: Arc::clone(&self.declared),
            }))
        }
    }

    struct ChannelSession {
        contents: Vec<u8>,
        declared: Arc<Mutex<Vec<String>>>,
    }

    impl ChannelSession {
        fn outcome(&self, spec: ExecSpec) -> ExecOutcome {
            let channels = spec
                .channels
                .iter()
                .map(|name| (name.clone(), self.contents.clone()))
                .collect();
            *self.declared.lock().unwrap() = spec.channels;
            ExecOutcome {
                status: ExecStatus { code: Some(0) },
                channels,
            }
        }
    }

    #[async_trait]
    impl Session for ChannelSession {
        async fn exec(
            &mut self,
            spec: ExecSpec,
            _sink: &mut dyn OutputSink,
        ) -> Result<ExecOutcome, RunnerError> {
            Ok(self.outcome(spec))
        }

        async fn exec_interactive(&mut self, spec: ExecSpec) -> Result<ExecOutcome, RunnerError> {
            Ok(self.outcome(spec))
        }

        async fn find_executable(
            &mut self,
            _names: &[String],
            _search_paths: &[GuestPath],
        ) -> Result<Option<GuestPath>, RunnerError> {
            Ok(None)
        }
    }

    /// An output of the rendered `rich` recipe whose build tree lives under
    /// `temp_dir` and whose platforms are the current one, so the build
    /// script can be generated on this machine.
    fn output_in(temp_dir: &Path) -> Output {
        let mut output: Output = serde_yaml::from_str(include_str!(
            "../../../test-data/rendered_recipes/rich_recipe.yaml"
        ))
        .unwrap();
        let platform = Subdir::current().unwrap();
        let directories = Directories::builder(
            "rich",
            &temp_dir.join("recipe").join("recipe.yaml"),
            &temp_dir.join("output"),
            &Timestamp::now(),
            platform,
        )
        .build()
        .unwrap();
        for directory in [
            &directories.work_dir,
            &directories.host_prefix,
            &directories.build_prefix,
            &directories.recipe_dir,
        ] {
            fs_err::create_dir_all(directory).unwrap();
        }
        output.build_configuration.directories = directories;
        output.build_configuration.target_platform = platform;
        output.build_configuration.host_platform.platform = platform;
        output.build_configuration.build_platform.platform = platform;
        output
    }

    /// The build script declares the package-files channel and what the
    /// session hands back in it is returned as the parsed list of paths.
    #[tokio::test]
    async fn run_build_script_returns_the_package_files_the_script_listed() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output = output_in(temp_dir.path());
        let runner = Arc::new(ChannelRunner::new(b"bin/keep\n\n  lib/also-keep  \n"));
        let tool_configuration = Configuration::builder()
            .with_runner(Arc::clone(&runner) as Arc<dyn Runner>)
            .finish();

        let package_files = output.run_build_script(&tool_configuration).await.unwrap();

        assert_eq!(
            package_files,
            Some(vec![
                PathBuf::from("bin/keep"),
                PathBuf::from("lib/also-keep")
            ])
        );
        assert_eq!(
            runner.declared(),
            vec![consts::RATTLER_BUILD_PACKAGE_FILES.to_string()]
        );
    }

    /// A script that leaves the channel empty yields no list, so packaging
    /// falls back to the new files in the prefix.
    #[tokio::test]
    async fn run_build_script_returns_no_package_files_for_an_untouched_channel() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output = output_in(temp_dir.path());
        let runner = Arc::new(ChannelRunner::new(b""));
        let tool_configuration = Configuration::builder()
            .with_runner(Arc::clone(&runner) as Arc<dyn Runner>)
            .finish();

        let package_files = output.run_build_script(&tool_configuration).await.unwrap();

        assert_eq!(package_files, None);
    }
}
