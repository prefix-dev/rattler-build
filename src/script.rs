//! Module for running scripts in different interpreters.
//!
//! This module provides integration between rattler-build and the rattler_build_script crate,
//! specifically handling the execution of build scripts within the Output context.
//!
//! Also supports pipeline execution with multi-step builds and external pipeline files.

use indexmap::IndexMap;
use minijinja::Value;
use rattler_build_jinja::{Jinja, Variable};
use rattler_conda_types::Platform;
use std::{collections::HashMap, collections::HashSet, path::Path};

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
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));
        env_vars.extend(self.env_vars_from_variant());

        let jinja_renderer = self.jinja_renderer();

        let build_prefix = if self.recipe.build().merge_build_and_host_envs {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        let work_dir = &self.build_configuration.directories.work_dir;
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

    /// Run the build script or pipeline for the output as defined in the recipe's build section.
    ///
    /// This method checks if there's a pipeline defined. If so, it runs the pipeline steps
    /// sequentially. Otherwise, it executes the traditional build script.
    ///
    /// The script execution respects the configured interpreter (bash/cmd/nushell) and
    /// sandbox settings.
    ///
    /// # Errors
    ///
    /// Returns an `InterpreterError` if:
    /// - The script file cannot be read or found
    /// - The script execution fails
    /// - The interpreter is not supported or not available
    /// - A pipeline file cannot be loaded
    pub async fn run_build_script(&self) -> Result<(), InterpreterError> {
        // Check if there's a pipeline defined
        if let Some(pipeline) = &self.recipe.build().pipeline {
            return self.run_build_pipeline(pipeline).await;
        }

        // Traditional script execution
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

    /// Run the build pipeline for the output.
    ///
    /// This method executes each pipeline step sequentially. Steps can be either:
    /// - Inline scripts: executed directly
    /// - External references (`uses`): loaded from files, with `with` arguments injected
    ///   as `input.<key>` variables
    async fn run_build_pipeline(
        &self,
        pipeline: &rattler_build_recipe::stage1::pipeline::ResolvedPipeline,
    ) -> Result<(), InterpreterError> {
        let span = tracing::info_span!("Running build pipeline");
        let _enter = span.enter();

        tracing::info!(
            "Executing build pipeline with {} step(s)",
            pipeline.steps.len()
        );

        for (i, step) in pipeline.steps.iter().enumerate() {
            // Determine step name: explicit name > inferred from uses path > "inline script"
            let step_name = step.name.clone().unwrap_or_else(|| {
                match &step.content {
                    rattler_build_recipe::stage1::pipeline::PipelineStepContent::External { uses, .. } => {
                        // Extract name from uses path, e.g., "./pipelines/cmake::configure" -> "cmake::configure"
                        uses.trim_start_matches("./").to_string()
                    }
                    rattler_build_recipe::stage1::pipeline::PipelineStepContent::Inline { .. } => {
                        "inline script".to_string()
                    }
                }
            });

            let step_span = tracing::info_span!(
                "Pipeline step",
                step = i + 1,
                total = pipeline.steps.len(),
                name = %step_name
            );
            let _step_enter = step_span.enter();

            tracing::info!(
                "Running pipeline step {}/{}: {}",
                i + 1,
                pipeline.steps.len(),
                step_name
            );

            // Execute the step based on its content type
            match &step.content {
                rattler_build_recipe::stage1::pipeline::PipelineStepContent::Inline { script } => {
                    tracing::info!("  executing inline script: {:?}", script.content);
                    self.run_pipeline_step_script(script, &IndexMap::new())
                        .await?;
                }
                rattler_build_recipe::stage1::pipeline::PipelineStepContent::External {
                    uses,
                    with,
                } => {
                    self.run_external_pipeline_step(uses, with).await?;
                }
            }

            tracing::info!(
                "Completed pipeline step {}/{}: {}",
                i + 1,
                pipeline.steps.len(),
                step_name
            );
        }

        tracing::info!("Build pipeline completed successfully");
        Ok(())
    }

    /// Run a pipeline step's script with optional input variables.
    async fn run_pipeline_step_script(
        &self,
        script: &Script,
        input_vars: &IndexMap<String, Variable>,
    ) -> Result<(), InterpreterError> {
        let host_prefix = self.build_configuration.directories.host_prefix.clone();
        let target_platform = self.build_configuration.target_platform;
        let mut env_vars = env_vars::vars(self, "BUILD");
        env_vars.extend(env_vars::os_vars(&host_prefix, &target_platform));
        env_vars.extend(self.env_vars_from_variant());

        let build_prefix = if self.recipe.build().merge_build_and_host_envs {
            None
        } else {
            Some(&self.build_configuration.directories.build_prefix)
        };

        // Create Jinja context with environment variables and input variables
        let mut jinja = Jinja::new(self.build_configuration.selector_config())
            .with_context(&self.recipe.context);

        // Add env vars to jinja context
        for (k, v) in &env_vars {
            if let Some(val) = v {
                jinja
                    .context_mut()
                    .insert(k.clone(), Value::from_safe_string(val.clone()));
            }
        }

        // Add input variables as an "inputs" object in the jinja context
        // This allows templates to use ${{ inputs.output-dir }} syntax
        if !input_vars.is_empty() {
            let input_map: Vec<(String, Value)> = input_vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect();
            tracing::debug!("Adding inputs to jinja context: {:?}", input_vars.keys().collect::<Vec<_>>());
            jinja
                .context_mut()
                .insert("inputs".to_string(), Value::from_iter(input_map));
        }

        let jinja_renderer = |template: &str| -> Result<String, String> {
            tracing::debug!("Jinja rendering template (first 200 chars): {}", &template[..template.len().min(200)]);
            let result = jinja.render_str(template);
            match &result {
                Ok(rendered) => tracing::debug!("Jinja rendered (first 200 chars): {}", &rendered[..rendered.len().min(200)]),
                Err(e) => tracing::error!("Jinja rendering error: {}", e),
            }
            result.map_err(|e| e.to_string())
        };

        script
            .run_script(
                env_vars,
                &self.build_configuration.directories.work_dir,
                &self.build_configuration.directories.recipe_dir,
                &host_prefix,
                build_prefix,
                Some(jinja_renderer),
                self.build_configuration.sandbox_config(),
                ScriptDebug::new(self.build_configuration.debug.is_enabled()),
            )
            .await?;

        Ok(())
    }

    /// Run an external pipeline step by loading and executing a pipeline file.
    async fn run_external_pipeline_step(
        &self,
        uses_path: &str,
        with_args: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<(), InterpreterError> {
        let recipe_dir = &self.build_configuration.directories.recipe_dir;
        self.run_pipeline_from_path(uses_path, with_args, recipe_dir)
            .await
    }

    /// Run a pipeline from a path, resolving it relative to base_dir.
    /// This method supports recursive pipeline execution.
    async fn run_pipeline_from_path(
        &self,
        uses_path: &str,
        with_args: &IndexMap<String, serde_yaml::Value>,
        base_dir: &Path,
    ) -> Result<(), InterpreterError> {
        // Resolve the pipeline file path relative to base_dir
        let pipeline_file = resolve_pipeline_path(uses_path, base_dir);

        tracing::info!(
            "Loading external pipeline: {} -> {}",
            uses_path,
            pipeline_file.display()
        );

        // Load and parse the pipeline definition file
        let pipeline_def = load_pipeline_definition(&pipeline_file).map_err(|e| {
            InterpreterError::ExecutionFailed(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Failed to load pipeline file '{}': {}",
                    pipeline_file.display(),
                    e
                ),
            ))
        })?;

        // Get the directory containing this pipeline file (for resolving nested uses)
        let pipeline_dir = pipeline_file
            .parent()
            .unwrap_or(base_dir)
            .to_path_buf();

        // Log the pipeline name from the definition file
        let pipeline_name = pipeline_def
            .name
            .as_deref()
            .unwrap_or_else(|| pipeline_file.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown"));
        tracing::info!("Executing pipeline: {}", pipeline_name);

        // Convert with_args to input Variables
        let mut input_vars: IndexMap<String, Variable> = IndexMap::new();
        for (key, value) in with_args {
            let var = serde_yaml_value_to_variable(value);
            input_vars.insert(key.clone(), var);
        }

        // Log input variables
        if !input_vars.is_empty() {
            tracing::info!("  with inputs: {:?}", input_vars.keys().collect::<Vec<_>>());
        }

        // Apply defaults for missing inputs
        for (key, input_def) in &pipeline_def.inputs {
            if !input_vars.contains_key(key.as_str()) {
                if let Some(default) = &input_def.default {
                    input_vars.insert(key.clone(), serde_yaml_value_to_variable(default));
                    tracing::debug!("  using default for input '{}': {:?}", key, default);
                } else if input_def.required {
                    return Err(InterpreterError::ExecutionFailed(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "Required input '{}' not provided for pipeline '{}'",
                            key, uses_path
                        ),
                    )));
                }
            }
        }

        // Execute the pipeline definition (use Box::pin for recursive async)
        Box::pin(self.execute_pipeline_definition(&pipeline_def, &pipeline_dir, &input_vars))
            .await
    }

    /// Execute a pipeline definition, handling both inline scripts and nested uses.
    async fn execute_pipeline_definition(
        &self,
        pipeline_def: &rattler_build_recipe::stage0::PipelineDefinition,
        pipeline_dir: &Path,
        input_vars: &IndexMap<String, Variable>,
    ) -> Result<(), InterpreterError> {
        // Check if this pipeline has a direct script (Format 1)
        if pipeline_def.script.content.is_some() || pipeline_def.script.file.is_some() {
            tracing::info!("  executing direct script (Format 1)");
            let script = evaluate_pipeline_script(pipeline_def)?;
            return self.run_pipeline_step_script(&script, input_vars).await;
        }

        // Check if this pipeline has nested steps (Format 2)
        if !pipeline_def.pipeline.is_empty() {
            tracing::info!(
                "  executing nested pipeline (Format 2) with {} steps",
                pipeline_def.pipeline.len()
            );

            for (i, step) in pipeline_def.pipeline.iter().enumerate() {
                let step_name = step
                    .name
                    .clone()
                    .or_else(|| step.uses.clone())
                    .unwrap_or_else(|| format!("step {}", i + 1));

                tracing::info!("  running nested step {}/{}: {}", i + 1, pipeline_def.pipeline.len(), step_name);

                if let Some(uses_path) = &step.uses {
                    // This step references another pipeline file
                    // First, render the `with` values using current input context
                    let rendered_with = self.render_with_args(&step.with, input_vars)?;

                    // Recursively execute the nested pipeline (Box::pin for recursive async)
                    Box::pin(self.run_pipeline_from_path(uses_path, &rendered_with, pipeline_dir))
                        .await?;
                } else if step.script.content.is_some() || step.script.file.is_some() {
                    // This step has an inline script
                    let script = evaluate_step_script(step)?;
                    self.run_pipeline_step_script(&script, input_vars).await?;
                } else {
                    tracing::warn!("  step {} has neither 'uses' nor 'script', skipping", i + 1);
                }
            }

            return Ok(());
        }

        // No content found
        tracing::warn!("Pipeline has no script or nested steps");
        Ok(())
    }

    /// Render `with` argument values using the current Jinja context.
    /// This handles templates like `${{ inputs.output_dir }}` in nested pipeline args.
    fn render_with_args(
        &self,
        with_args: &IndexMap<String, serde_yaml::Value>,
        input_vars: &IndexMap<String, Variable>,
    ) -> Result<IndexMap<String, serde_yaml::Value>, InterpreterError> {
        // Create a Jinja context for rendering
        let mut jinja = Jinja::new(self.build_configuration.selector_config())
            .with_context(&self.recipe.context);

        // Add input variables as "inputs" object
        if !input_vars.is_empty() {
            let input_map: Vec<(String, Value)> = input_vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect();
            jinja
                .context_mut()
                .insert("inputs".to_string(), Value::from_iter(input_map));
        }

        let mut rendered = IndexMap::new();
        for (key, value) in with_args {
            let rendered_value = render_yaml_value(value, &jinja)?;
            rendered.insert(key.clone(), rendered_value);
        }

        Ok(rendered)
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

/// Render a serde_yaml::Value, processing any Jinja templates in string values.
fn render_yaml_value(
    value: &serde_yaml::Value,
    jinja: &Jinja,
) -> Result<serde_yaml::Value, InterpreterError> {
    match value {
        serde_yaml::Value::String(s) => {
            // Check if the string contains Jinja template markers
            if s.contains("${{") || s.contains("{%") {
                let rendered = jinja.render_str(s).map_err(|e| {
                    InterpreterError::ExecutionFailed(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to render template '{}': {}", s, e),
                    ))
                })?;
                Ok(serde_yaml::Value::String(rendered))
            } else {
                Ok(value.clone())
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            let rendered: Result<Vec<_>, _> = seq
                .iter()
                .map(|v| render_yaml_value(v, jinja))
                .collect();
            Ok(serde_yaml::Value::Sequence(rendered?))
        }
        serde_yaml::Value::Mapping(map) => {
            let mut rendered = serde_yaml::Mapping::new();
            for (k, v) in map {
                let rendered_value = render_yaml_value(v, jinja)?;
                rendered.insert(k.clone(), rendered_value);
            }
            Ok(serde_yaml::Value::Mapping(rendered))
        }
        // Other types (null, bool, number) pass through unchanged
        _ => Ok(value.clone()),
    }
}

/// Evaluate a single pipeline definition step's script into a Script struct.
fn evaluate_step_script(
    step: &rattler_build_recipe::stage0::PipelineDefinitionStep,
) -> Result<Script, InterpreterError> {
    use rattler_build_script::ScriptContent;

    let script_source = &step.script;

    // Evaluate interpreter
    let interpreter = script_source
        .interpreter
        .as_ref()
        .and_then(|i| i.as_concrete())
        .cloned();

    // Evaluate environment variables
    let mut env = IndexMap::new();
    for (key, val) in &script_source.env {
        if let Some(v) = val.as_concrete() {
            env.insert(key.clone(), v.clone());
        }
    }

    // Evaluate cwd
    let cwd = script_source
        .cwd
        .as_ref()
        .and_then(|c| c.as_concrete())
        .map(|s| std::path::PathBuf::from(s));

    // Evaluate content
    let content = if let Some(file_val) = &script_source.file {
        if let Some(file_str) = file_val.as_concrete() {
            ScriptContent::Path(std::path::PathBuf::from(file_str))
        } else {
            ScriptContent::Default
        }
    } else if let Some(content_list) = &script_source.content {
        let commands: Vec<String> = content_list
            .iter()
            .filter_map(|item| {
                item.as_value()
                    .and_then(|v| v.as_concrete())
                    .map(|s| s.clone())
            })
            .collect();

        if commands.is_empty() {
            ScriptContent::Default
        } else if commands.len() == 1 {
            ScriptContent::CommandOrPath(commands.into_iter().next().unwrap())
        } else {
            ScriptContent::Commands(commands)
        }
    } else {
        ScriptContent::Default
    };

    Ok(Script {
        interpreter,
        env,
        secrets: script_source.secrets.clone(),
        content,
        cwd,
        content_explicit: script_source.content_explicit,
    })
}

/// Resolve a pipeline path relative to the recipe directory.
///
/// Supports two path formats:
/// - `./pipelines/cmake/configure.yaml` - direct file path
/// - `./pipelines/cmake::configure` - shorthand that converts to `./pipelines/cmake/configure.yaml`
///
/// If the path starts with "./" it's relative to recipe_dir.
/// Otherwise, it's treated as an absolute path.
fn resolve_pipeline_path(uses_path: &str, recipe_dir: &Path) -> std::path::PathBuf {
    // Handle :: syntax: ./pipelines/cmake::configure -> ./pipelines/cmake/configure
    let normalized_path = if uses_path.contains("::") {
        uses_path.replace("::", "/")
    } else {
        uses_path.to_string()
    };

    let path = if normalized_path.starts_with("./") {
        recipe_dir.join(&normalized_path[2..])
    } else {
        std::path::PathBuf::from(&normalized_path)
    };

    // Add .yaml extension if not present
    if path.extension().is_none() {
        path.with_extension("yaml")
    } else {
        path
    }
}

/// Load and parse a pipeline definition from a YAML file.
fn load_pipeline_definition(
    file_path: &Path,
) -> Result<rattler_build_recipe::stage0::PipelineDefinition, String> {
    let content = std::fs::read_to_string(file_path).map_err(|e| {
        format!(
            "Failed to read pipeline file '{}': {}",
            file_path.display(),
            e
        )
    })?;

    tracing::info!("Pipeline file content:\n{}", content);

    // Parse the YAML content using serde_yaml
    let pipeline_def: rattler_build_recipe::stage0::PipelineDefinition =
        serde_yaml::from_str(&content).map_err(|e| {
            format!(
                "Failed to parse pipeline file '{}': {}",
                file_path.display(),
                e
            )
        })?;

    // Debug: show what was parsed
    tracing::info!("Parsed pipeline definition:");
    tracing::info!("  name: {:?}", pipeline_def.name);
    tracing::info!("  script.content: {:?}", pipeline_def.script.content);
    tracing::info!("  script.file: {:?}", pipeline_def.script.file);
    tracing::info!("  script.interpreter: {:?}", pipeline_def.script.interpreter);

    Ok(pipeline_def)
}

/// Convert a serde_yaml::Value to a Variable for use in Jinja context.
fn serde_yaml_value_to_variable(value: &serde_yaml::Value) -> Variable {
    match value {
        serde_yaml::Value::Null => Variable::from(""),
        serde_yaml::Value::Bool(b) => Variable::from(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Variable::from(i)
            } else {
                // Convert f64 to string since Variable doesn't support f64 directly
                Variable::from(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => Variable::from(s.as_str()),
        serde_yaml::Value::Sequence(seq) => {
            let items: Vec<Variable> = seq.iter().map(serde_yaml_value_to_variable).collect();
            Variable::from(items)
        }
        serde_yaml::Value::Mapping(map) => {
            // Convert mapping to a minijinja Value directly
            let items: Vec<(String, Value)> = map
                .iter()
                .filter_map(|(k, v)| {
                    k.as_str()
                        .map(|ks| (ks.to_string(), serde_yaml_value_to_value(v)))
                })
                .collect();
            Variable::from(Value::from_iter(items))
        }
        serde_yaml::Value::Tagged(tagged) => serde_yaml_value_to_variable(&tagged.value),
    }
}

/// Convert a serde_yaml::Value to a minijinja Value.
fn serde_yaml_value_to_value(value: &serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::from(()),
        serde_yaml::Value::Bool(b) => Value::from(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(f) = n.as_f64() {
                Value::from(f)
            } else {
                Value::from(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => Value::from(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            Value::from_iter(seq.iter().map(serde_yaml_value_to_value))
        }
        serde_yaml::Value::Mapping(map) => {
            let items: Vec<(String, Value)> = map
                .iter()
                .filter_map(|(k, v)| {
                    k.as_str()
                        .map(|ks| (ks.to_string(), serde_yaml_value_to_value(v)))
                })
                .collect();
            Value::from_iter(items)
        }
        serde_yaml::Value::Tagged(tagged) => serde_yaml_value_to_value(&tagged.value),
    }
}

/// Evaluate a pipeline definition's script into a Script struct.
///
/// This converts the Stage0 script into a Stage1 script that can be executed.
/// Supports two formats:
/// - Format 1: Direct `script` field at top level
/// - Format 2: Nested `pipeline` with steps containing scripts
fn evaluate_pipeline_script(
    pipeline_def: &rattler_build_recipe::stage0::PipelineDefinition,
) -> Result<Script, InterpreterError> {
    use rattler_build_script::ScriptContent;

    // Determine which script source to use:
    // 1. If top-level `script` has content, use it
    // 2. Otherwise, if `pipeline` has steps, collect scripts from them
    let (script_source, collected_commands) = if pipeline_def.script.content.is_some()
        || pipeline_def.script.file.is_some()
    {
        // Format 1: Direct script
        tracing::info!("  using Format 1: direct script");
        (&pipeline_def.script, None)
    } else if !pipeline_def.pipeline.is_empty() {
        // Format 2: Collect scripts from pipeline steps
        tracing::info!("  using Format 2: nested pipeline with {} steps", pipeline_def.pipeline.len());
        let mut all_commands: Vec<String> = Vec::new();
        for (i, step) in pipeline_def.pipeline.iter().enumerate() {
            tracing::info!("    step {}: script.content = {:?}", i, step.script.content);
            if let Some(content_list) = &step.script.content {
                for item in content_list.iter() {
                    if let Some(value) = item.as_value() {
                        if let Some(s) = value.as_concrete() {
                            tracing::info!("      command: {}", s.lines().next().unwrap_or("(empty)"));
                            all_commands.push(s.clone());
                        }
                    }
                }
            }
        }
        tracing::info!("  collected {} commands from pipeline steps", all_commands.len());
        // Use first step's script as base for interpreter/env settings
        let base_script = pipeline_def.pipeline.first()
            .map(|s| &s.script)
            .unwrap_or(&pipeline_def.script);
        (base_script, Some(all_commands))
    } else {
        // No content found
        tracing::info!("  no script content found (script.content={:?}, pipeline.len={})",
            pipeline_def.script.content.is_some(), pipeline_def.pipeline.len());
        (&pipeline_def.script, None)
    };

    // Evaluate interpreter (from top-level or script source)
    let interpreter = pipeline_def.interpreter
        .as_ref()
        .or(script_source.interpreter.as_ref())
        .and_then(|i| i.as_concrete())
        .cloned();

    // Evaluate environment variables
    let mut env = IndexMap::new();
    // First from pipeline definition
    for (key, val) in &pipeline_def.env {
        if let Some(v) = val.as_concrete() {
            env.insert(key.clone(), v.clone());
        }
    }
    // Then from script source (can override)
    for (key, val) in &script_source.env {
        if let Some(v) = val.as_concrete() {
            env.insert(key.clone(), v.clone());
        }
    }

    // Evaluate cwd
    let cwd = script_source
        .cwd
        .as_ref()
        .and_then(|c| c.as_concrete())
        .map(|s| std::path::PathBuf::from(s));

    // Evaluate content
    let content = if let Some(commands) = collected_commands {
        // Format 2: Use collected commands from pipeline steps
        if commands.is_empty() {
            ScriptContent::Default
        } else if commands.len() == 1 {
            ScriptContent::CommandOrPath(commands.into_iter().next().unwrap())
        } else {
            ScriptContent::Commands(commands)
        }
    } else if let Some(file_val) = &script_source.file {
        if let Some(file_str) = file_val.as_concrete() {
            ScriptContent::Path(std::path::PathBuf::from(file_str))
        } else {
            ScriptContent::Default
        }
    } else if let Some(content_list) = &script_source.content {
        let commands: Vec<String> = content_list
            .iter()
            .filter_map(|item| {
                item.as_value()
                    .and_then(|v| v.as_concrete())
                    .map(|s| s.clone())
            })
            .collect();

        if commands.is_empty() {
            ScriptContent::Default
        } else if commands.len() == 1 {
            ScriptContent::CommandOrPath(commands.into_iter().next().unwrap())
        } else {
            ScriptContent::Commands(commands)
        }
    } else {
        ScriptContent::Default
    };

    Ok(Script {
        interpreter,
        env,
        secrets: script_source.secrets.clone(),
        content,
        cwd,
        content_explicit: script_source.content_explicit,
    })
}
