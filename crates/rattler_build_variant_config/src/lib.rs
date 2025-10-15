//! # rattler_build_variant_config
//!
//! A standalone library for managing variant configurations in conda package builds.
//!
//! This crate provides functionality for:
//! - Loading variant configurations from YAML files (`variants.yaml`)
//! - Loading legacy `conda_build_config.yaml` files with selector support
//! - Computing all possible variant combinations (build matrices)
//! - Handling "zip keys" to synchronize related variants
//!
//! ## Example
//!
//! ```rust
//! use rattler_build_types::NormalizedKey;
//! use rattler_build_variant_config::VariantConfig;
//! use std::collections::HashSet;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Parse a variant configuration
//! let yaml = r#"
//! python:
//!   - "3.9"
//!   - "3.10"
//! numpy:
//!   - "1.20"
//!   - "1.21"
//! zip_keys:
//!   - [python, numpy]
//! "#;
//!
//! let config = VariantConfig::from_yaml_str(yaml)?;
//!
//! // Define which variables are actually used
//! let mut used_vars = HashSet::new();
//! used_vars.insert("python".into());
//! used_vars.insert("numpy".into());
//!
//! // Compute all combinations
//! let combinations = config.combinations(&used_vars, None)?;
//!
//! // With zip_keys, we get 2 combinations (not 3x2=6)
//! assert_eq!(combinations.len(), 2);
//! # Ok(())
//! # }
//! ```
//!
//! ## Variant Configuration Format
//!
//! A variant configuration is a YAML file that defines variables and their possible values:
//!
//! ```yaml
//! # Simple variants - creates a full cartesian product
//! python:
//!   - "3.9"
//!   - "3.10"
//! compiler:
//!   - gcc
//!   - clang
//!
//! # This creates 2x2 = 4 combinations:
//! # [python=3.9, compiler=gcc]
//! # [python=3.9, compiler=clang]
//! # [python=3.10, compiler=gcc]
//! # [python=3.10, compiler=clang]
//! ```
//!
//! ## Zip Keys
//!
//! Zip keys allow you to synchronize related variants:
//!
//! ```yaml
//! python:
//!   - "3.9"
//!   - "3.10"
//! numpy:
//!   - "1.20"
//!   - "1.21"
//! zip_keys:
//!   - [python, numpy]
//!
//! # This creates only 2 combinations:
//! # [python=3.9, numpy=1.20]
//! # [python=3.10, numpy=1.21]
//! ```
//!
//! ## Advanced Features (with `parser` feature)
//!
//! When the `parser` feature is enabled (default), you can use conditionals and Jinja expressions:
//!
//! ```yaml
//! python:
//!   - if: unix
//!     then: ["3.14", "3.15"]
//!   - if: win
//!     then: ["3.14"]
//!
//! foobar:
//!   - ${{ "unknown" if unix else "known" }}
//! ```
//!
//! ## conda_build_config Support
//!
//! This crate also supports loading legacy `conda_build_config.yaml` files:
//!
//! ```yaml
//! python:
//!   - 3.9
//!   - 3.10  # [unix]
//!   - 3.11  # [osx]
//! ```

pub mod combination;
pub mod conda_build_config;
pub mod config;
pub mod error;

#[cfg(feature = "parser")]
pub mod parser;

// Re-export main types
pub use combination::compute_combinations;
pub use conda_build_config::{SelectorContext, load_conda_build_config};
pub use config::VariantConfig;
pub use error::{VariantConfigError, VariantError, VariantExpandError};

#[cfg(feature = "parser")]
pub use parser::{parse_variant_file, parse_variant_str};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_basic_workflow() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
"#;

        let config = VariantConfig::from_yaml_str(yaml).unwrap();

        let mut used_vars = HashSet::new();
        used_vars.insert("python".into());
        used_vars.insert("numpy".into());

        let combos = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combos.len(), 4); // 2x2 combinations
    }

    #[test]
    fn test_with_zip_keys() {
        let yaml = r#"
python:
  - "3.9"
  - "3.10"
numpy:
  - "1.20"
  - "1.21"
zip_keys:
  - [python, numpy]
"#;

        let config = VariantConfig::from_yaml_str(yaml).unwrap();

        let mut used_vars = HashSet::new();
        used_vars.insert("python".into());
        used_vars.insert("numpy".into());

        let combos = config.combinations(&used_vars, None).unwrap();
        assert_eq!(combos.len(), 2); // Zipped: only 2 combinations
    }
}
