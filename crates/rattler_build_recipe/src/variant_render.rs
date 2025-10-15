//! Variant-based recipe rendering
//!
//! This module provides functionality to render recipes with variant configurations,
//! allowing you to compute all build matrix combinations and evaluate recipes with
//! specific variant values.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
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
    /// The rendered stage1 recipe
    pub recipe: Stage1Recipe,
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

        return Ok(vec![RenderedVariant {
            variant: BTreeMap::new(),
            recipe,
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

        results.push(RenderedVariant { variant, recipe });
    }

    Ok(results)
}

fn render_multi_output_with_variants(
    _stage0_recipe: &MultiOutputRecipe,
    _variant_config: &VariantConfig,
    _config: RenderConfig,
) -> Result<Vec<RenderedVariant>, ParseError> {
    // TODO: Implement multi-output recipe variant rendering
    // For now, return an error indicating this is not yet supported
    Err(ParseError::from_message(
        "Variant rendering for multi-output recipes is not yet implemented",
    ))
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
}
