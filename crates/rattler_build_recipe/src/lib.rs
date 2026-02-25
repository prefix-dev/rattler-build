pub mod error;
pub mod source_code;
pub mod stage0;
pub mod stage1;

#[cfg(feature = "variant-config")]
pub mod variant_render;

pub use error::{ParseError, ParseErrorWithSource, ParseResult, WithSourceCode};
pub use stage0::Stage0Recipe;
pub use stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe};

#[cfg(feature = "variant-config")]
pub use variant_render::{
    RenderConfig, RenderError, RenderedVariant, TopologicalSortError,
    render_recipe_with_variant_config,
};

#[cfg(all(feature = "variant-config", not(target_arch = "wasm32")))]
pub use variant_render::render_recipe_with_variants;

pub use marked_yaml::Span;

/// Convenience type alias for a [`RenderError`] wrapped with source code.
#[cfg(feature = "variant-config")]
pub type RenderErrorWithSource<S> = WithSourceCode<RenderError, S>;

/// Parse a recipe from source, returning errors with source code attached for diagnostics.
///
/// This is a thin wrapper around [`stage0::parse_recipe_or_multi_from_source`] that
/// attaches source code context to any parse errors for better miette output.
#[allow(clippy::result_large_err)]
pub fn parse_recipe(
    source: &source_code::Source,
) -> Result<stage0::Recipe, ParseErrorWithSource<source_code::Source>> {
    stage0::parse_recipe_or_multi_from_source(source.as_ref())
        .map_err(|e| ParseErrorWithSource::new(source.clone(), e))
}

/// Render a recipe with variant config, returning errors with source code attached.
///
/// This is a thin wrapper around [`variant_render::render_recipe_with_variant_config`]
/// that attaches source code context to any render errors for better miette output.
#[cfg(feature = "variant-config")]
#[allow(clippy::result_large_err)]
pub fn render_recipe(
    source: &source_code::Source,
    stage0_recipe: &stage0::Recipe,
    variant_config: &rattler_build_variant_config::VariantConfig,
    config: RenderConfig,
) -> Result<Vec<RenderedVariant>, RenderErrorWithSource<source_code::Source>> {
    variant_render::render_recipe_with_variant_config(stage0_recipe, variant_config, config)
        .map_err(|e| WithSourceCode::new(source.clone(), e))
}
