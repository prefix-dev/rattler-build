pub mod error;
pub mod source_code;
pub mod stage0;
pub mod stage1;

#[cfg(feature = "variant-config")]
pub mod variant_render;

pub use error::{ParseError, ParseErrorWithSource, ParseResult};
pub use stage0::Stage0Recipe;
pub use stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe};

#[cfg(feature = "variant-config")]
pub use variant_render::{
    RenderConfig, RenderedVariant, TopologicalSortError, render_recipe_with_variant_config,
    render_recipe_with_variants,
};

pub use marked_yaml::Span;
