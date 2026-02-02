//! Stage0 Pipeline types - templates and conditionals before evaluation
//!
//! This module defines the pipeline structures used as an alternative to the
//! traditional script-based builds. Pipelines allow for multi-step builds with
//! reusable pipeline definitions loaded from external files.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};

use crate::stage0::types::{ConditionalList, Script, Value};

/// A single step in a build pipeline.
///
/// Each step can either reference an external pipeline definition via `uses`,
/// or define the script inline. These are mutually exclusive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineStep {
    /// Reference to an external pipeline file (e.g., "./pipelines/cmake/configure.yaml")
    /// Mutually exclusive with inline `script`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses: Option<Value<String>>,

    /// Arguments to pass to the pipeline when using `uses`.
    /// These are exposed as `input.<var-name>` in Jinja templates within the pipeline.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub with: IndexMap<String, serde_yaml::Value>,

    /// Inline script definition (mutually exclusive with `uses`).
    /// When specified directly, the step runs this script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,

    /// Optional name/description for this step (for logging and debugging).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Value<String>>,
}

impl Default for PipelineStep {
    fn default() -> Self {
        Self {
            uses: None,
            with: IndexMap::new(),
            script: None,
            name: None,
        }
    }
}

impl Display for PipelineStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(uses) = &self.uses {
            write!(f, "PipelineStep(uses: {})", uses)
        } else if let Some(script) = &self.script {
            write!(f, "PipelineStep(script: {})", script)
        } else {
            write!(f, "PipelineStep(empty)")
        }
    }
}

impl PipelineStep {
    /// Check if this step has neither `uses` nor `script` defined.
    pub fn is_empty(&self) -> bool {
        self.uses.is_none() && self.script.is_none()
    }

    /// Collect all variables used in this pipeline step.
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        if let Some(uses) = &self.uses {
            vars.extend(uses.used_variables());
        }

        // Note: `with` values are serde_yaml::Value, we don't extract variables from them
        // as they are passed through to the pipeline definition

        if let Some(script) = &self.script {
            vars.extend(script.used_variables());
        }

        if let Some(name) = &self.name {
            vars.extend(name.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }
}

/// A complete pipeline definition loaded from an external file.
///
/// Pipeline files define reusable build steps with configurable inputs.
/// Supports two formats:
///
/// Format 1 - Direct script (simple):
/// ```yaml
/// name: Configure CMake
/// script:
///   - cmake -S . -B build ${{ input.cmake_args | join(" ") }}
/// inputs:
///   cmake_args:
///     description: Additional CMake arguments
///     default: []
/// ```
///
/// Format 2 - Nested pipeline steps:
/// ```yaml
/// name: Build CMake project
/// inputs:
///   output-dir:
///     default: build
/// pipeline:
///   - script: |
///       cmake --build ${{ input.output-dir }}
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PipelineDefinition {
    /// Human-readable name for this pipeline step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The script to execute for this pipeline step (Format 1).
    /// Mutually exclusive with `pipeline`.
    #[serde(default)]
    pub script: Script,

    /// Nested pipeline steps (Format 2).
    /// If present, scripts from these steps are concatenated and executed.
    /// Mutually exclusive with direct `script`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pipeline: Vec<PipelineDefinitionStep>,

    /// Input parameters that can be passed via `with` when using this pipeline.
    /// Each input can have a description and default value.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub inputs: IndexMap<String, PipelineInput>,

    /// Output paths produced by this pipeline step (for future caching support).
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub outputs: ConditionalList<String>,

    /// CPU cost hint for parallel scheduling (defaults to 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_cost: Option<Value<String>>,

    /// Override the interpreter for the script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Value<String>>,

    /// Additional environment variables for this pipeline step.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env: IndexMap<String, Value<String>>,
}

/// A step within a pipeline definition file (for Format 2).
///
/// This is a simplified step structure used within pipeline definition files.
/// Supports both inline scripts and nested pipeline references via `uses`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PipelineDefinitionStep {
    /// Reference to another pipeline file (e.g., "./configure.yaml").
    /// Mutually exclusive with inline `script`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses: Option<String>,

    /// Arguments to pass to the nested pipeline when using `uses`.
    /// These are exposed as `inputs.<var-name>` in the nested pipeline.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub with: IndexMap<String, serde_yaml::Value>,

    /// The script content for this step (mutually exclusive with `uses`).
    #[serde(default)]
    pub script: Script,

    /// Optional name for this step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Display for PipelineDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "PipelineDefinition({})", name)
        } else {
            write!(f, "PipelineDefinition(unnamed)")
        }
    }
}

impl PipelineDefinition {
    /// Collect all variables used in this pipeline definition.
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        vars.extend(self.script.used_variables());

        for input in self.inputs.values() {
            vars.extend(input.used_variables());
        }

        vars.extend(self.outputs.used_variables());

        if let Some(cpu_cost) = &self.cpu_cost {
            vars.extend(cpu_cost.used_variables());
        }

        if let Some(interpreter) = &self.interpreter {
            vars.extend(interpreter.used_variables());
        }

        for value in self.env.values() {
            vars.extend(value.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }
}

/// Input parameter definition for a pipeline.
///
/// Defines metadata about an input parameter that can be passed to a pipeline
/// via the `with` field when referencing it with `uses`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PipelineInput {
    /// Human-readable description of this input parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default value if the input is not provided.
    /// Can be any YAML value (string, list, mapping, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_yaml::Value>,

    /// Whether this input is required (defaults to false).
    /// If true and no default is provided, an error is raised when the input is missing.
    #[serde(default)]
    pub required: bool,
}

impl PipelineInput {
    /// Collect all variables used in this input definition.
    pub fn used_variables(&self) -> Vec<String> {
        // Default values are serde_yaml::Value, we don't extract variables from them
        Vec::new()
    }
}

/// A build pipeline - a list of pipeline steps with conditional support.
pub type Pipeline = ConditionalList<PipelineStep>;
