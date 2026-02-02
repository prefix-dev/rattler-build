//! Stage 1 Pipeline - evaluated pipeline with all templates resolved
//!
//! This module contains the resolved pipeline structures where all Jinja templates
//! have been evaluated and all conditionals have been flattened.
//!
//! Pipeline steps can be either:
//! - Inline scripts (fully resolved)
//! - External references via `uses` (resolved at build time when file access is available)

use indexmap::IndexMap;
use rattler_build_script::Script;
use serde::{Deserialize, Serialize};

/// Content of a pipeline step - either an inline script or an external reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PipelineStepContent {
    /// Inline script that was evaluated from the recipe
    Inline {
        /// The script to execute
        script: Script,
    },
    /// External pipeline reference that needs resolution at build time.
    /// The `uses` path points to a pipeline definition file.
    External {
        /// Path to the external pipeline file (e.g., "./pipelines/cmake/configure.yaml")
        uses: String,
        /// Arguments to pass to the pipeline, exposed as `input.<key>` in Jinja
        #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
        with: IndexMap<String, serde_yaml::Value>,
    },
}

impl Default for PipelineStepContent {
    fn default() -> Self {
        Self::Inline {
            script: Script::default(),
        }
    }
}

impl PipelineStepContent {
    /// Create inline content with a script
    pub fn inline(script: Script) -> Self {
        Self::Inline { script }
    }

    /// Create external content with a uses path
    pub fn external(uses: impl Into<String>) -> Self {
        Self::External {
            uses: uses.into(),
            with: IndexMap::new(),
        }
    }

    /// Create external content with uses path and with arguments
    pub fn external_with_args(
        uses: impl Into<String>,
        with: IndexMap<String, serde_yaml::Value>,
    ) -> Self {
        Self::External {
            uses: uses.into(),
            with,
        }
    }

    /// Check if this is an inline script
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }

    /// Check if this is an external reference
    pub fn is_external(&self) -> bool {
        matches!(self, Self::External { .. })
    }

    /// Get the inline script if this is an inline content
    pub fn as_inline(&self) -> Option<&Script> {
        match self {
            Self::Inline { script } => Some(script),
            Self::External { .. } => None,
        }
    }

    /// Get the external reference if this is an external content
    pub fn as_external(&self) -> Option<(&str, &IndexMap<String, serde_yaml::Value>)> {
        match self {
            Self::External { uses, with } => Some((uses, with)),
            Self::Inline { .. } => None,
        }
    }
}

/// A resolved pipeline step ready for execution.
///
/// The step can contain either an inline script or an external reference.
/// External references are resolved at build time when file access is available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPipelineStep {
    /// Name of this step (for logging and debugging)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The content of this step (inline script or external reference)
    #[serde(flatten)]
    pub content: PipelineStepContent,

    /// Additional environment variables for this step
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env: IndexMap<String, String>,

    /// CPU cost hint for parallel scheduling (defaults to 1)
    #[serde(default = "default_cpu_cost")]
    pub cpu_cost: u32,

    /// Output paths produced by this step (for future caching support)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
}

fn default_cpu_cost() -> u32 {
    1
}

impl Default for ResolvedPipelineStep {
    fn default() -> Self {
        Self {
            name: None,
            content: PipelineStepContent::default(),
            env: IndexMap::new(),
            cpu_cost: 1,
            outputs: Vec::new(),
        }
    }
}

impl ResolvedPipelineStep {
    /// Create a new pipeline step with inline script content
    pub fn with_script(script: Script) -> Self {
        Self {
            content: PipelineStepContent::inline(script),
            ..Default::default()
        }
    }

    /// Create a new pipeline step with an external uses reference
    pub fn with_uses(uses: impl Into<String>) -> Self {
        Self {
            content: PipelineStepContent::external(uses),
            ..Default::default()
        }
    }

    /// Create a new pipeline step with an external uses reference and arguments
    pub fn with_uses_and_args(
        uses: impl Into<String>,
        with: IndexMap<String, serde_yaml::Value>,
    ) -> Self {
        Self {
            content: PipelineStepContent::external_with_args(uses, with),
            ..Default::default()
        }
    }

    /// Create a new pipeline step with a name and inline script
    pub fn with_name_and_script(name: impl Into<String>, script: Script) -> Self {
        Self {
            name: Some(name.into()),
            content: PipelineStepContent::inline(script),
            ..Default::default()
        }
    }

    /// Check if this step uses an external pipeline reference
    pub fn is_external(&self) -> bool {
        self.content.is_external()
    }

    /// Check if this step has inline script content
    pub fn is_inline(&self) -> bool {
        self.content.is_inline()
    }

    /// Get the script if this is an inline step
    pub fn script(&self) -> Option<&Script> {
        self.content.as_inline()
    }

    /// Get the uses path and with arguments if this is an external step
    pub fn uses(&self) -> Option<(&str, &IndexMap<String, serde_yaml::Value>)> {
        self.content.as_external()
    }
}

/// A complete evaluated pipeline ready for execution.
///
/// Contains a list of steps that will be executed sequentially.
/// External references in steps are resolved at build time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPipeline {
    /// List of steps to execute sequentially
    #[serde(default)]
    pub steps: Vec<ResolvedPipelineStep>,
}

impl ResolvedPipeline {
    /// Create a new empty pipeline
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a pipeline with the given steps
    pub fn with_steps(steps: Vec<ResolvedPipelineStep>) -> Self {
        Self { steps }
    }

    /// Check if the pipeline is empty (no steps)
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Get the number of steps in the pipeline
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Iterate over the pipeline steps
    pub fn iter(&self) -> impl Iterator<Item = &ResolvedPipelineStep> {
        self.steps.iter()
    }

    /// Check if any steps have external references that need resolution
    pub fn has_external_references(&self) -> bool {
        self.steps.iter().any(|s| s.is_external())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rattler_build_script::ScriptContent;

    #[test]
    fn test_resolved_pipeline_step_default() {
        let step = ResolvedPipelineStep::default();
        assert!(step.name.is_none());
        assert!(step.is_inline());
        assert!(step.env.is_empty());
        assert_eq!(step.cpu_cost, 1);
        assert!(step.outputs.is_empty());
    }

    #[test]
    fn test_resolved_pipeline_step_with_script() {
        let script = Script {
            content: ScriptContent::Commands(vec!["echo hello".to_string()]),
            ..Default::default()
        };
        let step = ResolvedPipelineStep::with_script(script.clone());
        assert!(step.name.is_none());
        assert!(step.is_inline());
        assert_eq!(step.script(), Some(&script));
    }

    #[test]
    fn test_resolved_pipeline_step_with_uses() {
        let step = ResolvedPipelineStep::with_uses("./pipelines/cmake/configure.yaml");
        assert!(step.is_external());
        let (uses, with) = step.uses().unwrap();
        assert_eq!(uses, "./pipelines/cmake/configure.yaml");
        assert!(with.is_empty());
    }

    #[test]
    fn test_resolved_pipeline_step_with_uses_and_args() {
        let mut with = IndexMap::new();
        with.insert(
            "cmake_args".to_string(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                "-DCMAKE_BUILD_TYPE=Release".to_string(),
            )]),
        );
        let step = ResolvedPipelineStep::with_uses_and_args("./pipelines/cmake/configure.yaml", with.clone());
        assert!(step.is_external());
        let (uses, args) = step.uses().unwrap();
        assert_eq!(uses, "./pipelines/cmake/configure.yaml");
        assert_eq!(args, &with);
    }

    #[test]
    fn test_resolved_pipeline_step_with_name_and_script() {
        let script = Script {
            content: ScriptContent::Commands(vec!["make install".to_string()]),
            ..Default::default()
        };
        let step = ResolvedPipelineStep::with_name_and_script("Install", script.clone());
        assert_eq!(step.name, Some("Install".to_string()));
        assert!(step.is_inline());
        assert_eq!(step.script(), Some(&script));
    }

    #[test]
    fn test_resolved_pipeline_empty() {
        let pipeline = ResolvedPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);
        assert!(!pipeline.has_external_references());
    }

    #[test]
    fn test_resolved_pipeline_with_steps() {
        let steps = vec![
            ResolvedPipelineStep::default(),
            ResolvedPipelineStep::with_uses("./pipelines/test.yaml"),
        ];
        let pipeline = ResolvedPipeline::with_steps(steps);
        assert!(!pipeline.is_empty());
        assert_eq!(pipeline.len(), 2);
        assert!(pipeline.has_external_references());
    }
}
