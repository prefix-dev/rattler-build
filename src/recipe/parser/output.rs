//! Output parsing is a bit more complicated than the other sections.
//!
//! The reason for this is that the `outputs` field is a list of mappings, and
//! each mapping can have its own `package`, `source`, `build`, `requirements`,
//! `test`, and `about` fields.

use marked_yaml::types::MarkedMappingNode;

use crate::{
    _partialerror,
    recipe::{
        ParsingError, Render,
        custom_yaml::{HasSpan, Node, RenderedNode, TryConvertNode, parse_yaml},
        error::{ErrorKind, PartialParsingError},
    },
    source_code::SourceCode,
};

use super::common_output::{
    ALLOWED_KEYS_MULTI_OUTPUTS, DEEP_MERGE_KEYS, extract_recipe_version_marked,
    merge_mapping_if_not_exists,
};

/// Result type for resolve_cache_inheritance_with_caches function
type CacheInheritanceResult = Result<
    (
        Vec<Node>,
        Vec<crate::recipe::parser::CacheOutput>,
        std::collections::HashMap<String, Vec<String>>,
    ),
    ParsingError<&'static str>,
>;

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
            tracing::warn!(
                "The cache has its own `source` key now. You probably want to move the top-level `source` key into the `cache` key."
            );
        }
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

        // Require explicit build scripts for multi-output recipes
        if !root_map.contains_key("build") {
            if let Some(outputs_value) = root_map.get("outputs") {
                if let Some(outputs_seq) = outputs_value.as_sequence() {
                    let missing_script = outputs_seq.iter().any(|output| {
                        output
                            .as_mapping()
                            .and_then(|mapping| mapping.get("cache").or_else(|| mapping.get("package")))
                            .map(|subnode| {
                                subnode
                                    .as_mapping()
                                    .and_then(|mapping| mapping.get("build"))
                                    .is_none()
                            })
                            .unwrap_or(true)
                    });

                    if missing_script {
                        let key = outputs_value
                            .span();
                        return Err(ParsingError::from_partial(
                            src,
                            _partialerror!(
                                *key,
                                ErrorKind::MissingField("build.script".into()),
                                help = "Multi-output recipes must specify `build.script` for each output; implicit build/script.sh is not supported"
                            ),
                        ));
                    }
                }
            }
        }
    }

    let Some(outputs) = root_map.get("outputs") else {
        let recipe =
            Node::try_from(root_node).map_err(|err| ParsingError::from_partial(src, err))?;
        return Ok(vec![recipe]);
    };

    // Extract recipe version from the recipe mapping
    let recipe_version = extract_recipe_version_marked(root_map, &src)?;

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

    let mut res = Vec::with_capacity(outputs.len());

    // the schema says that `outputs` can be either an output, a if-selector or a
    // sequence of outputs and if-selectors. We need to handle all of these
    // cases but for now, lets handle only sequence of outputs
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

        // Check if this is a cache output
        if output_map.contains_key("cache") {
            // Parse as cache output - don't merge top-level fields into cache outputs
            let cache_node = Node::try_from(output_node)
                .map_err(|err| ParsingError::from_partial(src.clone(), err))?;
            res.push(cache_node);
            continue;
        }

        let mut root = root_map.clone();
        root.remove("outputs");

        for (key, value) in root.iter() {
            if !output_map.contains_key(key) {
                output_map.insert(key.clone(), value.clone());
            } else {
                // deep merge
                if DEEP_MERGE_KEYS.contains(&key.as_str()) {
                    let output_map_span = *output_map.span();
                    let Some(output_value) = output_map.get_mut(key) else {
                        return Err(ParsingError::from_partial(
                            src,
                            _partialerror!(
                                output_map_span,
                                ErrorKind::MissingField(key.as_str().to_owned().into()),
                            ),
                        ));
                    };
                    let output_value_span = *output_value.span();
                    let Some(output_value_map) = output_value.as_mapping_mut() else {
                        return Err(ParsingError::from_partial(
                            src,
                            _partialerror!(output_value_span, ErrorKind::ExpectedMapping,),
                        ));
                    };

                    let mut root_value = value.clone();
                    let Some(root_value_map) = root_value.as_mapping_mut() else {
                        return Err(ParsingError::from_partial(
                            src,
                            _partialerror!(*value.span(), ErrorKind::ExpectedMapping,),
                        ));
                    };

                    // Do not merge top-level build.script into outputs
                    if key.as_str() == "build" {
                        root_value_map.remove("script");
                    }

                    merge_mapping_if_not_exists(output_value_map, root_value_map);
                }
            }
        }

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

        let inherit_spec = output_map
            .get("package")
            .and_then(|pkg_node| pkg_node.as_mapping())
            .and_then(|pkg_map| pkg_map.get("inherit"));

        match inherit_spec {
            Some(inherit_node) if inherit_node.is_null() => {}
            Some(_) => {
            }
            None => {
                if let Some(req_node) = output_map.get("requirements") {
                    return Err(ParsingError::from_partial(
                        src.clone(),
                        _partialerror!(
                            *req_node.span(),
                            ErrorKind::InvalidField("requirements".to_string().into()),
                            help = "When inheriting from top-level, outputs must not define `requirements`."
                        ),
                    ));
                }
                if let Some(build_node) = output_map.get("build") {
                    if let Some(build_map) = build_node.as_mapping() {
                        if build_map.contains_key("script") {
                            return Err(ParsingError::from_partial(
                                src.clone(),
                                _partialerror!(
                                    *build_node.span(),
                                    ErrorKind::InvalidField("build.script".to_string().into()),
                                    help = "When inheriting from top-level, outputs must not define `build.script`."
                                ),
                            ));
                        }
                    }
                }
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

/// Resolve cache inheritance relationships between package outputs and cache outputs
///
/// This function validates that cache inheritance references are valid.
/// The actual inheritance resolution happens during recipe parsing.
pub fn resolve_cache_inheritance(
    outputs: Vec<Node>,
    has_toplevel_cache: bool,
) -> Result<Vec<Node>, ParsingError<&'static str>> {
    use std::collections::HashSet;

    // Collect cache names from outputs array
    let mut cache_names = HashSet::new();
    let mut duplicate_caches = Vec::new();

    let mut cache_name_spans = std::collections::HashMap::new();

    for output in outputs.iter() {
        if let Some(cache_output) = parse_cache_output_from_node(output)? {
            let cache_name = cache_output.name.clone();
            if !cache_names.insert(cache_name.clone()) {
                duplicate_caches.push(cache_name);
            } else {
                cache_name_spans.insert(cache_output.name.clone(), cache_output.span);
            }
        }
    }

    if !duplicate_caches.is_empty() {
        return Err(ParsingError::from_partial(
            "",
            _partialerror!(
                marked_yaml::Span::new_blank(),
                ErrorKind::InvalidField(
                    format!("duplicate cache names: {}", duplicate_caches.join(", ")).into()
                ),
                help = "Each cache output must have a unique name"
            ),
        ));
    }

    for output in outputs.iter() {
        if let Some(mapping) = output.as_mapping() {
            if mapping.contains_key("package") {
                if let Some(package_node) = mapping.get("package") {
                    if let Some(package_mapping) = package_node.as_mapping() {
                        if let Some(inherit_node) = package_mapping.get("inherit") {
                            let cache_name = if let Some(inherit_scalar) = inherit_node.as_scalar()
                            {
                                inherit_scalar.as_str().to_string()
                            } else if let Some(inherit_mapping) = inherit_node.as_mapping() {
                                if let Some(from_node) = inherit_mapping.get("from") {
                                    if let Some(from_scalar) = from_node.as_scalar() {
                                        from_scalar.as_str().to_string()
                                    } else {
                                        continue;
                                    }
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            };

                            // Check if cache exists (in outputs array)
                            // Note: Explicit inheritance takes precedence over top-level cache.
                            // If an output explicitly inherits from a named cache, it must exist
                            // in the outputs array.
                            if !cache_names.contains(&cache_name) {
                                let available: Vec<_> =
                                    cache_names.iter().map(|n| format!("'{}'", n)).collect();
                                let help_msg = if available.is_empty() {
                                    if has_toplevel_cache {
                                        "No cache outputs defined in outputs array. To use top-level cache, omit the 'inherit' key.".to_string()
                                    } else {
                                        "No cache outputs defined".to_string()
                                    }
                                } else {
                                    format!("Available caches: {}", available.join(", "))
                                };
                                return Err(ParsingError::from_partial(
                                    "",
                                    _partialerror!(
                                        *output.span(),
                                        ErrorKind::InvalidField(
                                            format!("cache '{}' not found", cache_name).into()
                                        ),
                                        help = help_msg
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(outputs)
}

/// Parse a cache output from a Node using proper TryConvertNode
fn parse_cache_output_from_node(
    output: &Node,
) -> Result<Option<crate::recipe::parser::CacheOutput>, ParsingError<&'static str>> {
    // Convert Node to RenderedNode to use TryConvertNode
    // For now, we'll use a simplified approach since we don't have jinja context here
    // In a full implementation, we would need to render the node first

    if let Some(mapping) = output.as_mapping() {
        if mapping.contains_key("cache") {
            if let Some(cache_node) = mapping.get("cache") {
                if let Some(cache_mapping) = cache_node.as_mapping() {
                    let name = if let Some(name_node) = cache_mapping.get("name") {
                        if let Some(name_scalar) = name_node.as_scalar() {
                            Some(name_scalar.as_str().to_string())
                        } else {
                            return Err(ParsingError::from_partial(
                                "",
                                _partialerror!(
                                    *name_node.span(),
                                    ErrorKind::ExpectedScalar,
                                    help = "cache name must be a string"
                                ),
                            ));
                        }
                    } else {
                        None
                    };

                    // For now, create a basic cache output with defaults
                    return Ok(Some(crate::recipe::parser::CacheOutput {
                        name: name.unwrap_or_else(|| "default".to_string()),
                        source: Vec::new(),
                        build: crate::recipe::parser::CacheBuild::default(),
                        requirements: crate::recipe::parser::CacheRequirements::default(),
                        run_exports: crate::recipe::parser::RunExports::default(),
                        ignore_run_exports: None,
                    }));
                }
            }
        }
    }
    Ok(None)
}

/// Parse inheritance relationships from outputs
fn parse_inheritance_relationships(
    outputs: &[Node],
) -> Result<std::collections::HashMap<String, Vec<String>>, ParsingError<&'static str>> {
    let mut relationships = std::collections::HashMap::new();

    for output in outputs {
        if let Some(mapping) = output.as_mapping() {
            if mapping.contains_key("package") {
                if let Some(package_node) = mapping.get("package") {
                    if let Some(package_mapping) = package_node.as_mapping() {
                        let package_name = if let Some(name_node) = package_mapping.get("name") {
                            if let Some(name_scalar) = name_node.as_scalar() {
                                name_scalar.as_str().to_string()
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        };

                        if let Some(inherit_node) = package_mapping.get("inherit") {
                            let cache_name = if let Some(inherit_scalar) = inherit_node.as_scalar()
                            {
                                inherit_scalar.as_str().to_string()
                            } else if let Some(inherit_mapping) = inherit_node.as_mapping() {
                                if let Some(from_node) = inherit_mapping.get("from") {
                                    if let Some(from_scalar) = from_node.as_scalar() {
                                        from_scalar.as_str().to_string()
                                    } else {
                                        continue;
                                    }
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            };

                            relationships
                                .entry(package_name)
                                .or_insert_with(Vec::new)
                                .push(cache_name);
                        }
                    }
                }
            }
        }
    }

    Ok(relationships)
}

/// Parse cache outputs using proper TryConvertNode with jinja context
fn parse_cache_outputs_with_context(
    outputs: &[Node],
    jinja: &crate::recipe::Jinja,
) -> Result<Vec<crate::recipe::parser::CacheOutput>, ParsingError<&'static str>> {
    use super::output_parser::OutputType;
    let mut cache_outputs = Vec::new();

    for output in outputs {
        let rendered_node: RenderedNode = output.render(jinja, "output").map_err(|e| {
            ParsingError::from_partial_vec("", e)
                .into_iter()
                .next()
                .unwrap_or_else(|| {
                    ParsingError::from_partial(
                        "",
                        _partialerror!(
                            marked_yaml::Span::new_blank(),
                            ErrorKind::ExpectedMapping,
                            help = "Failed to render output for inheritance resolution"
                        ),
                    )
                })
        })?;

        match rendered_node.try_convert("output") {
            Ok(OutputType::Cache(cache)) => {
                cache_outputs.push(*cache);
            }
            Ok(OutputType::Package(_)) => {
                continue;
            }
            Err(_) => {
                continue;
            }
        }
    }

    Ok(cache_outputs)
}

/// Apply cache inheritance to package outputs during parsing
fn apply_inheritance_to_outputs(
    outputs: &[Node],
    cache_outputs: &[crate::recipe::parser::CacheOutput],
    inheritance_relationships: &std::collections::HashMap<String, Vec<String>>,
    jinja: &crate::recipe::Jinja,
) -> Result<Vec<Node>, ParsingError<&'static str>> {
    use super::output_parser::OutputType;
    let mut resolved_outputs = Vec::new();

    for output in outputs {
        let rendered_node: RenderedNode = output.render(jinja, "output").map_err(|e| {
            ParsingError::from_partial_vec("", e)
                .into_iter()
                .next()
                .unwrap_or_else(|| {
                    ParsingError::from_partial(
                        "",
                        _partialerror!(
                            marked_yaml::Span::new_blank(),
                            ErrorKind::ExpectedMapping,
                            help = "Failed to render output for inheritance resolution"
                        ),
                    )
                })
        })?;

        match rendered_node.try_convert("output") {
            Ok(OutputType::Package(mut package_output)) => {
                let package_name = package_output.package.name().as_normalized().to_string();
                if let Some(cache_names) = inheritance_relationships.get(&package_name) {
                    for cache_name in cache_names {
                        if let Some(cache_output) =
                            cache_outputs.iter().find(|c| &c.name == cache_name)
                        {
                            package_output.apply_cache_inheritance(cache_output);
                        }
                    }
                }
                let rendered = serde_yaml::to_value(&package_output)
                    .map_err(|err| ParsingError::from_partial(
                        "",
                        _partialerror!(
                            marked_yaml::Span::new_blank(),
                            ErrorKind::Other,
                            label = format!("Failed to serialize inherited output: {}", err)
                        ),
                    ))?;
                let node = Node::try_from(rendered)
                    .map_err(|err| ParsingError::from_partial("", err))?;
                resolved_outputs.push(node);
            }
            Ok(OutputType::Cache(_)) => {
                resolved_outputs.push(output.clone());
            }
            Err(_) => {
                resolved_outputs.push(output.clone());
            }
        }
    }

    Ok(resolved_outputs)
}

/// Resolve cache inheritance and return both outputs and cache outputs
///
/// This function validates inheritance and collects cache outputs for use in Output creation.
/// It uses the proper TryConvertNode for full parsing.
pub fn resolve_cache_inheritance_with_caches(
    outputs: Vec<Node>,
    has_toplevel_cache: bool,
    experimental_enabled: bool,
    jinja: &crate::recipe::Jinja,
) -> CacheInheritanceResult {
    if !experimental_enabled {
        for output in &outputs {
            if let Some(mapping) = output.as_mapping() {
                if let Some(cache_node) = mapping.get("cache") {
                    return Err(ParsingError::from_partial(
                        "",
                        _partialerror!(
                            *cache_node.span(),
                            ErrorKind::ExperimentalOnly("cache outputs".to_string()),
                            help = "Cache outputs require enabling experimental mode (`--experimental`)"
                        ),
                    ));
                }

                if let Some(package_node) = mapping.get("package") {
                    if let Some(package_mapping) = package_node.as_mapping() {
                        if let Some(inherit_node) = package_mapping.get("inherit") {
                            return Err(ParsingError::from_partial(
                                "",
                                _partialerror!(
                                    *inherit_node.span(),
                                    ErrorKind::ExperimentalOnly("inherit".to_string()),
                                    help = "The `inherit` key requires enabling experimental mode (`--experimental`)"
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    let all_cache_outputs = parse_cache_outputs_with_context(&outputs, jinja)?;
    let inheritance_relationships = parse_inheritance_relationships(&outputs)?;
    let resolved_outputs = apply_inheritance_to_outputs(
        &outputs,
        &all_cache_outputs,
        &inheritance_relationships,
        jinja,
    )?;

    Ok((
        resolved_outputs,
        all_cache_outputs,
        inheritance_relationships,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assert_miette_snapshot,
        recipe::{Recipe, jinja::SelectorConfig},
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
