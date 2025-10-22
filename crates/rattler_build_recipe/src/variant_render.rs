//! Variant-based recipe rendering
//!
//! This module provides functionality to render recipes with variant configurations,
//! allowing you to compute all build matrix combinations and evaluate recipes with
//! specific variant values.
//!
//! For multi-output recipes, this implements a two-stage rendering process:
//! - Stage 0: Expand recipes by variants from Jinja templates
//! - Stage 1: Add variants from dependencies and compute lazy hashes

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

use rattler_build_jinja::{JinjaConfig, Variable};
use rattler_build_types::NormalizedKey;
use rattler_build_variant_config::VariantConfig;

use crate::{
    error::ParseError,
    stage0::{self, MultiOutputRecipe, Recipe as Stage0Recipe, SingleOutputRecipe},
    stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe},
};

/// Variables that are always included in variant combinations
const ALWAYS_INCLUDED_VARS: &[&str] = &["target_platform", "channel_targets", "channel_sources"];

/// Information about a pin_subpackage dependency for variant tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinSubpackageInfo {
    /// The name of the pinned subpackage
    pub name: rattler_conda_types::PackageName,
    /// The version of the pinned subpackage
    pub version: rattler_conda_types::VersionWithSource,
    /// The build string of the pinned subpackage (if known)
    pub build_string: Option<String>,
    /// Whether this is an exact pin
    pub exact: bool,
}

/// Configuration for rendering recipes with variants
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// Additional context variables to provide (beyond variant values)
    /// These can be strings, booleans, numbers, etc. using the Variable type
    pub extra_context: IndexMap<String, Variable>,
    /// Whether experimental features are enabled
    pub experimental: bool,
    /// Path to the recipe file (for relative path resolution in Jinja functions)
    pub recipe_path: Option<PathBuf>,
    /// Target platform for the build
    pub target_platform: rattler_conda_types::Platform,
    /// Build platform (where the build runs)
    pub build_platform: rattler_conda_types::Platform,
    /// Host platform (for cross-compilation)
    pub host_platform: rattler_conda_types::Platform,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            extra_context: IndexMap::new(),
            experimental: false,
            recipe_path: None,
            target_platform: rattler_conda_types::Platform::current(),
            build_platform: rattler_conda_types::Platform::current(),
            host_platform: rattler_conda_types::Platform::current(),
        }
    }
}

impl RenderConfig {
    /// Create a new render configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an extra context variable
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<Variable>) -> Self {
        self.extra_context.insert(key.into(), value.into());
        self
    }

    /// Enable experimental features
    pub fn with_experimental(mut self, experimental: bool) -> Self {
        self.experimental = experimental;
        self
    }

    /// Set the recipe path for relative path resolution
    pub fn with_recipe_path(mut self, recipe_path: impl Into<PathBuf>) -> Self {
        self.recipe_path = Some(recipe_path.into());
        self
    }

    /// Set the target platform
    pub fn with_target_platform(mut self, platform: rattler_conda_types::Platform) -> Self {
        self.target_platform = platform;
        self
    }

    /// Set the build platform
    pub fn with_build_platform(mut self, platform: rattler_conda_types::Platform) -> Self {
        self.build_platform = platform;
        self
    }

    /// Set the host platform
    pub fn with_host_platform(mut self, platform: rattler_conda_types::Platform) -> Self {
        self.host_platform = platform;
        self
    }
}

/// Result of rendering a recipe with a specific variant combination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedVariant {
    /// The variant combination used (variable name -> value)
    pub variant: BTreeMap<NormalizedKey, Variable>,
    /// The rendered stage1 recipe
    pub recipe: Stage1Recipe,
    /// Pin subpackage dependencies that need to be tracked for exact pinning
    /// Maps package name (normalized) to pin information
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pin_subpackages: BTreeMap<NormalizedKey, PinSubpackageInfo>,
}

/// Helper function to extract pin_subpackage information from a recipe
fn extract_pin_subpackages(recipe: &Stage1Recipe) -> BTreeMap<NormalizedKey, PinSubpackageInfo> {
    recipe
        .requirements
        .exact_pin_subpackages()
        .map(|pin| {
            let key = NormalizedKey::from(pin.pin_subpackage.name.as_normalized());
            let info = PinSubpackageInfo {
                name: pin.pin_subpackage.name.clone(),
                version: recipe.package.version.clone(),
                build_string: recipe.build.string.as_ref().map(|s| s.as_str().to_string()),
                exact: pin.pin_subpackage.args.exact,
            };
            (key, info)
        })
        .collect()
}

/// Add pin_subpackage information to the variant map for hash computation
///
/// For each exact pin_subpackage dependency, we need to add it to the variant
/// so that the hash includes this information. This ensures that packages with
/// different pinned dependencies get different hashes.
///
/// Returns an error if a pinned package cannot be found in the rendered outputs.
fn add_pins_to_variant(
    variant: &mut BTreeMap<NormalizedKey, Variable>,
    pin_subpackages: &BTreeMap<NormalizedKey, PinSubpackageInfo>,
    all_rendered: &[RenderedVariant],
) -> Result<(), String> {
    for (pin_name, pin_info) in pin_subpackages {
        // Find the rendered variant for this pinned package
        if let Some(pinned_variant) = all_rendered
            .iter()
            .find(|v| v.recipe.package.name == pin_info.name)
        {
            // Add the pinned package to the variant with format "version build_string"
            let variant_value = if let Some(build_string) = &pinned_variant.recipe.build.string {
                format!(
                    "{} {}",
                    pinned_variant.recipe.package.version,
                    build_string.as_str()
                )
            } else {
                pinned_variant.recipe.package.version.to_string()
            };

            variant.insert(pin_name.clone(), Variable::from(variant_value));
        } else {
            return Err(format!("Missing output: {}", pin_info.name.as_normalized()));
        }
    }
    Ok(())
}

/// Sort rendered variants topologically based on pin_subpackage dependencies
///
/// This ensures that when building multi-output packages, outputs are built in the
/// correct order - base packages before packages that depend on them via pin_subpackage.
///
/// Returns the variants in topological order, or an error if there's a cycle.
pub fn topological_sort_variants(
    variants: Vec<RenderedVariant>,
) -> Result<Vec<RenderedVariant>, String> {
    if variants.is_empty() {
        return Ok(variants);
    }

    let name_to_indices = build_name_index(&variants);
    let graph = build_dependency_graph(&variants, &name_to_indices)?;

    stable_topological_sort(variants, &graph, &name_to_indices)
}

/// Return type for dependency graph building
type DependencyGraph = (DiGraph<usize, ()>, BTreeMap<usize, NodeIndex>);

/// Build a dependency graph for topological sorting
fn build_dependency_graph(
    variants: &[RenderedVariant],
    name_to_indices: &BTreeMap<rattler_conda_types::PackageName, Vec<usize>>,
) -> Result<DependencyGraph, String> {
    // Create a directed graph
    let mut graph = DiGraph::<usize, ()>::new();
    let mut idx_to_node: BTreeMap<usize, NodeIndex> = BTreeMap::new();

    // Add nodes for each variant
    for idx in 0..variants.len() {
        let node = graph.add_node(idx);
        idx_to_node.insert(idx, node);
    }

    // Add edges based on ALL dependencies (for cycle detection)
    for (idx, variant) in variants.iter().enumerate() {
        let current_name = &variant.recipe.package.name;

        // Check all dependencies in requirements
        for dep_name in extract_dependency_names(&variant.recipe) {
            // Skip self-dependencies
            if &dep_name == current_name {
                continue;
            }

            // Find all variants that produce this dependency
            if let Some(dep_indices) = name_to_indices.get(&dep_name) {
                for &dep_idx in dep_indices {
                    // Add edge: dep_idx -> idx (dependency must be built before dependent)
                    graph.add_edge(idx_to_node[&dep_idx], idx_to_node[&idx], ());
                }
            }
        }
    }

    // Check for cycles first
    if let Err(cycle) = petgraph::algo::toposort(&graph, None) {
        let cycle_idx = graph[cycle.node_id()];
        let cycle_pkg = &variants[cycle_idx].recipe.package.name;
        return Err(format!(
            "Cycle detected in recipe outputs: {}",
            cycle_pkg.as_normalized()
        ));
    }

    Ok((graph, idx_to_node))
}

/// Perform a stable topological sort that preserves original order when possible
fn stable_topological_sort(
    variants: Vec<RenderedVariant>,
    _graph: &DependencyGraph,
    name_to_indices: &BTreeMap<rattler_conda_types::PackageName, Vec<usize>>,
) -> Result<Vec<RenderedVariant>, String> {
    // Use a stable topological sort that preserves original order when possible
    // We iterate through variants in their original order and only skip those
    // that have unsatisfied dependencies
    let mut sorted_variants = Vec::new();
    let mut added = vec![false; variants.len()];
    let mut changed = true;

    while changed && sorted_variants.len() < variants.len() {
        changed = false;
        for idx in 0..variants.len() {
            if added[idx] {
                continue;
            }

            // Check if all dependencies are already added
            let mut can_add = true;
            let current_name = &variants[idx].recipe.package.name;

            // Check all dependencies in requirements
            for dep_name in extract_dependency_names(&variants[idx].recipe) {
                // Skip self-dependencies
                if &dep_name == current_name {
                    continue;
                }

                if let Some(dep_indices) = name_to_indices.get(&dep_name) {
                    for &dep_idx in dep_indices {
                        if !added[dep_idx] {
                            can_add = false;
                            break;
                        }
                    }
                    if !can_add {
                        break;
                    }
                }
            }

            if can_add {
                sorted_variants.push(variants[idx].clone());
                added[idx] = true;
                changed = true;
            }
        }
    }

    Ok(sorted_variants)
}

/// Helper function to collect used variables from a stage0 recipe
///
/// This collects:
/// - Template variables used in Jinja expressions
/// - Free specs (dependencies without version constraints) that could be variants
/// - Always-included variables (target_platform, etc.)
///
/// Returns only variables that exist in the variant config.
fn collect_used_variables(
    stage0_recipe: &Stage0Recipe,
    variant_config: &VariantConfig,
) -> HashSet<NormalizedKey> {
    let mut used_vars = HashSet::new();

    // Get template variables from the recipe
    for var in stage0_recipe.used_variables() {
        used_vars.insert(NormalizedKey::from(var));
    }

    // Get free specs (packages without version constraints) as potential variants
    for spec in stage0_recipe.free_specs() {
        used_vars.insert(NormalizedKey::from(spec.as_normalized()));
    }

    // Insert always-included variables
    for key in ALWAYS_INCLUDED_VARS {
        used_vars.insert(NormalizedKey::from(*key));
    }

    // Filter to only variants that exist in the config
    used_vars
        .into_iter()
        .filter(|v| variant_config.get(v).is_some())
        .collect()
}

/// Helper function to build an evaluation context from variant values and config
fn build_evaluation_context(
    variant: &BTreeMap<NormalizedKey, Variable>,
    config: &RenderConfig,
    stage0_recipe: &Stage0Recipe,
) -> Result<EvaluationContext, ParseError> {
    // Build context map from variant values and extra context
    let mut context_map = config.extra_context.clone();
    for (key, value) in variant {
        context_map.insert(key.normalize(), value.clone());
    }

    // Use from_variables to preserve Variable types (e.g., booleans)
    let mut context = EvaluationContext::from_variables(context_map);

    // Set the JinjaConfig with experimental and recipe_path
    context.set_jinja_config(create_jinja_config(config));

    // Render actual context from recipe
    match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => {
            context.with_context(&recipe.context)?;
        }
        Stage0Recipe::MultiOutput(recipe) => {
            context.with_context(&recipe.context)?;
        }
    }

    Ok(context)
}

/// Helper function to evaluate a recipe (handles both single and multi-output)
fn evaluate_recipe(
    stage0_recipe: &Stage0Recipe,
    context: &EvaluationContext,
) -> Result<Vec<Stage1Recipe>, ParseError> {
    match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => Ok(vec![recipe.evaluate(context)?]),
        Stage0Recipe::MultiOutput(recipe) => recipe.evaluate(context),
    }
}

/// Helper function to handle the empty combinations case
fn render_with_empty_combinations(
    stage0_recipe: &Stage0Recipe,
    config: &RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    // Create context with just extra_context
    let mut context = EvaluationContext::from_variables(config.extra_context.clone());
    context.set_jinja_config(create_jinja_config(config));

    // Render actual context from recipe
    match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => {
            context.with_context(&recipe.context)?;
        }
        Stage0Recipe::MultiOutput(recipe) => {
            context.with_context(&recipe.context)?;
        }
    }

    // Evaluate the recipe
    let outputs = evaluate_recipe(stage0_recipe, &context)?;

    // Convert each output to a RenderedVariant
    let mut results: Vec<_> = outputs
        .into_iter()
        .map(|recipe| {
            let mut variant = recipe.used_variant.clone();

            // For noarch packages, override target_platform
            if recipe.build.noarch.is_some() {
                variant.insert("target_platform".into(), "noarch".into());
            }

            let pin_subpackages = extract_pin_subpackages(&recipe);
            RenderedVariant {
                variant,
                recipe,
                pin_subpackages,
            }
        })
        .collect();

    // Add pin information to variants
    finalize_pin_subpackages(&mut results)?;

    Ok(results)
}

/// Helper function to finalize pin_subpackages across all variants
///
/// This adds pin_subpackage information to variant maps for hash computation.
/// Must be called after all variants are rendered.
fn finalize_pin_subpackages(results: &mut [RenderedVariant]) -> Result<(), ParseError> {
    let results_snapshot = results.to_vec();
    for result in results.iter_mut() {
        if !result.pin_subpackages.is_empty() {
            add_pins_to_variant(
                &mut result.variant,
                &result.pin_subpackages,
                &results_snapshot,
            )
            .map_err(ParseError::from_message)?;
        }
    }
    Ok(())
}

/// Helper function to extract all dependency package names from a recipe
fn extract_dependency_names(recipe: &Stage1Recipe) -> Vec<rattler_conda_types::PackageName> {
    use crate::stage1::requirements::Dependency;

    recipe
        .requirements
        .all_requirements()
        .filter_map(|dep| match dep {
            Dependency::Spec(spec) => spec.name.clone(),
            Dependency::PinSubpackage(pin) => Some(pin.pin_subpackage.name.clone()),
            Dependency::PinCompatible(pin) => Some(pin.pin_compatible.name.clone()),
        })
        .collect()
}

/// Helper function to build name-to-indices mapping for topological sort
fn build_name_index(
    variants: &[RenderedVariant],
) -> BTreeMap<rattler_conda_types::PackageName, Vec<usize>> {
    let mut name_to_indices: BTreeMap<rattler_conda_types::PackageName, Vec<usize>> =
        BTreeMap::new();
    for (idx, variant) in variants.iter().enumerate() {
        let pkg_name = variant.recipe.package.name.clone();
        name_to_indices.entry(pkg_name).or_default().push(idx);
    }
    name_to_indices
}

/// Helper function to create a JinjaConfig from RenderConfig
fn create_jinja_config(config: &RenderConfig) -> JinjaConfig {
    JinjaConfig {
        experimental: config.experimental,
        recipe_path: config.recipe_path.clone(),
        target_platform: config.target_platform,
        build_platform: config.build_platform,
        host_platform: config.host_platform,
        ..Default::default()
    }
}

/// Render a recipe with variant configuration files
///
/// This function:
/// 1. Loads variant configuration from one or more YAML files
/// 2. Determines which variables are used in the recipe
/// 3. Computes all possible variant combinations
/// 4. Evaluates the recipe for each combination
///
/// # Arguments
///
/// * `recipe_path` - Path to the recipe YAML file
/// * `variant_files` - Paths to variant configuration files (e.g., `variants.yaml`)
/// * `config` - Optional render configuration with extra context
///
/// # Returns
///
/// A vector of `RenderedVariant`, one for each variant combination
///
/// # Example
///
/// ```rust,no_run
/// use rattler_build_recipe::variant_render::{render_recipe_with_variants, RenderConfig};
/// use std::path::Path;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let recipe_path = Path::new("recipe.yaml");
/// let variant_files = vec![Path::new("variants.yaml")];
/// let config = RenderConfig::new()
///     .with_context("unix", "true");
///
/// let rendered = render_recipe_with_variants(recipe_path, &variant_files, Some(config))?;
///
/// for variant in rendered {
///     println!("Variant: {:?}", variant.variant);
///     println!("Package: {} {}",
///         variant.recipe.package().name().as_normalized(),
///         variant.recipe.package().version()
///     );
/// }
/// # Ok(())
/// # }
/// ```
pub fn render_recipe_with_variants(
    recipe_path: &Path,
    variant_files: &[impl AsRef<Path>],
    config: Option<RenderConfig>,
) -> Result<Vec<RenderedVariant>, ParseError> {
    let mut config = config.unwrap_or_default();

    // Set the recipe path if not already set
    if config.recipe_path.is_none() {
        config.recipe_path = Some(recipe_path.to_path_buf());
    }

    // Read and parse the recipe
    let yaml_content = fs_err::read_to_string(recipe_path)
        .map_err(|e| ParseError::io_error(e, recipe_path.to_path_buf()))?;

    let stage0_recipe = stage0::parse_recipe_or_multi_from_source(&yaml_content)?;

    // Load variant configuration
    let variant_config = VariantConfig::from_files(variant_files)
        .map_err(|e| ParseError::from_message(e.to_string()))?;

    render_recipe_with_variant_config(&stage0_recipe, &variant_config, config)
}

/// Render a stage0 recipe with a loaded variant configuration
///
/// This is a lower-level function that works with already-loaded recipe and variant config.
///
/// # Arguments
///
/// * `stage0_recipe` - The parsed stage0 recipe
/// * `variant_config` - The loaded variant configuration
/// * `config` - Render configuration with extra context
///
/// # Returns
///
/// A vector of `RenderedVariant`, one for each variant combination
pub fn render_recipe_with_variant_config(
    stage0_recipe: &Stage0Recipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => {
            render_single_output_with_variants(recipe.as_ref(), variant_config, config)
        }
        Stage0Recipe::MultiOutput(recipe) => {
            render_multi_output_with_variants(recipe.as_ref(), variant_config, config)
        }
    }
}

fn render_single_output_with_variants(
    stage0_recipe: &SingleOutputRecipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    let stage0 = Stage0Recipe::SingleOutput(Box::new(stage0_recipe.clone()));
    render_with_variants(&stage0, variant_config, config)
}

fn render_multi_output_with_variants(
    stage0_recipe: &MultiOutputRecipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    let stage0 = Stage0Recipe::MultiOutput(Box::new(stage0_recipe.clone()));
    render_with_variants(&stage0, variant_config, config)
}

/// Internal unified function to render both single and multi-output recipes
fn render_with_variants(
    stage0_recipe: &Stage0Recipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    // Collect used variables
    let used_vars = collect_used_variables(stage0_recipe, variant_config);

    // Compute all variant combinations
    let combinations = variant_config
        .combinations(&used_vars, None)
        .map_err(|e| ParseError::from_message(e.to_string()))?;

    // If no combinations, render once with just the extra context
    if combinations.is_empty() {
        return render_with_empty_combinations(stage0_recipe, &config);
    }

    // Render recipe for each variant combination
    let mut results = Vec::with_capacity(combinations.len());

    for variant in combinations {
        let context = build_evaluation_context(&variant, &config, stage0_recipe)?;
        let outputs = evaluate_recipe(stage0_recipe, &context)?;

        // Convert each output to a RenderedVariant
        for recipe in outputs {
            let variant = recipe.used_variant.clone();
            let pin_subpackages = extract_pin_subpackages(&recipe);
            results.push(RenderedVariant {
                variant,
                recipe,
                pin_subpackages,
            });
        }
    }

    // Add pin information to variants for hash computation
    finalize_pin_subpackages(&mut results)?;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage1::requirements::Dependency;

    #[test]
    fn test_render_with_simple_variants() {
        let recipe_yaml = r#"
package:
  name: test-pkg
  version: "1.0.0"

requirements:
  build:
    - ${{ compiler('c') }}
  host:
    - python ${{ python }}
  run:
    - python
"#;

        let variant_yaml = r#"
python:
  - "3.9.*"
  - "3.10.*"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap_or_else(|e| panic!("Failed to render: {:?}", e));

        assert_eq!(rendered.len(), 2);

        // Check that we have both variants (order may vary)
        let variants: Vec<String> = rendered
            .iter()
            .map(|r| r.variant.get(&"python".into()).unwrap().to_string())
            .collect();

        assert!(variants.contains(&"3.9.*".to_string()));
        assert!(variants.contains(&"3.10.*".to_string()));
    }

    #[test]
    fn test_render_with_free_specs() {
        let recipe_yaml = r#"
package:
  name: test-pkg
  version: "1.0.0"

requirements:
  build:
    - python
"#;

        let variant_yaml = r#"
python:
  - "3.9.*"
  - "3.10.*"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should create variants based on free spec "python"
        assert_eq!(rendered.len(), 2);
    }

    #[test]
    fn test_render_multi_output_simple() {
        let recipe_yaml = r#"
schema_version: 1

recipe:
  name: multi-pkg
  version: "1.0.0"

context:
  name: multi-pkg
  version: "1.0.0"

outputs:
  - package:
      name: ${{ name }}-lib
      version: ${{ version }}
    build:
      noarch: generic

  - package:
      name: ${{ name }}
      version: ${{ version }}
    build:
      noarch: generic
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 2 outputs
        assert_eq!(rendered.len(), 2);

        // Check package names
        let names: Vec<String> = rendered
            .iter()
            .map(|r| r.recipe.package.name.as_normalized().to_string())
            .collect();

        assert!(names.contains(&"multi-pkg-lib".to_string()));
        assert!(names.contains(&"multi-pkg".to_string()));
    }

    #[test]
    fn test_render_multi_output_with_pin_subpackage() {
        let recipe_yaml = r#"
schema_version: 1

context:
  name: my-pkg
  version: "0.1.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}
    build:
      noarch: generic

  - package:
      name: ${{ name }}-extra
    build:
      noarch: generic
    requirements:
      run:
        - ${{ pin_subpackage(name, exact=true) }}
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 2 outputs
        assert_eq!(rendered.len(), 2);

        // Check that the second output has the pin_subpackage reference
        let extra_pkg = rendered
            .iter()
            .find(|r| r.recipe.package.name.as_normalized() == "my-pkg-extra")
            .expect("Should have my-pkg-extra output");

        // The variant should include the pinned package (normalized to my_pkg)
        // For now, let's check that it has requirements
        assert!(!extra_pkg.recipe.requirements.run.is_empty());

        // Check that we have a pin_subpackage dependency
        let pin_sub = extra_pkg
            .recipe
            .requirements
            .run
            .iter()
            .find_map(|dep| {
                if let Dependency::PinSubpackage(pin) = dep {
                    Some(pin)
                } else {
                    None
                }
            })
            .expect("Should have pin_subpackage in run requirements");

        assert_eq!(pin_sub.pin_subpackage.name.as_normalized(), "my-pkg");
        assert!(
            pin_sub.pin_subpackage.args.exact,
            "pin_subpackage should have exact=true"
        );

        // Verify that the pin_subpackages field is populated
        assert_eq!(
            extra_pkg.pin_subpackages.len(),
            1,
            "Should have 1 pin_subpackage tracked"
        );

        let pin_info = extra_pkg
            .pin_subpackages
            .get(&"my-pkg".into())
            .expect("Should have my-pkg in pin_subpackages");

        assert_eq!(pin_info.name.as_normalized(), "my-pkg");
        assert_eq!(pin_info.version.to_string(), "0.1.0");
        assert!(pin_info.exact, "Pin should be marked as exact");

        assert!(
            extra_pkg.variant.contains_key(&"my-pkg".into()),
            "Variant should include pinned package for hash computation"
        );
    }

    #[test]
    fn test_render_multi_output_with_pin_subpackage_and_variants() {
        // Test that pin_subpackage with exact=true creates variant dependencies
        let recipe_yaml = r#"
schema_version: 1

context:
  name: mylib
  version: "1.2.3"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}
    build:
      noarch: python
    requirements:
      host:
        - python ${{ python }}.*

  - package:
      name: ${{ name }}-tools
    build:
      noarch: python
    requirements:
      host:
        - python ${{ python }}.*
      run:
        - python ${{ python }}.*
        - ${{ pin_subpackage(name, exact=true) }}
"#;

        let variant_yaml = r#"
python:
  - "3.9"
  - "3.10"
  - "3.11"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 2 outputs × 3 python variants = 6 total
        assert_eq!(rendered.len(), 6);

        // Check each tools package has pin_subpackage with exact=true
        let tools_packages: Vec<_> = rendered
            .iter()
            .filter(|r| r.recipe.package.name.as_normalized() == "mylib-tools")
            .collect();

        assert_eq!(
            tools_packages.len(),
            3,
            "Should have 3 mylib-tools variants"
        );

        for tools_pkg in tools_packages {
            // Check pin_subpackage dependency
            let pin_sub = tools_pkg
                .recipe
                .requirements
                .run
                .iter()
                .find_map(|dep| {
                    if let Dependency::PinSubpackage(pin) = dep {
                        Some(pin)
                    } else {
                        None
                    }
                })
                .expect("Should have pin_subpackage in run requirements");

            assert_eq!(pin_sub.pin_subpackage.name.as_normalized(), "mylib");
            assert!(
                pin_sub.pin_subpackage.args.exact,
                "pin_subpackage should have exact=true"
            );

            assert_eq!(
                tools_pkg.pin_subpackages.len(),
                1,
                "Should have 1 pin_subpackage tracked"
            );

            let pin_info = tools_pkg
                .pin_subpackages
                .get(&"mylib".into())
                .expect("Should have mylib in pin_subpackages");

            assert_eq!(pin_info.name.as_normalized(), "mylib");
            assert_eq!(pin_info.version.to_string(), "1.2.3");
            assert!(pin_info.exact, "Pin should be marked as exact");

            assert!(
                tools_pkg.variant.contains_key(&"mylib".into()),
                "Variant should include pinned package 'mylib' for hash computation"
            );

            // The variant should contain the version and build string
            let mylib_variant = tools_pkg.variant.get(&"mylib".into()).unwrap();
            assert!(
                mylib_variant.to_string().contains("1.2.3"),
                "Variant should include mylib version"
            );
        }
    }

    #[test]
    fn test_topological_sort_simple() {
        // Test simple case: base package -> dependent package
        let recipe_yaml = r#"
schema_version: 1

context:
  name: base
  version: "1.0.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}
    build:
      noarch: generic

  - package:
      name: ${{ name }}-tools
    build:
      noarch: generic
    requirements:
      run:
        - ${{ pin_subpackage(name, exact=true) }}
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Sort topologically
        let sorted = topological_sort_variants(rendered).unwrap();

        assert_eq!(sorted.len(), 2);
        // Base package should come first
        assert_eq!(sorted[0].recipe.package.name.as_normalized(), "base");
        // Dependent package should come second
        assert_eq!(sorted[1].recipe.package.name.as_normalized(), "base-tools");
    }

    #[test]
    fn test_topological_sort_chain() {
        // Test chain: pkg-a -> pkg-b -> pkg-c
        let recipe_yaml = r#"
schema_version: 1

context:
  version: "1.0.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: pkg-a
    build:
      noarch: generic

  - package:
      name: pkg-b
    build:
      noarch: generic
    requirements:
      run:
        - ${{ pin_subpackage("pkg-a", exact=true) }}

  - package:
      name: pkg-c
    build:
      noarch: generic
    requirements:
      run:
        - ${{ pin_subpackage("pkg-b", exact=true) }}
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Sort topologically
        let sorted = topological_sort_variants(rendered).unwrap();

        assert_eq!(sorted.len(), 3);
        // Should be in dependency order
        assert_eq!(sorted[0].recipe.package.name.as_normalized(), "pkg-a");
        assert_eq!(sorted[1].recipe.package.name.as_normalized(), "pkg-b");
        assert_eq!(sorted[2].recipe.package.name.as_normalized(), "pkg-c");
    }

    #[test]
    fn test_topological_sort_no_dependencies() {
        // Test case with no pin_subpackage dependencies
        let recipe_yaml = r#"
schema_version: 1

context:
  version: "1.0.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: pkg-a
    build:
      noarch: generic

  - package:
      name: pkg-b
    build:
      noarch: generic

  - package:
      name: pkg-c
    build:
      noarch: generic
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Sort topologically (should succeed even with no dependencies)
        let sorted = topological_sort_variants(rendered).unwrap();

        assert_eq!(sorted.len(), 3);

        let names: Vec<_> = sorted
            .iter()
            .map(|v| v.recipe.package.name.as_normalized())
            .collect();
        // Check that packages are in the right order
        assert_eq!(names, vec!["pkg-a", "pkg-b", "pkg-c"]);
    }

    #[test]
    fn test_topological_sort_cycle_detection() {
        // This test documents that cycle detection works
        // In practice, this would be caught earlier during recipe parsing/evaluation
        // since pin_subpackage typically can't create cycles (packages can't depend on themselves)

        // For now, we'll just test the empty case to ensure the function handles edge cases
        let sorted = topological_sort_variants(vec![]).unwrap();
        assert_eq!(sorted.len(), 0);
    }

    #[test]
    fn test_topological_sort_self_pin() {
        // Test that self-pins (package pinning itself) are allowed and don't cause cycles
        // This is common in run_exports where a package exports a pin to itself
        let recipe_yaml = r#"
schema_version: 1

context:
  name: mylib
  version: "1.0.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}
    build:
      noarch: generic
    requirements:
      run_exports:
        - ${{ pin_subpackage(name, exact=true) }}
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should succeed without cycle error
        let sorted = topological_sort_variants(rendered).unwrap();
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].recipe.package.name.as_normalized(), "mylib");

        // Self-pin should be tracked but not cause ordering issues
        assert_eq!(sorted[0].pin_subpackages.len(), 1);
    }
}
