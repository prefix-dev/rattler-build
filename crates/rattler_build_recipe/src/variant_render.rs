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
use rattler_build_jinja::{JinjaConfig, Variable};
use rattler_build_types::NormalizedKey;
use rattler_build_variant_config::VariantConfig;

use crate::{
    error::ParseError,
    stage0::{self, MultiOutputRecipe, Recipe as Stage0Recipe, SingleOutputRecipe},
    stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe},
};

/// Configuration for rendering recipes with variants
#[derive(Debug, Clone, Default)]
pub struct RenderConfig {
    /// Additional context variables to provide (beyond variant values)
    /// These can be strings, booleans, numbers, etc. using the Variable type
    pub extra_context: IndexMap<String, Variable>,
    /// Whether experimental features are enabled
    pub experimental: bool,
    /// Path to the recipe file (for relative path resolution in Jinja functions)
    pub recipe_path: Option<PathBuf>,
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
}

/// Result of rendering a recipe with a specific variant combination
#[derive(Debug)]
pub struct RenderedVariant {
    /// The variant combination used (variable name -> value)
    pub variant: BTreeMap<NormalizedKey, Variable>,
    /// The rendered stage1 recipe
    pub recipe: Stage1Recipe,
}

/// Helper function to create a JinjaConfig from RenderConfig
fn create_jinja_config(config: &RenderConfig) -> JinjaConfig {
    use rattler_conda_types::Platform;
    use std::str::FromStr;

    let mut jinja_config = JinjaConfig::default();
    jinja_config.experimental = config.experimental;
    jinja_config.recipe_path = config.recipe_path.clone();

    // Extract platform information from extra_context
    if let Some(target_platform_var) = config.extra_context.get("target_platform") {
        if let Some(platform_str) = target_platform_var.as_ref().as_str() {
            if let Ok(platform) = Platform::from_str(platform_str) {
                jinja_config.target_platform = platform;
            }
        }
    }

    if let Some(build_platform_var) = config.extra_context.get("build_platform") {
        if let Some(platform_str) = build_platform_var.as_ref().as_str() {
            if let Ok(platform) = Platform::from_str(platform_str) {
                jinja_config.build_platform = platform;
            }
        }
    }

    if let Some(host_platform_var) = config.extra_context.get("host_platform") {
        if let Some(platform_str) = host_platform_var.as_ref().as_str() {
            if let Ok(platform) = Platform::from_str(platform_str) {
                jinja_config.host_platform = platform;
            }
        }
    }

    jinja_config
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

    // Insert always-included variables
    const ALWAYS_INCLUDE: &[&str] = &["target_platform", "channel_targets", "channel_sources"];
    for key in ALWAYS_INCLUDE {
        used_vars.insert(NormalizedKey::from(*key));
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
        // Use from_variables to preserve Variable types (e.g., booleans)
        let mut context = EvaluationContext::from_variables(config.extra_context.clone())
            .with_context(&stage0_recipe.context)?;

        // Set the JinjaConfig with experimental and recipe_path
        context.set_jinja_config(create_jinja_config(&config));

        let recipe = stage0_recipe.evaluate(&context)?;

        // Include only target_platform in the variant (matches conda-build behavior)
        // build_platform and host_platform are available in the Jinja context but not in the hash
        let mut variant: BTreeMap<NormalizedKey, Variable> = BTreeMap::new();

        // For noarch packages, override target_platform to "noarch"
        if recipe.build.noarch.is_some() {
            variant.insert("target_platform".into(), "noarch".into());
        }

        return Ok(vec![RenderedVariant { variant, recipe }]);
    }

    // Render recipe for each variant combination
    let mut results = Vec::with_capacity(combinations.len());

    for variant in combinations {
        // Build evaluation context from variant values and extra context
        // Preserve Variable types (e.g., booleans for platform selectors)
        let mut context_map = config.extra_context.clone();
        for (key, value) in &variant {
            context_map.insert(key.normalize(), value.clone());
        }

        let mut context =
            EvaluationContext::from_variables(context_map).with_context(&stage0_recipe.context)?;

        // Set the JinjaConfig with experimental and recipe_path
        context.set_jinja_config(create_jinja_config(&config));

        let recipe = stage0_recipe.evaluate(&context)?;

        results.push(RenderedVariant {
            variant: recipe.used_variant.clone(),
            recipe,
        });
    }

    Ok(results)
}

fn render_multi_output_with_variants(
    stage0_recipe: &MultiOutputRecipe,
    variant_config: &VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    // Collect all used variables from Jinja templates across all outputs
    let mut used_vars = HashSet::new();
    for var in stage0_recipe.used_variables() {
        used_vars.insert(NormalizedKey::from(var));
    }

    // Get free specs (packages without version constraints) as potential variants
    for spec in stage0_recipe.free_specs() {
        used_vars.insert(NormalizedKey::from(spec.as_normalized()));
    }

    // Insert always-included variables
    const ALWAYS_INCLUDE: &[&str] = &["target_platform", "channel_targets", "channel_sources"];
    for key in ALWAYS_INCLUDE {
        used_vars.insert(NormalizedKey::from(*key));
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
        let mut context = EvaluationContext::from_variables(config.extra_context.clone())
            .with_context(&stage0_recipe.context)?;

        context.set_jinja_config(create_jinja_config(&config));

        // Evaluate the multi-output recipe - this returns Vec<Stage1Recipe>
        let outputs = stage0_recipe.evaluate(&context)?;

        // Convert each output to a RenderedVariant
        let mut results = Vec::new();
        for recipe in outputs {
            // Use the used_variant from the evaluated recipe
            let variant = recipe.used_variant.clone();
            results.push(RenderedVariant { variant, recipe });
        }

        return Ok(results);
    }

    // Render recipe for each variant combination
    let mut results = Vec::new();

    for variant in combinations {
        // Build evaluation context from variant values and extra context
        let mut context_map = config.extra_context.clone();
        for (key, value) in &variant {
            context_map.insert(key.normalize(), value.clone());
        }

        let mut context =
            EvaluationContext::from_variables(context_map).with_context(&stage0_recipe.context)?;

        context.set_jinja_config(create_jinja_config(&config));

        // Evaluate the multi-output recipe - this returns Vec<Stage1Recipe>
        // Each output already has its hash and variant computed correctly
        let outputs = stage0_recipe.evaluate(&context)?;

        // Convert each output to a RenderedVariant
        for recipe in outputs {
            // Use the used_variant from the evaluated recipe
            let variant = recipe.used_variant.clone();
            results.push(RenderedVariant { variant, recipe });
        }
    }

    Ok(results)
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

        // TODO: Once we implement proper hash computation, check the variant includes the pinned package:
        // assert!(extra_pkg.variant.contains_key(&"my_pkg".into()) || extra_pkg.variant.contains_key(&"my-pkg".into()));
    }
}
