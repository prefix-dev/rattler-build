//! Output parsing is a bit more complicated than the other sections.
//!
//! The reason for this is that the `outputs` field is a list of mappings, and
//! each mapping can have its own `package`, `source`, `build`, `requirements`,
//! `test`, and `about` fields.

use marked_yaml::types::MarkedMappingNode;
use std::collections::HashMap;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{parse_yaml, Node},
        error::{ErrorKind, PartialParsingError},
        ParsingError,
    },
    source_code::SourceCode,
};

static DEEP_MERGE_KEYS: [&str; 4] = ["package", "about", "extra", "build"];
static ALLOWED_KEYS_MULTI_OUTPUTS: [&str; 10] = [
    "context",
    "recipe",
    "source",
    "build",
    "outputs",
    "about",
    "extra",
    "cache",
    "schema_version",
    "inherits",
];

// Check if the `cache` top-level key is present. If it does not contain a
// source, but there is a top-level `source` key, then we should warn the user
// because this key was moved to the `cache`
fn check_src_cache(root: &MarkedMappingNode) -> Result<(), PartialParsingError> {
    if let Some(cache) = root.get("cache") {
        let has_top_level_source = root.contains_key("source");
        let cache_map = cache.as_mapping().ok_or_else(|| {
            _partialerror!(
                *cache.span(),
                ErrorKind::ExpectedMapping,
                help = "`cache` must always be a mapping"
            )
        })?;

        if !cache_map.contains_key("source") && has_top_level_source {
            tracing::warn!("The cache has its own `source` key now. You probably want to move the top-level `source` key into the `cache` key.");
        }
    }

    Ok(())
}

/// Apply inheritance to an output map based on the provided ancestors
fn apply_inheritance<S: SourceCode>(
    output_map: &mut MarkedMappingNode,
    ancestors: &[MarkedMappingNode],
    src: &S,
) -> Result<(), PartialParsingError> {
    for ancestor in ancestors {
        for (key, value) in ancestor.iter() {
            if !output_map.contains_key(key) {
                output_map.insert(key.clone(), value.clone());
            } else if DEEP_MERGE_KEYS.contains(&key.as_str()) {
                // Deep merge for specific keys
                let output_map_span = *output_map.span();
                let Some(output_value) = output_map.get_mut(key) else {
                    return Err(_partialerror!(
                        output_map_span,
                        ErrorKind::MissingField(key.as_str().to_owned().into()),
                    ));
                };
                let output_value_span = *output_value.span();
                let Some(output_value_map) = output_value.as_mapping_mut() else {
                    return Err(_partialerror!(output_value_span, ErrorKind::ExpectedMapping,));
                };

                let mut ancestor_value = value.clone();
                let Some(ancestor_value_map) = ancestor_value.as_mapping_mut() else {
                    return Err(_partialerror!(*value.span(), ErrorKind::ExpectedMapping,));
                };

                for (key, value) in ancestor_value_map.iter() {
                    if !output_value_map.contains_key(key) {
                        output_value_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Find if the output has a name (either in package.name or cache.name)
fn get_output_name(output_map: &MarkedMappingNode) -> Option<String> {
    // Try to get name from package
    if let Some(package) = output_map.get("package") {
        if let Some(package_map) = package.as_mapping() {
            if let Some(name) = package_map.get("name") {
                if let Some(name_str) = name.as_str() {
                    return Some(name_str.to_string());
                }
            }
        }
    }
    
    // Try to get name from cache
    if let Some(cache) = output_map.get("cache") {
        if let Some(cache_map) = cache.as_mapping() {
            if let Some(name) = cache_map.get("name") {
                if let Some(name_str) = name.as_str() {
                    return Some(name_str.to_string());
                }
            }
        }
    }
    
    None
}

/// Resolve inheritance for an output
fn resolve_inheritance<S: SourceCode>(
    output_map: &mut MarkedMappingNode,
    named_outputs: &HashMap<String, MarkedMappingNode>,
    root: &MarkedMappingNode,
    src: &S,
    processed_names: &mut Vec<String>
) -> Result<(), PartialParsingError> {
    // Check for inherits field
    if let Some(inherits) = output_map.get("inherits") {
        let inherits_span = *inherits.span();
        let Some(inherits_seq) = inherits.as_sequence() else {
            return Err(_partialerror!(
                inherits_span,
                ErrorKind::ExpectedSequence,
                help = "`inherits` must be a sequence of strings"
            ));
        };
        
        let mut ancestors = Vec::new();
        
        for inherit_node in inherits_seq.iter() {
            let inherit_span = *inherit_node.span();
            let Some(inherit_name) = inherit_node.as_str() else {
                return Err(_partialerror!(
                    inherit_span,
                    ErrorKind::ExpectedString,
                    help = "each item in `inherits` must be a string"
                ));
            };
            
            // Check for circular dependencies
            if processed_names.contains(&inherit_name.to_string()) {
                return Err(_partialerror!(
                    inherit_span,
                    ErrorKind::InvalidValue(inherit_name.to_string().into()),
                    help = format!("circular dependency detected in `inherits`: {}", inherit_name)
                ));
            }
            
            let Some(ancestor) = named_outputs.get(inherit_name) else {
                return Err(_partialerror!(
                    inherit_span,
                    ErrorKind::InvalidValue(inherit_name.to_string().into()),
                    help = format!("referenced output `{}` not found", inherit_name)
                ));
            };
            
            ancestors.push(ancestor.clone());
        }
        
        // Apply inheritance from specified ancestors
        apply_inheritance(output_map, &ancestors, src)?;
        
        // Remove the inherits field after processing
        output_map.remove("inherits");
    } else {
        // Implicit inheritance from root if no inherits field
        apply_inheritance(output_map, &[root.clone()], src)?;
    }
    
    Ok(())
}

/// Retrieve all outputs from the recipe source (YAML)
pub fn find_outputs_from_src<S: SourceCode>(src: S) -> Result<Vec<Node>, ParsingError<S>> {
    let root_node = parse_yaml(0, src.clone())?;
    let root_map = root_node.as_mapping().ok_or_else(|| {
        ParsingError::from_partial(
            src.clone(),
            _partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                help = "root node must always be a mapping"
            ),
        )
    })?;

    if let Err(err) = check_src_cache(root_map) {
        return Err(ParsingError::from_partial(src, err));
    };

    // Validate the structure for multi-output recipes
    if root_map.contains_key("outputs") {
        if root_map.contains_key("package") {
            let key = root_map
                .keys()
                .find(|k| k.as_str() == "package")
                .expect("unreachable we preemptively check for if contains");
            return Err(ParsingError::from_partial(
                src.clone(),
                _partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField("package".to_string().into()),
                    help = "recipe cannot have both `outputs` and `package` fields. Rename `package` to `recipe` or remove `outputs`"
                ),
            ));
        }

        if root_map.contains_key("requirements") {
            let key = root_map
                .keys()
                .find(|k| k.as_str() == "requirements")
                .expect("unreachable we preemptively check for if contains");
            return Err(ParsingError::from_partial(
                src,
                _partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField("requirements".to_string().into()),
                    help = "multi-output recipes cannot have a top-level requirements field. Move `requirements` inside the individual output."
                ),
            ));
        }

        for key in root_map.keys() {
            if !ALLOWED_KEYS_MULTI_OUTPUTS.contains(&key.as_str()) {
                return Err(ParsingError::from_partial(
                    src,
                    _partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(key.as_str().to_string().into()),
                        help = format!("invalid key `{}` in root node", key.as_str())
                    ),
                ));
            }
        }
    }

    // Handle single output recipes (no outputs field)
    let Some(outputs) = root_map.get("outputs") else {
        let recipe =
            Node::try_from(root_node).map_err(|err| ParsingError::from_partial(src, err))?;
        return Ok(vec![recipe]);
    };

    let mut recipe_version: Option<marked_yaml::Node> = None;

    // Extract recipe version for defaulting
    if let Some(recipe_mapping) = root_map
        .get("recipe")
        .and_then(|recipe| recipe.as_mapping())
    {
        // make sure that mapping only contains name and version
        for (k, v) in recipe_mapping.iter() {
            match k.as_str() {
                "name" => {}
                "version" => recipe_version = Some(v.clone()),
                _ => {
                    return Err(ParsingError::from_partial(
                        src,
                        _partialerror!(
                            *k.span(),
                            ErrorKind::InvalidField(k.as_str().to_string().into()),
                            help = "recipe can only contain `name` and `version` fields"
                        ),
                    ));
                }
            }
        }
    }

    let Some(outputs) = outputs.as_sequence() else {
        return Err(ParsingError::from_partial(
            src,
            _partialerror!(
                *outputs.span(),
                ErrorKind::ExpectedSequence,
                help = "`outputs` must always be a sequence"
            ),
        ));
    };

    // First pass: collect all named outputs for inheritance
    let mut named_outputs = HashMap::new();
    let mut root = root_map.clone();
    root.remove("outputs");
    
    for output in outputs.iter() {
        let Some(output_map) = output.as_mapping() else {
            return Err(ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::ExpectedMapping,
                    help = "individual `output` must always be a mapping"
                ),
            ));
        };
        
        if let Some(name) = get_output_name(output_map) {
            named_outputs.insert(name, output_map.clone());
        }
    }

    let mut res = Vec::with_capacity(outputs.len());

    // Second pass: process outputs with inheritance
    for output in outputs.iter() {
        let mut output_node = output.clone();
        
        let Some(output_map) = output_node.as_mapping_mut() else {
            return Err(ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::ExpectedMapping,
                    help = "individual `output` must always be a mapping"
                ),
            ));
        };
        
        // Track processed outputs to detect circular dependencies
        let mut processed_names = Vec::new();
        if let Some(name) = get_output_name(output_map) {
            processed_names.push(name);
        }
        
        // Apply inheritance based on the inherits field
        if let Err(err) = resolve_inheritance(output_map, &named_outputs, &root, &src, &mut processed_names) {
            return Err(ParsingError::from_partial(src, err));
        }
        
        // Apply version from recipe if needed
        if let Some(version) = recipe_version.as_ref() {
            let Some(package_map) = output_map
                .get_mut("package")
                .and_then(|node| node.as_mapping_mut())
            else {
                return Err(ParsingError::from_partial(
                    src,
                    _partialerror!(
                        *output_node.span(),
                        ErrorKind::MissingField("package".to_string().into())
                    ),
                ));
            };

            if !package_map.contains_key("version") {
                package_map.insert("version".into(), version.clone());
            }
        }

        output_map.remove("recipe");

        let recipe = match Node::try_from(output_node) {
            Ok(node) => node,
            Err(err) => return Err(ParsingError::from_partial(src, err)),
        };
        res.push(recipe);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assert_miette_snapshot,
        recipe::{jinja::SelectorConfig, Recipe},
    };
    use fs_err as fs;
    use insta::assert_debug_snapshot;

    #[test]
    fn recipe_schema_error() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("recipes/test-parsing/recipe_outputs_and_package.yaml");
        let src = fs::read_to_string(yaml_file).unwrap();
        assert_miette_snapshot!(find_outputs_from_src(src.as_str()).unwrap_err());

        let yaml_file =
            test_data_dir.join("recipes/test-parsing/recipe_outputs_and_requirements.yaml");
        let src = fs::read_to_string(yaml_file).unwrap();
        assert_miette_snapshot!(find_outputs_from_src(src.as_str()).unwrap_err());

        let yaml_file = test_data_dir.join("recipes/test-parsing/recipe_missing_version.yaml");
        let src = fs::read_to_string(yaml_file).unwrap();
        let nodes = find_outputs_from_src(src.as_str()).unwrap();
        let parsed_recipe =
            Recipe::from_node(&nodes[0], SelectorConfig::default()).map_err(|err| {
                err.into_iter()
                    .map(|err| ParsingError::from_partial(src.as_str(), err))
                    .collect::<Vec<_>>()
            });
        let err: crate::variant_config::ParseErrors<_> = parsed_recipe.unwrap_err().into();
        assert_miette_snapshot!(err);

        let yaml_file = test_data_dir.join("recipes/test-parsing/recipe_outputs_extra_keys.yaml");
        let src = fs::read_to_string(yaml_file).unwrap();
        assert_miette_snapshot!(find_outputs_from_src(src.as_str()).unwrap_err());
    }

    #[test]
    fn recipe_outputs_merging() {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = test_data_dir.join("recipes/test-parsing/recipe_outputs_merging.yaml");
        let src = fs::read_to_string(yaml_file).unwrap();
        assert_debug_snapshot!(find_outputs_from_src(src.as_str()).unwrap());
    }
}
