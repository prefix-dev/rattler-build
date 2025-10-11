//! Common types and utilities shared between different output parsing modules

use crate::{
    _partialerror,
    recipe::{
        ParsingError,
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
    source_code::SourceCode,
};
use serde::{Deserialize, Serialize};

/// Represents how inheritance should be handled
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InheritSpec {
    /// Short form - just the cache name (inherits run_exports by default)
    Short(String),
    /// Long form - specifying what to inherit
    Long {
        /// The cache name to inherit from
        from: String,
        /// Whether to inherit run_exports (default: true)
        #[serde(default = "default_true")]
        run_exports: bool,
    },
}

fn default_true() -> bool {
    true
}

impl InheritSpec {
    /// Get the cache name to inherit from
    pub fn cache_name(&self) -> &str {
        match self {
            InheritSpec::Short(name) => name,
            InheritSpec::Long { from, .. } => from,
        }
    }

    /// Check if run_exports should be inherited
    pub fn inherit_run_exports(&self) -> bool {
        match self {
            InheritSpec::Short(_) => true,
            InheritSpec::Long { run_exports, .. } => *run_exports,
        }
    }
}

impl TryConvertNode<InheritSpec> for RenderedNode {
    fn try_convert(&self, _name: &str) -> Result<InheritSpec, Vec<PartialParsingError>> {
        // Try short form first (just a string)
        if let Some(scalar) = self.as_scalar() {
            return Ok(InheritSpec::Short(scalar.as_str().to_string()));
        }

        // Try long form (mapping)
        if let Some(mapping) = self.as_mapping() {
            let mut from = None;
            let mut run_exports = true;

            for (key, value) in mapping.iter() {
                match key.as_str() {
                    "from" => {
                        from = Some(
                            value
                                .as_scalar()
                                .ok_or_else(|| {
                                    vec![_partialerror!(
                                        *value.span(),
                                        ErrorKind::ExpectedScalar,
                                        help = "expected a string"
                                    )]
                                })?
                                .as_str()
                                .to_string(),
                        );
                    }
                    "run_exports" => {
                        let scalar = value.as_scalar().ok_or_else(|| {
                            vec![_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                help = "expected a boolean"
                            )]
                        })?;
                        run_exports = scalar.as_bool().ok_or_else(|| {
                            vec![_partialerror!(
                                *value.span(),
                                ErrorKind::ExpectedScalar,
                                help = "expected a boolean"
                            )]
                        })?;
                    }
                    _ => {
                        return Err(vec![_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(key.as_str().to_string().into())
                        )]);
                    }
                }
            }

            let from = from.ok_or_else(|| {
                vec![_partialerror!(
                    *mapping.span(),
                    ErrorKind::MissingField("from".to_string().into()),
                    help = "inherit mapping must have a 'from' field"
                )]
            })?;

            return Ok(InheritSpec::Long { from, run_exports });
        }

        Err(vec![_partialerror!(
            *self.span(),
            ErrorKind::ExpectedMapping,
            help = "inherit must be either a string or a mapping"
        )])
    }
}

/// Keys that should be deep merged when combining top-level and output-specific values
pub static DEEP_MERGE_KEYS: [&str; 4] = ["package", "about", "extra", "build"];

/// Allowed keys in the root mapping for multi-output recipes
pub static ALLOWED_KEYS_MULTI_OUTPUTS: [&str; 9] = [
    "context",
    "recipe",
    "source",
    "build",
    "outputs",
    "about",
    "extra",
    "cache",
    "schema_version",
];

/// Deep merge two mapping nodes for specific keys
#[allow(dead_code)]
pub fn deep_merge_mapping(
    output_map: &mut marked_yaml::types::MarkedMappingNode,
    root_map: &marked_yaml::types::MarkedMappingNode,
    keys_to_merge: &[&str],
) -> Result<(), Vec<PartialParsingError>> {
    for (key, value) in root_map.iter() {
        let key_str = key.as_str();

        if !output_map.contains_key(key) {
            output_map.insert(key.clone(), value.clone());
        } else if keys_to_merge.contains(&key_str) {
            // Deep merge specific keys
            let output_value = output_map.get_mut(key).unwrap();
            let output_value_span = *output_value.span();
            let output_value_map = output_value.as_mapping_mut().ok_or_else(|| {
                vec![_partialerror!(
                    output_value_span,
                    ErrorKind::ExpectedMapping
                )]
            })?;

            let mut root_value = value.clone();
            let root_value_map = root_value
                .as_mapping_mut()
                .ok_or_else(|| vec![_partialerror!(*value.span(), ErrorKind::ExpectedMapping)])?;

            merge_mapping_if_not_exists(output_value_map, root_value_map);
        }
    }

    Ok(())
}

/// Merge items from source mapping into target mapping if keys don't already exist
pub fn merge_mapping_if_not_exists(
    target: &mut marked_yaml::types::MarkedMappingNode,
    source: &marked_yaml::types::MarkedMappingNode,
) {
    for (k, v) in source.iter() {
        if !target.contains_key(k) {
            target.insert(k.clone(), v.clone());
        }
    }
}

/// Merge items from source mapping into target mapping if keys don't already exist (RenderedMappingNode version)
pub fn merge_rendered_mapping_if_not_exists(
    target: &mut RenderedMappingNode,
    source: &RenderedMappingNode,
) {
    for (k, v) in source.iter() {
        if !target.contains_key(k.as_str()) {
            target.insert(k.clone(), v.clone());
        }
    }
}

/// Validate recipe mapping and extract version
/// This helper validates that the recipe mapping only contains 'name' and 'version' fields
fn validate_and_extract_version<'a, S, K, V, I, F, G>(
    recipe_mapping: I,
    src: &S,
    key_to_str: F,
    key_span: G,
) -> Result<Option<V>, ParsingError<S>>
where
    S: SourceCode,
    K: 'a,
    V: Clone + 'a,
    I: Iterator<Item = (&'a K, &'a V)>,
    F: Fn(&K) -> &str,
    G: Fn(&K) -> marked_yaml::Span,
{
    let mut recipe_version: Option<V> = None;

    for (k, v) in recipe_mapping {
        match key_to_str(k) {
            "name" => {}
            "version" => recipe_version = Some(v.clone()),
            _ => {
                return Err(ParsingError::from_partial(
                    src.clone(),
                    _partialerror!(
                        key_span(k),
                        ErrorKind::InvalidField(key_to_str(k).to_string().into()),
                        help = "recipe can only contain `name` and `version` fields"
                    ),
                ));
            }
        }
    }

    Ok(recipe_version)
}

/// Extract recipe version from the recipe mapping (marked_yaml version)
pub fn extract_recipe_version_marked<S: SourceCode>(
    root_map: &marked_yaml::types::MarkedMappingNode,
    src: &S,
) -> Result<Option<marked_yaml::types::Node>, ParsingError<S>> {
    let Some(recipe_node) = root_map.get("recipe") else {
        return Ok(None);
    };

    let Some(recipe_mapping) = recipe_node.as_mapping() else {
        return Ok(None);
    };

    validate_and_extract_version(recipe_mapping.iter(), src, |k| k.as_str(), |k| *k.span())
}

/// Extract recipe version from the recipe mapping (RenderedNode version)
pub fn extract_recipe_version_rendered<S: SourceCode>(
    root_map: &RenderedMappingNode,
    src: &S,
) -> Result<Option<RenderedNode>, ParsingError<S>> {
    let Some(recipe_node) = root_map.get("recipe") else {
        return Ok(None);
    };

    let Some(recipe_mapping) = recipe_node.as_mapping() else {
        return Ok(None);
    };

    validate_and_extract_version(recipe_mapping.iter(), src, |k| k.as_str(), |k| *k.span())
}

/// Validate that outputs is a sequence and return it (RenderedNode version)
pub fn validate_outputs_sequence_rendered<'a, S: SourceCode>(
    outputs: &'a RenderedNode,
    src: &S,
) -> Result<&'a crate::recipe::custom_yaml::RenderedSequenceNode, ParsingError<S>> {
    outputs.as_sequence().ok_or_else(|| {
        ParsingError::from_partial(
            src.clone(),
            _partialerror!(
                *outputs.span(),
                ErrorKind::ExpectedSequence,
                help = "`outputs` must always be a sequence"
            ),
        )
    })
}

/// Parse root node as mapping (RenderedNode version)
pub fn parse_root_as_mapping_rendered<'a, S: SourceCode>(
    root_node: &'a RenderedNode,
    src: &S,
) -> Result<&'a RenderedMappingNode, ParsingError<S>> {
    root_node.as_mapping().ok_or_else(|| {
        ParsingError::from_partial(
            src.clone(),
            _partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                help = "root node must always be a mapping"
            ),
        )
    })
}
