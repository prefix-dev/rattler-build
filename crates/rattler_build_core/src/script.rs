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
    RuntimeEnv, SandboxArguments, SandboxConfiguration, SandboxRequest, Script, ScriptContent,
    platform_script_extensions,
    runner::{
        ExecSpec, ExecStatus, GuestInfo, GuestPath, HostPath, LocalRunner, Mount, OutputSink,
        OutputStream, Runner, RunnerError, Session, SessionSpec,
    },
};

use crate::{env_vars, metadata::Output};
use rattler_build_recipe::stage1::build::BuildPlan;

fn resolve_sandbox_request(
    request: &SandboxRequest,
    env: &HashMap<String, Option<String>>,
    work_dir: &Path,
) -> Result<SandboxRequest, std::io::Error> {
    let resolve_paths = |paths: &[PathBuf]| {
        paths
            .iter()
            .map(|path| {
                let expanded = expand_sandbox_path(path, env)?;
                Ok(if expanded.is_absolute() {
                    expanded
                } else {
                    work_dir.join(expanded)
                })
            })
            .collect::<Result<Vec<_>, std::io::Error>>()
    };

    Ok(SandboxRequest {
        network: request.network,
        read: resolve_paths(&request.read)?,
        read_execute: resolve_paths(&request.read_execute)?,
        read_write: resolve_paths(&request.read_write)?,
        reason: request.reason.clone(),
    })
}

fn expand_sandbox_path(
    path: &Path,
    env: &HashMap<String, Option<String>>,
) -> Result<PathBuf, std::io::Error> {
    let input = path.to_string_lossy();
    let chars: Vec<char> = input.chars().collect();
    let mut output = String::new();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '$' {
            let (name, next) = if chars.get(index + 1) == Some(&'{') {
                let end = chars[index + 2..]
                    .iter()
                    .position(|character| *character == '}')
                    .map(|offset| index + 2 + offset)
                    .ok_or_else(|| invalid_sandbox_variable(&input, "missing closing `}`"))?;
                (chars[index + 2..end].iter().collect::<String>(), end + 1)
            } else {
                let end = (index + 1..chars.len())
                    .find(|position| {
                        let character = chars[*position];
                        !(character == '_' || character.is_ascii_alphanumeric())
                    })
                    .unwrap_or(chars.len());
                if end == index + 1 {
                    output.push(chars[index]);
                    index += 1;
                    continue;
                }
                (chars[index + 1..end].iter().collect::<String>(), end)
            };
            let value = env.get(&name).and_then(Option::as_deref).ok_or_else(|| {
                invalid_sandbox_variable(&input, &format!("unknown variable `{name}`"))
            })?;
            output.push_str(value);
            index = next;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }

    Ok(PathBuf::from(output))
}

fn invalid_sandbox_variable(path: &str, detail: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("invalid recipe sandbox path '{path}': {detail}"),
    )
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
    mut sandbox_config: Option<SandboxConfiguration>,
    env_isolation: rattler_build_script::EnvironmentIsolation,
    experimental: bool,
) -> Result<ExecutionArgs, std::io::Error> {
    if matches!(plan, BuildPlan::Steps(_)) && !experimental {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "`build.steps` is an experimental feature: provide the `--experimental` flag to enable it",
        ));
    }

    if let BuildPlan::Script(script) = plan
        && let Some(request) = script.sandbox.as_ref()
    {
        let request = resolve_sandbox_request(request, &env_vars, &work_dir)?;
        let config = sandbox_config.get_or_insert_with(SandboxConfiguration::for_current_platform);
        config.with_cwd(&work_dir).authorize_request(&request)?;
        if let Some(reason) = request.reason.as_deref() {
            tracing::info!("authorized recipe sandbox request: {reason}");
        }
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

        prepare_build_plan_execution_args(
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
        )
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

    #[test]
    fn sandbox_paths_expand_build_environment_variables() {
        let env = HashMap::from([
            ("SRC_DIR".to_string(), Some("/work/source".to_string())),
            ("CACHE".to_string(), Some("cache".to_string())),
        ]);
        let request = SandboxRequest {
            read: vec![PathBuf::from("$SRC_DIR/include")],
            read_write: vec![PathBuf::from("${SRC_DIR}/$CACHE")],
            ..Default::default()
        };

        let resolved = resolve_sandbox_request(&request, &env, Path::new("/work")).unwrap();
        assert_eq!(resolved.read, vec![PathBuf::from("/work/source/include")]);
        assert_eq!(
            resolved.read_write,
            vec![PathBuf::from("/work/source/cache")]
        );
    }

    #[test]
    fn sandbox_paths_reject_unknown_variables() {
        let request = SandboxRequest {
            read: vec![PathBuf::from("$NOT_DEFINED/data")],
            ..Default::default()
        };
        let error =
            resolve_sandbox_request(&request, &HashMap::new(), Path::new("/work")).unwrap_err();
        assert!(error.to_string().contains("NOT_DEFINED"));
    }
}
