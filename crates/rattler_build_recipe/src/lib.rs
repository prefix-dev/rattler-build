pub mod error;
#[cfg(feature = "miette")]
pub mod source_code;
pub mod span;
pub mod stage0;
pub mod stage1;

#[cfg(feature = "variant-config")]
pub mod variant_render;

pub use error::{ErrorKind, ParseError, ParseErrors, ParseResult};
pub use span::{Span, SpannedString};
pub use stage0::Stage0Recipe;
pub use stage1::{Evaluate, EvaluationContext, Recipe as Stage1Recipe};

#[cfg(feature = "variant-config")]
pub use variant_render::{
    RenderConfig, RenderedVariant, render_recipe_with_variant_config, render_recipe_with_variants,
};
