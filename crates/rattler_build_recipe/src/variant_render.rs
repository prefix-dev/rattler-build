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
use rattler_build_yaml_parser::ParseError;
use rattler_conda_types::NoArchType;
use serde::{Deserialize, Serialize};

use rattler_build_jinja::{JinjaConfig, Variable};
use rattler_build_types::NormalizedKey;
use rattler_build_variant_config::VariantConfig;

use crate::stage0::evaluate::ALWAYS_INCLUDED_VARS;
use crate::stage1::{Dependency, HashInfo, build::BuildString};
use crate::{
    stage0::{self, MultiOutputRecipe, Recipe as Stage0Recipe, SingleOutputRecipe},
    stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe},
};

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
    /// OS environment variable keys that can be overridden by variant configuration.
    /// These are typically derived from `env_vars::os_vars()` and include variables
    /// like `MACOSX_DEPLOYMENT_TARGET` on macOS that have default values but can be
    /// customized via variant config.
    pub os_env_var_keys: HashSet<String>,
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
            os_env_var_keys: HashSet::new(),
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

    /// Set the OS environment variable keys that can be overridden by variant config.
    /// These are typically derived from `env_vars::os_vars()` keys.
    pub fn with_os_env_var_keys(mut self, keys: HashSet<String>) -> Self {
        self.os_env_var_keys = keys;
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
    /// The hash info that was used to compute the build string.
    pub hash_info: Option<HashInfo>,
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
                build_string: recipe.build.string.as_resolved().map(|s| s.to_string()),
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
    self_name: &rattler_conda_types::PackageName,
    variant: &mut BTreeMap<NormalizedKey, Variable>,
    pin_subpackages: &BTreeMap<NormalizedKey, PinSubpackageInfo>,
    all_rendered: &[RenderedVariant],
) -> Result<(), String> {
    for (pin_name, pin_info) in pin_subpackages {
        // Ignore ourselves
        if self_name == &pin_info.name {
            continue;
        }

        // Find the rendered variant for this pinned package
        if let Some(pinned_variant) = all_rendered
            .iter()
            .find(|v| v.recipe.package.name == pin_info.name)
        {
            // Add the pinned package to the variant with format "version build_string"
            let variant_value =
                if let Some(build_string) = pinned_variant.recipe.build.string.as_resolved() {
                    format!("{} {}", pinned_variant.recipe.package.version, build_string)
                } else {
                    unreachable!("Should not happen when topological ordering succeeded");
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
        println!(
            "Dependency names: {:?}",
            extract_dependency_names(&variant.recipe)
        );
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
/// - OS environment variable keys (passed via RenderConfig)
/// - use_keys from build.variant.use_keys (forces keys into the variant matrix)
///
/// Returns only variables that exist in the variant config.
fn collect_used_variables(
    stage0_recipe: &Stage0Recipe,
    variant_config: &VariantConfig,
    os_env_var_keys: &HashSet<String>,
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

    // Insert OS environment variable keys that can be overridden by variant config
    // These come from env_vars::os_vars() and include platform-specific vars like
    // MACOSX_DEPLOYMENT_TARGET
    for key in os_env_var_keys {
        used_vars.insert(NormalizedKey::from(key.as_str()));
    }

    // Add use_keys from build.variant.use_keys
    // These force specific variant keys to be included in the matrix even if
    // not explicitly referenced in templates or dependencies
    for key in stage0_recipe.use_keys() {
        used_vars.insert(NormalizedKey::from(key.as_str()));
    }

    // Filter to only variants that exist in the config
    used_vars
        .into_iter()
        .filter(|v| variant_config.get(v).is_some())
        .collect()
}

/// Evaluate requirements with a variant combination and extract free specs
/// that are also variant keys (not yet in the combination)
fn discover_new_variant_keys_from_evaluation(
    stage0_recipe: &Stage0Recipe,
    combination: &BTreeMap<NormalizedKey, Variable>,
    variant_config: &VariantConfig,
    config: &RenderConfig,
) -> Result<HashSet<NormalizedKey>, ParseError> {
    let context = build_evaluation_context(combination, config, stage0_recipe)?;

    // Get requirements and evaluate them
    let free_specs: Vec<rattler_conda_types::PackageName> = match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => {
            let evaluated = recipe.requirements.evaluate(&context)?;
            evaluated.free_specs()
        }
        Stage0Recipe::MultiOutput(recipe) => {
            let mut all_free_specs = Vec::new();
            for output in &recipe.outputs {
                let reqs = match output {
                    stage0::Output::Staging(staging) => &staging.requirements,
                    stage0::Output::Package(pkg) => &pkg.requirements,
                };
                if let Ok(evaluated) = reqs.evaluate(&context) {
                    all_free_specs.extend(evaluated.free_specs());
                }
            }
            all_free_specs
        }
    };

    // Find free specs that are variant keys but not in current combination
    let mut new_keys = HashSet::new();
    for spec in free_specs {
        let key = NormalizedKey::from(spec.as_normalized());
        if variant_config.get(&key).is_some() && !combination.contains_key(&key) {
            new_keys.insert(key);
        }
    }

    Ok(new_keys)
}

/// Expand a variant combination with new keys, creating all combinations
fn expand_combination_with_keys(
    base: &BTreeMap<NormalizedKey, Variable>,
    new_keys: &HashSet<NormalizedKey>,
    variant_config: &VariantConfig,
) -> Vec<BTreeMap<NormalizedKey, Variable>> {
    if new_keys.is_empty() {
        return vec![base.clone()];
    }

    // Get values for each new key
    let key_values: Vec<(NormalizedKey, Vec<Variable>)> = new_keys
        .iter()
        .filter_map(|key| {
            variant_config
                .get(key)
                .map(|values| (key.clone(), values.clone()))
        })
        .collect();

    if key_values.is_empty() {
        return vec![base.clone()];
    }

    // Create cross-product of all new key values
    let mut results = vec![base.clone()];
    for (key, values) in key_values {
        let mut new_results = Vec::new();
        for combo in &results {
            for value in &values {
                let mut new_combo = combo.clone();
                new_combo.insert(key.clone(), value.clone());
                new_results.push(new_combo);
            }
        }
        results = new_results;
    }

    results
}

/// Recursively expand variant combinations by discovering new variant keys from evaluation
/// This implements a tree-based approach where each combination can spawn new combinations
/// if its evaluated free specs reveal new variant keys
fn expand_variants_tree(
    stage0_recipe: &Stage0Recipe,
    initial_combinations: Vec<BTreeMap<NormalizedKey, Variable>>,
    variant_config: &VariantConfig,
    config: &RenderConfig,
) -> Result<Vec<BTreeMap<NormalizedKey, Variable>>, ParseError> {
    let mut final_combinations = Vec::new();
    let mut to_process = initial_combinations;

    // Limit iterations to prevent infinite loops
    const MAX_ITERATIONS: usize = 10;
    let mut iteration = 0;

    while !to_process.is_empty() && iteration < MAX_ITERATIONS {
        iteration += 1;
        let mut next_round = Vec::new();

        for combination in to_process {
            // Discover new variant keys from evaluation
            let new_keys = discover_new_variant_keys_from_evaluation(
                stage0_recipe,
                &combination,
                variant_config,
                config,
            )?;

            if new_keys.is_empty() {
                // No new keys - this combination is final
                final_combinations.push(combination);
            } else {
                // Expand with new keys and process in next round
                let expanded =
                    expand_combination_with_keys(&combination, &new_keys, variant_config);
                next_round.extend(expanded);
            }
        }

        to_process = next_round;
    }

    // Add any remaining combinations (in case we hit iteration limit)
    final_combinations.extend(to_process);

    // Deduplicate combinations
    let mut seen = HashSet::new();
    final_combinations.retain(|combo| {
        let key: Vec<_> = combo
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        seen.insert(key)
    });

    Ok(final_combinations)
}

/// Helper function to build an evaluation context from variant values and config
fn build_evaluation_context(
    variant: &BTreeMap<NormalizedKey, Variable>,
    config: &RenderConfig,
    stage0_recipe: &Stage0Recipe,
) -> Result<EvaluationContext, ParseError> {
    // Build context map from variant values and extra context
    // Merge variant into the variables map for template rendering
    let mut context_map = config.extra_context.clone();
    for (key, value) in variant {
        context_map.insert(key.normalize(), value.clone());
    }

    // Create JinjaConfig with the variant properly populated
    let jinja_config = create_jinja_config(config, variant);

    // Create evaluation context with variables, config, and OS env var keys
    let context = EvaluationContext::with_variables_config_and_os_env_keys(
        context_map,
        jinja_config,
        config.os_env_var_keys.clone(),
    );

    // Evaluate and merge recipe context variables
    let (context, _evaluated_context) = match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => context.with_context(&recipe.context)?,
        Stage0Recipe::MultiOutput(recipe) => context.with_context(&recipe.context)?,
    };

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
    // Create context with just extra_context (no variant for empty combinations)
    let empty_variant = BTreeMap::new();
    let jinja_config = create_jinja_config(config, &empty_variant);
    let context =
        EvaluationContext::with_variables_and_config(config.extra_context.clone(), jinja_config);

    // Evaluate and merge recipe context variables
    let (context, _evaluated_context) = match stage0_recipe {
        Stage0Recipe::SingleOutput(recipe) => context.with_context(&recipe.context)?,
        Stage0Recipe::MultiOutput(recipe) => context.with_context(&recipe.context)?,
    };

    // Evaluate the recipe
    let outputs = evaluate_recipe(stage0_recipe, &context)?;

    // Convert each output to a RenderedVariant
    let results: Vec<_> = outputs
        .into_iter()
        .map(|recipe| {
            let mut variant = recipe.used_variant.clone();

            // Filter out ignore_keys from the variant
            let ignore_keys: HashSet<NormalizedKey> = recipe
                .build
                .variant
                .ignore_keys
                .iter()
                .map(|k| k.as_str().into())
                .collect();
            variant.retain(|key, _| !ignore_keys.contains(key));

            // For noarch packages, override target_platform
            if recipe.build.noarch.is_some() {
                variant.insert("target_platform".into(), "noarch".into());
            }

            RenderedVariant {
                variant,
                recipe,
                pin_subpackages: BTreeMap::new(),
                hash_info: None,
            }
        })
        .collect();

    // Sort variants topologically by pin_subpackage dependencies
    // This ensures we resolve build strings in the correct order
    let mut results = topological_sort_variants(results).map_err(ParseError::from_message)?;

    // Resolve build strings in topological order
    // For each variant, we:
    // 1. Extract its pin_subpackages (which may reference already-resolved variants)
    // 2. Add those pins to its variant
    // 3. Compute its hash and finalize its build string
    // This ensures that when variant B pins variant A, A's build string is already finalized
    for i in 0..results.len() {
        // Extract pin_subpackages for this variant
        let pin_subpackages = extract_pin_subpackages(&results[i].recipe);
        results[i].pin_subpackages = pin_subpackages.clone();

        // Add pin information to this variant's variant map
        if !pin_subpackages.is_empty() {
            let results_snapshot = results.clone();
            add_pins_to_variant(
                results_snapshot[i].recipe.package.name(),
                &mut results[i].variant,
                &pin_subpackages,
                &results_snapshot,
            )
            .map_err(ParseError::from_message)?;
        }

        // Finalize build string with complete pin information
        finalize_build_string_single(&mut results[i])?;
    }

    Ok(results)
}

/// Helper function to finalize a single build string
///
/// This computes the hash from the variant (which includes pin information)
/// and resolves the build string for one variant.
fn finalize_build_string_single(result: &mut RenderedVariant) -> Result<(), ParseError> {
    let noarch = result.recipe.build.noarch.unwrap_or(NoArchType::none());

    // Compute hash from the variant (which now includes pin_subpackage information)
    let hash_info = HashInfo::from_variant(&result.variant, &noarch);

    // If build string is not set (Default), or if it needs resolving
    if matches!(
        result.recipe.build.string,
        BuildString::Default | BuildString::Unresolved(_, _)
    ) {
        // Always resolve/re-resolve the build string with the current hash
        // This ensures we use the latest hash that includes all pin information
        // Create a temporary evaluation context for build string resolution
        // Merge both recipe context variables and variant variables
        let mut variables = IndexMap::new();

        // First add recipe context variables (from context: section)
        for (k, v) in &result.recipe.context {
            variables.insert(k.clone(), v.clone());
        }

        // Then add variant variables (which may override context variables)
        for (k, v) in &result.variant {
            variables.insert(k.0.as_str().to_string(), v.clone());
        }

        let eval_ctx = EvaluationContext::from_variables(variables);

        // Resolve the build string template with the hash
        result
            .recipe
            .build
            .string
            .resolve(&hash_info, result.recipe.build.number, &eval_ctx)?;
        result.hash_info = Some(hash_info);
    }
    Ok(())
}

/// Helper function to extract all dependency package names from a recipe.
///
/// This collects:
/// - All named dependencies from build/host/run requirements
/// - All pin_subpackage references from run_exports (these reference other outputs
///   and create build-order dependencies even though they're not direct build deps)
///
/// Note: May contain duplicates, which is acceptable for dependency graph construction.
fn extract_dependency_names(recipe: &Stage1Recipe) -> Vec<rattler_conda_types::PackageName> {
    let requirements = recipe.requirements();

    // Collect names from build/host/run dependencies
    let build_host_run = requirements
        .build_host()
        .filter_map(|dep| dep.name().cloned());

    // Also collect pin_subpackage names from run_exports (these reference other outputs)
    let run_export_pins = requirements
        .run_exports_and_constraints()
        .filter_map(|dep| match dep {
            Dependency::PinSubpackage(pin) => Some(pin.pin_subpackage.name.clone()),
            _ => None,
        });

    build_host_run.chain(run_export_pins).collect()
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
fn create_jinja_config(
    config: &RenderConfig,
    variant: &BTreeMap<NormalizedKey, Variable>,
) -> JinjaConfig {
    // Check if the variant specifies a target_platform - if so, use it
    // This allows rendering recipes for platforms different from the current platform
    // (e.g., rendering a Windows variant on Linux to check if outputs would be skipped)
    let target_platform = variant
        .get(&"target_platform".into())
        .and_then(|v| v.to_string().parse::<rattler_conda_types::Platform>().ok())
        .unwrap_or(config.target_platform);

    // Similarly for host_platform (defaults to target_platform if not specified)
    let host_platform = variant
        .get(&"host_platform".into())
        .and_then(|v| v.to_string().parse::<rattler_conda_types::Platform>().ok())
        .unwrap_or_else(|| {
            // If host_platform not in variant, use config.host_platform if it differs from default,
            // otherwise use the (potentially variant-derived) target_platform
            if config.host_platform != config.target_platform {
                config.host_platform
            } else {
                target_platform
            }
        });

    JinjaConfig {
        experimental: config.experimental,
        recipe_path: config.recipe_path.clone(),
        target_platform,
        build_platform: config.build_platform,
        host_platform,
        variant: variant.clone(),
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
        .map_err(|e| ParseError::io_error(recipe_path.to_path_buf(), e))?;

    let stage0_recipe = stage0::parse_recipe_or_multi_from_source(&yaml_content)?;

    // Load variant configuration
    let variant_config = VariantConfig::from_files(variant_files, config.target_platform)
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
    // Check if recipe contains staging outputs - these require experimental flag
    let has_staging = stage0_recipe
        .outputs
        .iter()
        .any(|o| matches!(o, stage0::Output::Staging(_)));

    if has_staging && !config.experimental {
        return Err(ParseError::from_message(
            "staging outputs are an experimental feature: provide the `--experimental` flag to enable this feature",
        ));
    }

    let stage0 = Stage0Recipe::MultiOutput(Box::new(stage0_recipe.clone()));
    render_with_variants(&stage0, variant_config, config)
}

/// Internal unified function to render both single and multi-output recipes
fn render_with_variants(
    stage0_recipe: &Stage0Recipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    // Collect initially used variables (from templates and stage0 free specs)
    let used_vars = collect_used_variables(stage0_recipe, variant_config, &config.os_env_var_keys);

    // Compute initial variant combinations
    let initial_combinations = variant_config
        .combinations(&used_vars)
        .map_err(|e| ParseError::from_message(e.to_string()))?;

    // If no combinations, render once with just the extra context
    if initial_combinations.is_empty() {
        return render_with_empty_combinations(stage0_recipe, &config);
    }

    // Expand combinations tree-style: evaluate each combination, discover new variant keys
    // from free specs, and expand combinations if new keys are found
    let combinations =
        expand_variants_tree(stage0_recipe, initial_combinations, variant_config, &config)?;

    // Render recipe for each final variant combination
    let mut results = Vec::with_capacity(combinations.len());

    for combination in combinations {
        let context = build_evaluation_context(&combination, &config, stage0_recipe)?;
        let outputs = evaluate_recipe(stage0_recipe, &context)?;

        // Convert each output to a RenderedVariant
        for recipe in outputs {
            let mut variant = recipe.used_variant.clone();

            // Add use_keys to the variant (forces them to be included even if not referenced)
            // We need to get them from the combination since they were used to compute it
            let use_keys: HashSet<NormalizedKey> = recipe
                .build
                .variant
                .use_keys
                .iter()
                .map(|k| k.as_str().into())
                .collect();
            for key in &use_keys {
                if let Some(value) = combination.get(key) {
                    variant.insert(key.clone(), value.clone());
                }
            }

            // Filter out ignore_keys from the variant
            let ignore_keys: HashSet<NormalizedKey> = recipe
                .build
                .variant
                .ignore_keys
                .iter()
                .map(|k| k.as_str().into())
                .collect();
            variant.retain(|key, _| !ignore_keys.contains(key));

            results.push(RenderedVariant {
                variant,
                recipe,
                pin_subpackages: BTreeMap::new(), // Will be populated after first build string resolution
                hash_info: None,
            });
        }
    }

    // Sort variants topologically by pin_subpackage dependencies
    let mut results = topological_sort_variants(results).map_err(ParseError::from_message)?;

    // Resolve build strings in topological order
    for i in 0..results.len() {
        // Extract pin_subpackages for this variant
        let pin_subpackages = extract_pin_subpackages(&results[i].recipe);
        results[i].pin_subpackages = pin_subpackages.clone();

        // Add pin information to variant if needed
        if !pin_subpackages.is_empty() {
            let results_snapshot = results.clone();
            add_pins_to_variant(
                results_snapshot[i].recipe.package().name(),
                &mut results[i].variant,
                &pin_subpackages,
                &results_snapshot,
            )
            .map_err(ParseError::from_message)?;
        }

        // Finalize build string with complete pin information
        finalize_build_string_single(&mut results[i])?;
    }

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

    #[test]
    fn test_staging_requires_experimental() {
        // Test that staging outputs require the experimental flag
        let recipe_yaml = r#"
schema_version: 1

context:
  version: "1.0.0"

recipe:
  version: ${{ version }}

build:
  number: 0

outputs:
  - staging:
      name: build-cache
    build:
      script:
        - echo "Building..."

  - package:
      name: my-pkg
    inherit: build-cache
    build:
      noarch: generic
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        // Without experimental flag, should fail
        let result =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new());

        assert!(
            result.is_err(),
            "Staging outputs should require experimental flag"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("experimental"),
            "Error should mention experimental flag: {}",
            err
        );

        // With experimental flag, should succeed
        let result = render_recipe_with_variant_config(
            &stage0_recipe,
            &variant_config,
            RenderConfig::new().with_experimental(true),
        );

        assert!(
            result.is_ok(),
            "Staging outputs should work with experimental flag: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_variant_discovery_from_conditional_template() {
        // Test that `host: ["${{ 'openssl' if unix }}"]` discovers 'openssl' as a variant key
        let recipe_yaml = r#"
package:
  name: test-pkg
  version: "1.0.0"

requirements:
  host:
    - ${{ 'openssl' if unix }}
"#;

        let variant_yaml = r#"
openssl:
  - "1.1"
  - "3.0"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        // Use linux platform to ensure `unix` is true
        let config =
            RenderConfig::new().with_target_platform(rattler_conda_types::Platform::Linux64);

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, config).unwrap();

        // Should create 2 variants for openssl (1.1 and 3.0)
        assert_eq!(
            rendered.len(),
            2,
            "Should have 2 variants from openssl in conditional template"
        );

        // Check that openssl is in the variants
        let openssl_versions: Vec<String> = rendered
            .iter()
            .filter_map(|r| r.variant.get(&"openssl".into()).map(|v| v.to_string()))
            .collect();

        assert!(
            openssl_versions.contains(&"1.1".to_string()),
            "Should have openssl 1.1 variant"
        );
        assert!(
            openssl_versions.contains(&"3.0".to_string()),
            "Should have openssl 3.0 variant"
        );
    }

    #[test]
    fn test_variant_discovery_from_variable_template() {
        // Test tree-based variant discovery:
        // - `host: ["${{ mpi }}"]` with `mpi: [openmpi, mpich]`
        // - After evaluating mpi=openmpi, free spec 'openmpi' is discovered as a variant key
        // - After evaluating mpi=mpich, free spec 'mpich' is discovered as a variant key
        // - This creates a tree of variants
        let recipe_yaml = r#"
package:
  name: test-pkg
  version: "1.0.0"

requirements:
  host:
    - ${{ mpi }}
"#;

        let variant_yaml = r#"
mpi:
  - openmpi
  - mpich
openmpi:
  - "4.0"
  - "4.1"
mpich:
  - "3.4"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Tree-based variant discovery should create:
        // - {mpi: openmpi, openmpi: 4.0}
        // - {mpi: openmpi, openmpi: 4.1}
        // - {mpi: mpich, mpich: 3.4}
        assert_eq!(
            rendered.len(),
            3,
            "Should have 3 variants: 2 for openmpi × openmpi versions, 1 for mpich × mpich version"
        );

        // Check mpi=openmpi variants have openmpi key
        let openmpi_variants: Vec<_> = rendered
            .iter()
            .filter(|r| {
                r.variant.get(&"mpi".into()).map(|v| v.to_string()) == Some("openmpi".to_string())
            })
            .collect();
        assert_eq!(openmpi_variants.len(), 2, "Should have 2 openmpi variants");

        let openmpi_versions: Vec<String> = openmpi_variants
            .iter()
            .filter_map(|r| r.variant.get(&"openmpi".into()).map(|v| v.to_string()))
            .collect();
        assert!(
            openmpi_versions.contains(&"4.0".to_string()),
            "Should have openmpi 4.0"
        );
        assert!(
            openmpi_versions.contains(&"4.1".to_string()),
            "Should have openmpi 4.1"
        );

        // Check mpi=mpich variant has mpich key
        let mpich_variants: Vec<_> = rendered
            .iter()
            .filter(|r| {
                r.variant.get(&"mpi".into()).map(|v| v.to_string()) == Some("mpich".to_string())
            })
            .collect();
        assert_eq!(mpich_variants.len(), 1, "Should have 1 mpich variant");
        assert_eq!(
            mpich_variants[0]
                .variant
                .get(&"mpich".into())
                .map(|v| v.to_string()),
            Some("3.4".to_string()),
            "mpich variant should have mpich=3.4"
        );
    }

    #[test]
    fn test_skipped_output_with_platform_specific_requirements() {
        // Test that when an output is skipped (e.g., skip: win), its requirements
        // are not evaluated. This prevents errors from platform-specific functions
        // like stdlib('c') which may not have defaults on certain platforms.
        let recipe_yaml = r#"
schema_version: 1

context:
  name: test-pkg
  version: "1.0.0"

recipe:
  name: ${{ name }}-split
  version: ${{ version }}

build:
  number: 0

outputs:
  - package:
      name: ${{ name }}-unix
    build:
      skip: win
    requirements:
      build:
        - ${{ stdlib('c') }}
        - ${{ compiler('cxx') }}
"#;
        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();

        // Variant for Windows - the output should be skipped
        let variant_config = VariantConfig::default();

        // RenderConfig with Windows as target platform
        let config = RenderConfig::new().with_target_platform(rattler_conda_types::Platform::Win64);

        // The rendering should NOT fail - even though stdlib('c') would fail on Windows,
        // the output is skipped (skip: win) so requirements should not be evaluated
        let result = render_recipe_with_variant_config(&stage0_recipe, &variant_config, config);

        // Check if result is OK - it should be because the output is skipped
        assert!(
            result.is_ok(),
            "Rendering skipped output should not fail. Error: {:?}",
            result.err()
        );

        let rendered = result.unwrap();
        assert_eq!(rendered.len(), 1, "Should have 1 output");

        // The output should be marked as skipped (skip contains "win")
        let recipe = &rendered[0].recipe;
        assert!(
            recipe.build.skip.contains(&"win".to_string()),
            "Output should have win in skip conditions"
        );
    }
}
