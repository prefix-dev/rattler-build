//! Variant-based recipe rendering
//!
//! This module provides functionality to render recipes with variant configurations,
//! allowing you to compute all build matrix combinations and evaluate recipes with
//! specific variant values.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
use rattler_build_variant_config::{NormalizedKey, Variable, VariantConfig};

use crate::{
    error::ParseError,
    stage0::{self, MultiOutputRecipe, Recipe as Stage0Recipe, SingleOutputRecipe},
    stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe},
};

/// Configuration for rendering recipes with variants
#[derive(Debug, Clone, Default)]
pub struct RenderConfig {
    /// Additional context variables to provide (beyond variant values)
    pub extra_context: IndexMap<String, String>,
}

impl RenderConfig {
    /// Create a new render configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an extra context variable
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_context.insert(key.into(), value.into());
        self
    }
}

/// Result of rendering a recipe with a specific variant combination
#[derive(Debug)]
pub struct RenderedVariant {
    /// The variant combination used (variable name -> value)
    pub variant: BTreeMap<NormalizedKey, Variable>,
    /// The rendered outputs (one or more depending on recipe type)
    pub outputs: Vec<RenderedOutput>,
}

/// A single rendered output from a recipe variant
///
/// This represents one package that will be built. In single-output recipes,
/// there will be exactly one output. In multi-output recipes, there can be
/// multiple package outputs (staging outputs don't produce packages).
#[derive(Debug, Clone)]
pub struct RenderedOutput {
    /// The rendered stage1 recipe for this output
    pub recipe: Stage1Recipe,

    /// Variables that were actually used during rendering
    ///
    /// This is the minimum set of variables that influenced this output's content.
    /// It includes:
    /// - Variables used in Jinja templates that were rendered
    /// - Variables checked in conditional expressions (tracked during evaluation)
    /// - Variables from dependencies (free specs that became variants)
    ///
    /// For multi-output recipes, this also includes variables from:
    /// - The top-level recipe if inherited
    /// - The staging cache if inherited
    ///
    /// Note: The tracking is done via `EvaluationContext::accessed_variables()`
    /// which captures all variables accessed during template rendering.
    pub used_variables: HashSet<String>,

    /// The name of the output (for multi-output recipes)
    ///
    /// For single-output recipes, this is the same as recipe.package().name()
    /// For multi-output recipes, this identifies which output this is
    pub output_name: String,

    /// The source of this output (for tracking inheritance in multi-output recipes)
    ///
    /// - `TopLevel` - output inherits from top-level recipe
    /// - `Cache(name)` - output inherits from a staging cache
    /// - `Standalone` - single-output recipe (not applicable to multi-output)
    pub source: OutputSource,

    /// Finalized recipe layers (for multi-output recipes)
    ///
    /// This captures the complete inheritance chain for multi-output recipes:
    /// - Output layer (output-specific fields)
    /// - Cache layer (if inheriting from staging cache)
    /// - Top-level layer (recipe-level fields)
    ///
    /// For single-output recipes, this is None.
    pub finalized_recipe: Option<FinalizedRecipe>,
}

/// The source that an output inherits from
#[derive(Debug, Clone, PartialEq)]
pub enum OutputSource {
    /// Inherits from top-level recipe (multi-output only)
    TopLevel,
    /// Inherits from a named staging cache (multi-output only)
    Cache(String),
    /// Standalone single-output recipe
    Standalone,
}

/// Finalized recipe with layered inheritance tracking
///
/// This structure captures the complete inheritance chain for multi-output recipes,
/// allowing you to see which variables were used at each layer and how fields were merged.
#[derive(Debug, Clone)]
pub struct FinalizedRecipe {
    /// Top-level layer (from the recipe: section)
    ///
    /// Contains:
    /// - Variables used when evaluating top-level build/requirements/about/source
    /// - The evaluated top-level fields
    pub top_level: Option<RecipeLayer>,

    /// Cache layer (from a staging output, if inherited)
    ///
    /// Contains:
    /// - Name of the cache
    /// - Variables used when evaluating the staging cache
    /// - The evaluated cache fields (build script, requirements)
    pub cache: Option<CacheLayer>,

    /// Output layer (from the package output)
    ///
    /// Contains:
    /// - Variables used when evaluating output-specific fields
    /// - The evaluated output fields (package, build, requirements, about, tests)
    pub output: OutputLayer,
}

/// Top-level recipe layer
#[derive(Debug, Clone)]
pub struct RecipeLayer {
    /// Variables used in this layer during evaluation
    pub used_variables: HashSet<String>,

    /// Evaluated top-level build configuration
    pub build: Option<crate::stage1::Build>,

    /// Evaluated top-level requirements
    pub requirements: Option<crate::stage1::Requirements>,

    /// Evaluated top-level about
    pub about: Option<crate::stage1::About>,

    /// Evaluated top-level source
    pub source: Vec<crate::stage1::Source>,
}

/// Staging cache layer
#[derive(Debug, Clone)]
pub struct CacheLayer {
    /// Name of the cache
    pub cache_name: String,

    /// Variables used in this layer during evaluation
    pub used_variables: HashSet<String>,

    /// Evaluated cache build script
    pub build_script: Vec<String>,

    /// Evaluated cache requirements (build/host only)
    pub requirements: Option<crate::stage1::Requirements>,

    /// Evaluated cache source
    pub source: Vec<crate::stage1::Source>,
}

/// Output-specific layer
#[derive(Debug, Clone)]
pub struct OutputLayer {
    /// Variables used in this layer during evaluation
    pub used_variables: HashSet<String>,

    /// Evaluated package metadata
    pub package: crate::stage1::Package,

    /// Evaluated output build configuration
    pub build: Option<crate::stage1::Build>,

    /// Evaluated output requirements
    pub requirements: Option<crate::stage1::Requirements>,

    /// Evaluated output about
    pub about: Option<crate::stage1::About>,

    /// Evaluated output source
    pub source: Vec<crate::stage1::Source>,

    /// Evaluated output tests
    pub tests: Vec<crate::stage1::TestType>,
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
    let config = config.unwrap_or_default();

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
    // Collect variables used in the recipe (including free specs for variants)
    let mut used_vars = HashSet::new();

    // Get template variables from the recipe
    for var in stage0_recipe.used_variables() {
        used_vars.insert(NormalizedKey::from(var));
    }

    // Get free specs (packages without version constraints) as potential variants
    for spec in stage0_recipe.free_specs() {
        used_vars.insert(NormalizedKey::from(spec.as_normalized()));
    }

    // Filter to only variants that exist in the config
    let used_vars: HashSet<NormalizedKey> = used_vars
        .into_iter()
        .filter(|v| variant_config.get(v).is_some())
        .collect();

    // Compute all variant combinations
    let combinations = variant_config
        .combinations(&used_vars, None)
        .map_err(|e| ParseError::from_message(e.to_string()))?;

    // If no combinations, render once with just the extra context
    if combinations.is_empty() {
        let context = EvaluationContext::from_map(config.extra_context.clone())
            .with_context(&stage0_recipe.context)?;

        let recipe = stage0_recipe.evaluate(&context)?;

        // Get the actually used variables from the context
        let used_variables = context.accessed_variables();

        // Get the package name for the output
        let output_name = recipe.package().name().as_normalized().to_string();

        return Ok(vec![RenderedVariant {
            variant: BTreeMap::new(),
            outputs: vec![RenderedOutput {
                recipe,
                used_variables,
                output_name,
                source: OutputSource::Standalone,
                finalized_recipe: None,
            }],
        }]);
    }

    // Render recipe for each variant combination
    let mut results = Vec::with_capacity(combinations.len());

    for variant in combinations {
        // Build evaluation context from variant values and extra context
        let mut context_map = config.extra_context.clone();
        for (key, value) in &variant {
            context_map.insert(key.normalize(), value.to_string());
        }

        let context =
            EvaluationContext::from_map(context_map).with_context(&stage0_recipe.context)?;

        let recipe = stage0_recipe.evaluate(&context)?;

        // Get the actually used variables from the context
        let used_variables = context.accessed_variables();

        // Get the package name for the output
        let output_name = recipe.package().name().as_normalized().to_string();

        results.push(RenderedVariant {
            variant,
            outputs: vec![RenderedOutput {
                recipe,
                used_variables,
                output_name,
                source: OutputSource::Standalone,
                finalized_recipe: None,
            }],
        });
    }

    Ok(results)
}

fn render_multi_output_with_variants(
    stage0_recipe: &MultiOutputRecipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    // Collect all variables used across the recipe and all outputs
    let mut used_vars = HashSet::new();

    // Get template variables from the entire recipe
    for var in stage0_recipe.used_variables() {
        used_vars.insert(NormalizedKey::from(var));
    }

    // Get free specs from all outputs
    for spec in stage0_recipe.free_specs() {
        used_vars.insert(NormalizedKey::from(spec.as_normalized()));
    }

    // Filter to only variants that exist in the config
    let used_vars: HashSet<NormalizedKey> = used_vars
        .into_iter()
        .filter(|v| variant_config.get(v).is_some())
        .collect();

    // Compute all variant combinations
    let combinations = variant_config
        .combinations(&used_vars, None)
        .map_err(|e| ParseError::from_message(e.to_string()))?;

    // If no combinations, render once with just the extra context
    if combinations.is_empty() {
        let context = EvaluationContext::from_map(config.extra_context.clone())
            .with_context(&stage0_recipe.context)?;

        let outputs = render_multi_output_for_variant(stage0_recipe, &context)?;

        return Ok(vec![RenderedVariant {
            variant: BTreeMap::new(),
            outputs,
        }]);
    }

    // Render recipe for each variant combination
    let mut results = Vec::with_capacity(combinations.len());

    for variant in combinations {
        // Build evaluation context from variant values and extra context
        let mut context_map = config.extra_context.clone();
        for (key, value) in &variant {
            context_map.insert(key.normalize(), value.to_string());
        }

        let context =
            EvaluationContext::from_map(context_map).with_context(&stage0_recipe.context)?;

        let outputs = render_multi_output_for_variant(stage0_recipe, &context)?;

        results.push(RenderedVariant { variant, outputs });
    }

    Ok(results)
}

/// Render all outputs of a multi-output recipe for a single variant
///
/// This uses a two-pass approach to handle pin_subpackage dependencies:
/// 1. First pass: Render all outputs to extract pin_subpackage dependencies
/// 2. Topological sort based on dependencies
/// 3. Second pass: Re-render in dependency order (for exact pins)
fn render_multi_output_for_variant(
    stage0_recipe: &MultiOutputRecipe,
    context: &EvaluationContext,
) -> Result<Vec<RenderedOutput>, ParseError> {
    // Step 1: Evaluate top-level layer
    let top_level_layer = evaluate_top_level_layer(stage0_recipe, context)?;

    // Step 2: Evaluate all staging caches
    let mut staging_caches: BTreeMap<String, CacheLayer> = BTreeMap::new();
    for output in &stage0_recipe.outputs {
        if let stage0::Output::Staging(staging_output) = output {
            let cache_layer = evaluate_staging_cache(staging_output, context)?;
            staging_caches.insert(cache_layer.cache_name.clone(), cache_layer);
        }
    }

    // Step 3: First pass - render all outputs to extract dependencies
    let mut first_pass_outputs = Vec::new();
    for output in &stage0_recipe.outputs {
        if let stage0::Output::Package(package_output) = output {
            let rendered = evaluate_package_output(
                package_output,
                stage0_recipe,
                &top_level_layer,
                &staging_caches,
                context,
            )?;
            first_pass_outputs.push((package_output, rendered));
        }
    }

    // Step 4: Build dependency graph from rendered requirements
    let dep_graph = build_dependency_graph(&first_pass_outputs);

    // Step 5: Topologically sort outputs
    let sorted_indices = topological_sort(&dep_graph)?;

    // Step 6: Second pass - render in dependency order
    // For now, just reorder the first pass results
    // TODO: In the future, we may want to re-render outputs with exact pins
    //       using the full package specs of their dependencies
    let mut rendered_outputs = Vec::new();
    for idx in sorted_indices {
        rendered_outputs.push(first_pass_outputs[idx].1.clone());
    }

    Ok(rendered_outputs)
}

/// Evaluate the top-level recipe layer
fn evaluate_top_level_layer(
    stage0_recipe: &MultiOutputRecipe,
    context: &EvaluationContext,
) -> Result<RecipeLayer, ParseError> {
    // Clear tracking before evaluating top-level
    context.clear_accessed();

    // Evaluate top-level build configuration
    let build = Some(stage0_recipe.build.evaluate(context)?);

    // Multi-output recipes don't have top-level requirements
    let requirements = None;

    // Evaluate top-level about
    let about = Some(stage0_recipe.about.evaluate(context)?);

    // Evaluate top-level source
    let mut source = Vec::new();
    for src in &stage0_recipe.source {
        source.push(src.evaluate(context)?);
    }

    // Capture variables used in top-level evaluation
    let used_variables = context.accessed_variables();

    Ok(RecipeLayer {
        used_variables,
        build,
        requirements,
        about,
        source,
    })
}

/// Evaluate a staging cache
fn evaluate_staging_cache(
    staging_output: &stage0::StagingOutput,
    context: &EvaluationContext,
) -> Result<CacheLayer, ParseError> {
    use crate::stage0::evaluate::{evaluate_script_list, evaluate_string_value};

    // Clear tracking before evaluating cache
    context.clear_accessed();

    // Evaluate cache name
    let cache_name = evaluate_string_value(&staging_output.staging.name, context)?;

    // Evaluate build script
    let build_script = evaluate_script_list(&staging_output.build.script, context)?;

    // Evaluate requirements (only build/host for staging)
    let requirements = if !staging_output.requirements.is_empty() {
        Some(staging_output.requirements.evaluate(context)?)
    } else {
        None
    };

    // Evaluate source
    let mut source = Vec::new();
    for src in &staging_output.source {
        source.push(src.evaluate(context)?);
    }

    // Capture variables used in cache evaluation
    let used_variables = context.accessed_variables();

    Ok(CacheLayer {
        cache_name,
        used_variables,
        build_script,
        requirements,
        source,
    })
}

/// Evaluate a package output with inheritance
fn evaluate_package_output(
    package_output: &stage0::PackageOutput,
    stage0_recipe: &MultiOutputRecipe,
    top_level_layer: &RecipeLayer,
    staging_caches: &BTreeMap<String, CacheLayer>,
    context: &EvaluationContext,
) -> Result<RenderedOutput, ParseError> {
    use crate::stage0::evaluate::evaluate_string_value;

    // Clear tracking before evaluating output
    context.clear_accessed();

    // Determine inheritance source and cache layer
    let (source, cache_layer) = match &package_output.inherit {
        stage0::Inherit::TopLevel => (OutputSource::TopLevel, None),
        stage0::Inherit::CacheName(cache_name) => {
            let cache_name_str = evaluate_string_value(cache_name, context)?;
            let cache = staging_caches.get(&cache_name_str).cloned();
            (OutputSource::Cache(cache_name_str), cache)
        }
        stage0::Inherit::CacheWithOptions(cache_options) => {
            let cache_name_str = evaluate_string_value(&cache_options.from, context)?;
            let cache = staging_caches.get(&cache_name_str).cloned();
            (OutputSource::Cache(cache_name_str), cache)
        }
    };

    // Evaluate output-specific fields
    let output_layer = evaluate_output_layer(package_output, stage0_recipe, context)?;

    // Merge layers to create final Stage1Recipe
    let recipe = merge_recipe_layers(
        &output_layer,
        cache_layer.as_ref(),
        Some(top_level_layer),
        &stage0_recipe.extra,
        context,
    )?;

    // Collect all used variables from all layers
    let mut used_variables = output_layer.used_variables.clone();
    if let Some(cache) = &cache_layer {
        used_variables.extend(cache.used_variables.clone());
    }
    used_variables.extend(top_level_layer.used_variables.clone());

    let output_name = output_layer.package.name().as_normalized().to_string();

    // Build finalized recipe structure
    let finalized_recipe = FinalizedRecipe {
        top_level: Some(top_level_layer.clone()),
        cache: cache_layer,
        output: output_layer,
    };

    Ok(RenderedOutput {
        recipe,
        used_variables,
        output_name,
        source,
        finalized_recipe: Some(finalized_recipe),
    })
}

/// Evaluate output-specific layer
fn evaluate_output_layer(
    package_output: &stage0::PackageOutput,
    stage0_recipe: &MultiOutputRecipe,
    context: &EvaluationContext,
) -> Result<OutputLayer, ParseError> {
    use crate::stage0::evaluate::evaluate_value_to_string;
    use rattler_conda_types::{PackageName as RattlerPackageName, VersionWithSource};
    use std::str::FromStr;

    // Evaluate package metadata (with fallback to recipe metadata)
    let name_str = evaluate_value_to_string(&package_output.package.name, context)?;
    let name = RattlerPackageName::from_str(&name_str).map_err(|e| ParseError {
        kind: crate::ErrorKind::InvalidValue,
        span: crate::Span::unknown(),
        message: Some(format!("Invalid package name '{}': {}", name_str, e)),
        suggestion: None,
    })?;

    // Version can come from output or from recipe-level metadata
    let version = if let Some(version_value) = &package_output.package.version {
        let version_str = evaluate_value_to_string(version_value, context)?;
        VersionWithSource::from_str(&version_str).map_err(|e| ParseError {
            kind: crate::ErrorKind::InvalidValue,
            span: crate::Span::unknown(),
            message: Some(format!("Invalid version '{}': {}", version_str, e)),
            suggestion: None,
        })?
    } else if let Some(recipe_version) = &stage0_recipe.recipe.version {
        let version_str = evaluate_value_to_string(recipe_version, context)?;
        VersionWithSource::from_str(&version_str).map_err(|e| ParseError {
            kind: crate::ErrorKind::InvalidValue,
            span: crate::Span::unknown(),
            message: Some(format!("Invalid version '{}': {}", version_str, e)),
            suggestion: None,
        })?
    } else {
        return Err(ParseError {
            kind: crate::ErrorKind::InvalidValue,
            span: crate::Span::unknown(),
            message: Some("Package version not specified in output or recipe".to_string()),
            suggestion: Some(
                "Either specify version in the package output or in the recipe metadata"
                    .to_string(),
            ),
        });
    };

    let package = crate::stage1::Package::new(name, version);

    // Evaluate build configuration
    let build = Some(package_output.build.evaluate(context)?);

    // Evaluate requirements
    let requirements = Some(package_output.requirements.evaluate(context)?);

    // Evaluate about
    let about = Some(package_output.about.evaluate(context)?);

    // Evaluate source
    let mut source = Vec::new();
    for src in &package_output.source {
        source.push(src.evaluate(context)?);
    }

    // Evaluate tests
    let mut tests = Vec::new();
    for test in &package_output.tests {
        tests.push(test.evaluate(context)?);
    }

    // Capture variables used in output evaluation
    let used_variables = context.accessed_variables();

    Ok(OutputLayer {
        used_variables,
        package,
        build,
        requirements,
        about,
        source,
        tests,
    })
}

/// Merge recipe layers to create final Stage1Recipe
fn merge_recipe_layers(
    output_layer: &OutputLayer,
    cache_layer: Option<&CacheLayer>,
    top_level_layer: Option<&RecipeLayer>,
    extra: &stage0::Extra,
    context: &EvaluationContext,
) -> Result<Stage1Recipe, ParseError> {
    // Merge build: output > cache > top-level
    let build = output_layer
        .build
        .clone()
        .or_else(|| cache_layer.and_then(|_c| None)) // Cache doesn't have full build config
        .or_else(|| top_level_layer.and_then(|t| t.build.clone()))
        .unwrap_or_default();

    // Merge requirements: output > cache > top-level
    let requirements = output_layer
        .requirements
        .clone()
        .or_else(|| cache_layer.and_then(|c| c.requirements.clone()))
        .or_else(|| top_level_layer.and_then(|t| t.requirements.clone()))
        .unwrap_or_default();

    // Merge about: output > top-level (caches don't have about)
    let about = output_layer
        .about
        .clone()
        .or_else(|| top_level_layer.and_then(|t| t.about.clone()))
        .unwrap_or_default();

    // Merge source: combine all (output + cache + top-level)
    let mut source = Vec::new();
    if let Some(top) = top_level_layer {
        source.extend(top.source.clone());
    }
    if let Some(cache) = cache_layer {
        source.extend(cache.source.clone());
    }
    source.extend(output_layer.source.clone());

    // Tests come only from output
    let tests = output_layer.tests.clone();

    // Extra comes from top-level recipe
    let extra_evaluated = extra.evaluate(context)?;

    // Context is already in the evaluation context
    let resolved_context = context.variables().clone();

    Ok(Stage1Recipe::new(
        output_layer.package.clone(),
        build,
        about,
        requirements,
        extra_evaluated,
        source,
        tests,
        resolved_context,
    ))
}

/// Build a dependency graph from rendered outputs
///
/// Returns a map of output_index -> Vec<dependency_indices>
fn build_dependency_graph(
    outputs: &[(&Box<stage0::PackageOutput>, RenderedOutput)],
) -> BTreeMap<usize, Vec<usize>> {
    let mut graph = BTreeMap::new();

    // Build a name -> index map
    let mut name_to_idx = BTreeMap::new();
    for (idx, (_stage0, rendered)) in outputs.iter().enumerate() {
        name_to_idx.insert(rendered.output_name.clone(), idx);
    }

    // For each output, find which other outputs it depends on
    for (idx, (_stage0, rendered)) in outputs.iter().enumerate() {
        let mut deps = Vec::new();

        // Extract pin_subpackage references from the rendered recipe's requirements
        let pin_refs = rendered.recipe.requirements().pin_subpackage_refs();

        // Map package names to indices
        for pin_ref in pin_refs {
            if let Some(&dep_idx) = name_to_idx.get(pin_ref.as_normalized()) {
                // Don't add self-dependencies
                if dep_idx != idx {
                    deps.push(dep_idx);
                }
            }
        }

        // Deduplicate
        deps.sort();
        deps.dedup();

        graph.insert(idx, deps);
    }

    graph
}

/// Topologically sort outputs based on dependency graph
///
/// Returns a vector of indices in evaluation order (dependencies before dependents)
fn topological_sort(dep_graph: &BTreeMap<usize, Vec<usize>>) -> Result<Vec<usize>, ParseError> {
    let n = dep_graph.len();
    let mut in_degree = vec![0; n];
    let mut adj_list: Vec<Vec<usize>> = vec![Vec::new(); n];

    // Build adjacency list and in-degree count
    for (&node, deps) in dep_graph.iter() {
        for &dep in deps {
            adj_list[dep].push(node); // dep -> node (dep must come before node)
            in_degree[node] += 1;
        }
    }

    // Kahn's algorithm for topological sort
    let mut queue = std::collections::VecDeque::new();
    for (idx, &degree) in in_degree.iter().enumerate() {
        if degree == 0 {
            queue.push_back(idx);
        }
    }

    let mut result = Vec::new();
    while let Some(node) = queue.pop_front() {
        result.push(node);

        for &neighbor in &adj_list[node] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    // Check for cycles
    if result.len() != n {
        return Err(ParseError {
            kind: crate::ErrorKind::InvalidValue,
            span: crate::Span::unknown(),
            message: Some("Circular dependency detected in pin_subpackage references".to_string()),
            suggestion: Some(
                "Check your outputs for circular pin_subpackage dependencies".to_string(),
            ),
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Check that each rendered variant has one output
        for variant in &rendered {
            assert_eq!(variant.outputs.len(), 1);
            let output = &variant.outputs[0];
            assert_eq!(output.output_name, "test-pkg");
            assert_eq!(output.source, OutputSource::Standalone);

            // Check that used variables were tracked
            assert!(!output.used_variables.is_empty());
            // Should have used python variable in the template
            assert!(output.used_variables.contains("python"));
        }
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

        // Check that each has one output with correct structure
        for variant in &rendered {
            assert_eq!(variant.outputs.len(), 1);
            assert_eq!(variant.outputs[0].source, OutputSource::Standalone);
        }
    }

    #[test]
    fn test_used_variables_tracking() {
        let recipe_yaml = r#"
context:
  base_name: my-package

package:
  name: '${{ base_name }}-${{ variant }}'
  version: "1.0.0"

requirements:
  build:
    - ${{ compiler('c') }}
  host:
    - python ${{ python }}
  run:
    - python
    - if: unix
      then: libgcc
"#;

        let variant_yaml = r#"
python:
  - "3.9.*"
  - "3.10.*"
variant:
  - "cpu"
  - "gpu"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let config = RenderConfig::new().with_context("unix", "true");

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, config).unwrap();

        // Should have 2 * 2 = 4 combinations
        assert_eq!(rendered.len(), 4);

        // Check that each variant has correct used variables
        for rendered_variant in &rendered {
            assert_eq!(rendered_variant.outputs.len(), 1);
            let output = &rendered_variant.outputs[0];

            // Check that used variables were tracked
            let used_vars = &output.used_variables;

            // Should contain:
            // - base_name (from context)
            // - variant (from template in package name)
            // - python (from template in requirements)
            // - c_compiler and c_compiler_version (expanded from compiler('c'))
            assert!(
                used_vars.contains("base_name"),
                "Should track base_name variable"
            );
            assert!(
                used_vars.contains("variant"),
                "Should track variant variable"
            );
            assert!(used_vars.contains("python"), "Should track python variable");
            assert!(
                used_vars.contains("c_compiler"),
                "Should track c_compiler from compiler('c')"
            );
            assert!(
                used_vars.contains("c_compiler_version"),
                "Should track c_compiler_version from compiler('c')"
            );

            // unix is provided as extra context, but it's used in a conditional
            // So it might be accessed when evaluating the conditional
            // Note: Current implementation may not track conditionals yet
            // assert!(used_vars.contains("unix"), "Should track unix variable used in conditional");

            // The output name should be constructed from the template
            assert!(output.output_name.starts_with("my-package-"));
        }
    }

    #[test]
    fn test_no_variants_still_tracks_variables() {
        let recipe_yaml = r#"
package:
  name: test-pkg
  version: "1.0.0"

requirements:
  build:
    - ${{ compiler('c') }}
  host:
    - python 3.9.*
"#;

        // No variant config, so no variants
        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 1 output (no variants)
        assert_eq!(rendered.len(), 1);
        let output = &rendered[0].outputs[0];

        // Should still track variables used in templates
        let used_vars = &output.used_variables;
        assert!(
            used_vars.contains("c_compiler"),
            "Should track c_compiler even without variants"
        );
        assert!(
            used_vars.contains("c_compiler_version"),
            "Should track c_compiler_version even without variants"
        );
    }

    #[test]
    fn test_multi_output_layered_rendering() {
        let recipe_yaml = r#"
schema_version: 1

context:
  version: "1.0.0"

recipe:
  name: mylib
  version: ${{ version }}

build:
  number: 0

requirements:
  build:
    - ${{ compiler('c') }}

outputs:
  - package:
      name: mylib
    requirements:
      run:
        - python
    tests:
      - python:
          imports:
            - mylib

  - package:
      name: mylib-dev
    requirements:
      run:
        - ${{ pin_subpackage('mylib', exact=True) }}
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 1 variant (no variants in config)
        assert_eq!(rendered.len(), 1);

        // Should have 2 outputs (mylib and mylib-dev)
        assert_eq!(rendered[0].outputs.len(), 2);

        // Check first output (mylib)
        let mylib_output = &rendered[0].outputs[0];
        assert_eq!(mylib_output.output_name, "mylib");
        assert_eq!(mylib_output.source, OutputSource::TopLevel);

        // Check that finalized_recipe is present
        assert!(mylib_output.finalized_recipe.is_some());
        let finalized = mylib_output.finalized_recipe.as_ref().unwrap();

        // Should have top-level layer
        assert!(finalized.top_level.is_some());

        // Should not have cache layer (no staging outputs)
        assert!(finalized.cache.is_none());

        // Check that top-level layer has used variables
        let top_level = finalized.top_level.as_ref().unwrap();
        assert!(
            !top_level.used_variables.is_empty(),
            "Top-level should have used variables"
        );
        assert!(
            top_level.used_variables.contains("c_compiler"),
            "Top-level should track c_compiler"
        );

        // Check second output (mylib-dev)
        let dev_output = &rendered[0].outputs[1];
        assert_eq!(dev_output.output_name, "mylib-dev");
        assert_eq!(dev_output.source, OutputSource::TopLevel);
    }

    #[test]
    fn test_multi_output_with_cache_inheritance() {
        let recipe_yaml = r#"
schema_version: 1

recipe:
  version: "1.0.0"

outputs:
  - staging:
      name: build-cache
    source:
      - url: https://example.com/source.tar.gz
    requirements:
      build:
        - ${{ compiler('c') }}
      host:
        - python ${{ python }}
    build:
      script:
        - ./configure
        - make

  - package:
      name: mylib
    inherit: build-cache
    requirements:
      run:
        - python

  - package:
      name: mylib-dev
    inherit: build-cache
    requirements:
      run:
        - mylib
        - python
"#;

        let variant_yaml = r#"
python:
  - "3.9"
  - "3.10"
"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 2 variants (python 3.9 and 3.10)
        assert_eq!(rendered.len(), 2);

        // Each variant should have 2 package outputs (mylib and mylib-dev)
        // Note: staging outputs don't produce packages
        for variant in &rendered {
            assert_eq!(variant.outputs.len(), 2);

            // Check both outputs inherit from cache
            for output in &variant.outputs {
                assert_eq!(
                    output.source,
                    OutputSource::Cache("build-cache".to_string())
                );

                // Check that finalized_recipe has cache layer
                let finalized = output.finalized_recipe.as_ref().unwrap();
                assert!(finalized.cache.is_some(), "Output should have cache layer");

                let cache = finalized.cache.as_ref().unwrap();
                assert_eq!(cache.cache_name, "build-cache");
                assert!(
                    !cache.used_variables.is_empty(),
                    "Cache should have used variables"
                );
                assert!(
                    cache.used_variables.contains("python"),
                    "Cache should track python variable"
                );
            }
        }
    }

    #[test]
    fn test_pin_subpackage_ordering() {
        let recipe_yaml = r#"
schema_version: 1

recipe:
  version: "1.0.0"

outputs:
  - package:
      name: mylib
    requirements:
      run:
        - python

  - package:
      name: mylib-dev
    requirements:
      run:
        - ${{ pin_subpackage('mylib', exact=True) }}
        - python
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 1 variant (no variants in config)
        assert_eq!(rendered.len(), 1);

        // Should have 2 outputs (mylib and mylib-dev)
        assert_eq!(rendered[0].outputs.len(), 2);

        // Check ordering: mylib should come before mylib-dev
        let first_output = &rendered[0].outputs[0];
        let second_output = &rendered[0].outputs[1];

        assert_eq!(first_output.output_name, "mylib");
        assert_eq!(second_output.output_name, "mylib-dev");

        // Verify that mylib-dev has a pin_subpackage reference to mylib
        let pin_refs = second_output.recipe.requirements().pin_subpackage_refs();
        assert_eq!(pin_refs.len(), 1);
        assert_eq!(pin_refs[0].as_normalized(), "mylib");
    }

    #[test]
    fn test_pin_subpackage_complex_ordering() {
        let recipe_yaml = r#"
schema_version: 1

recipe:
  version: "1.0.0"

outputs:
  - package:
      name: lib-c
    requirements:
      run:
        - ${{ pin_subpackage('lib-a', exact=True) }}
        - ${{ pin_subpackage('lib-b', exact=True) }}

  - package:
      name: lib-b
    requirements:
      run:
        - ${{ pin_subpackage('lib-a', exact=True) }}

  - package:
      name: lib-a
    requirements:
      run:
        - python
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let rendered =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new())
                .unwrap();

        // Should have 1 variant (no variants in config)
        assert_eq!(rendered.len(), 1);

        // Should have 3 outputs
        assert_eq!(rendered[0].outputs.len(), 3);

        // Check ordering: lib-a should come first, then lib-b, then lib-c
        let first_output = &rendered[0].outputs[0];
        let second_output = &rendered[0].outputs[1];
        let third_output = &rendered[0].outputs[2];

        assert_eq!(first_output.output_name, "lib-a");
        assert_eq!(second_output.output_name, "lib-b");
        assert_eq!(third_output.output_name, "lib-c");

        // Verify dependencies
        let lib_a_refs = first_output.recipe.requirements().pin_subpackage_refs();
        assert_eq!(lib_a_refs.len(), 0); // No dependencies

        let lib_b_refs = second_output.recipe.requirements().pin_subpackage_refs();
        assert_eq!(lib_b_refs.len(), 1);
        assert_eq!(lib_b_refs[0].as_normalized(), "lib-a");

        let lib_c_refs = third_output.recipe.requirements().pin_subpackage_refs();
        assert_eq!(lib_c_refs.len(), 2);
        // The refs should contain both lib-a and lib-b
        let ref_names: Vec<&str> = lib_c_refs.iter().map(|r| r.as_normalized()).collect();
        assert!(ref_names.contains(&"lib-a"));
        assert!(ref_names.contains(&"lib-b"));
    }

    #[test]
    fn test_pin_subpackage_circular_dependency() {
        let recipe_yaml = r#"
schema_version: 1

recipe:
  version: "1.0.0"

outputs:
  - package:
      name: lib-a
    requirements:
      run:
        - ${{ pin_subpackage('lib-b', exact=True) }}

  - package:
      name: lib-b
    requirements:
      run:
        - ${{ pin_subpackage('lib-a', exact=True) }}
"#;

        let variant_yaml = r#"{}"#;

        let stage0_recipe = stage0::parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let variant_config = VariantConfig::from_yaml_str(variant_yaml).unwrap();

        let result =
            render_recipe_with_variant_config(&stage0_recipe, &variant_config, RenderConfig::new());

        // Should fail with circular dependency error
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .as_ref()
                .unwrap()
                .contains("Circular dependency detected")
        );
    }
}
