//! Output parsing is a bit more complicated than the other sections.
//!
//! The reason for this is that the `outputs` field is a list of mappings, and
//! each mapping can have its own `package`, `source`, `build`, `requirements`,
//! `test`, and `about` fields.

use marked_yaml::types::MarkedMappingNode;
use std::collections::{HashMap, HashSet};

use crate::{
    _partialerror,
    recipe::{
        ParsingError, Render,
        custom_yaml::{HasSpan, Node, RenderedNode, TryConvertNode, parse_yaml},
        error::{ErrorKind, PartialParsingError},
    },
    source_code::SourceCode,
};

use rattler_conda_types::PackageName;

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

// Extract inherit information
fn inherit_name_from(node: &Node) -> Option<String> {
    node.as_scalar()
        .map(|s| s.as_str().to_string())
        .or_else(|| {
            node.as_mapping()?
                .get("from")?
                .as_scalar()
                .map(|s| s.as_str().to_string())
        })
}

fn pkg_inherit_node(output: &Node) -> Option<&Node> {
    output
        .as_mapping()?
        .get("package")?
        .as_mapping()?
        .get("inherit")
}

fn cache_inherit_node(output: &Node) -> Option<&Node> {
    output
        .as_mapping()?
        .get("cache")?
        .as_mapping()?
        .get("inherit")
}

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
            Some(inherit_node)
                if inherit_node
                    .as_scalar()
                    .is_some_and(|s| s.as_str().is_empty()) => {}
            Some(_) => {}
            None => {
                // When inheriting from top-level, requirements and build.script are ignored (not forbidden)
                // rather than throwing errors, we just warn about them
                if let Some(_req_node) = output_map.get("requirements") {
                    tracing::warn!(
                        "When inheriting from top-level, the `requirements` field will be ignored. \
                         The output will inherit all requirements from the top-level recipe."
                    );
                }
                if let Some(build_node) = output_map.get("build") {
                    if let Some(build_map) = build_node.as_mapping() {
                        if build_map.contains_key("script") {
                            tracing::warn!(
                                "When inheriting from top-level (implicit inheritance), the `build.script` field will be ignored. \
                                 This is a breaking change from older versions where explicit script definitions in output were used. \
                                 The output will use the top-level recipe's build script instead."
                            );
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
    use std::collections::{HashMap, HashSet};

    // Collect cache names from outputs array
    let mut cache_names = HashSet::new();
    let mut duplicate_caches = Vec::new();
    let mut cache_name_spans = HashMap::new();
    let mut cache_inheritance = HashMap::new();

    for output in outputs.iter() {
        if let Some(cache_output) = parse_cache_output_from_node(output)? {
            let cache_name = cache_output.name.as_normalized().to_string();
            if !cache_names.insert(cache_name.clone()) {
                duplicate_caches.push(cache_name);
            } else {
                cache_name_spans.insert(
                    cache_output.name.as_normalized().to_string(),
                    cache_output.span,
                );
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

    // Validate package outputs inheriting from cache outputs
    for output in outputs.iter() {
        let Some(cache_name) = pkg_inherit_node(output).and_then(inherit_name_from) else {
            continue;
        };

        // Check if cache exists (in outputs array)
        // Note: Explicit inheritance takes precedence over top-level cache.
        // If an output explicitly inherits from a named cache, it must exist
        // in the outputs array.
        if !cache_names.contains(&cache_name) {
            let available: Vec<_> = cache_names.iter().map(|n| format!("'{}'", n)).collect();
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
                    ErrorKind::InvalidField(format!("cache '{}' not found", cache_name).into()),
                    help = help_msg
                ),
            ));
        }
    }

    for output in outputs.iter() {
        let Some(cache_mapping) = output
            .as_mapping()
            .and_then(|mapping| mapping.get("cache"))
            .and_then(|cache_node| cache_node.as_mapping())
        else {
            continue;
        };

        let Some(inherit_node) = cache_mapping.get("inherit") else {
            continue;
        };

        let Some(cache_name_to_inherit) = inherit_name_from(inherit_node) else {
            continue;
        };

        let Some(this_cache_name) = cache_mapping
            .get("name")
            .and_then(|name_node| name_node.as_scalar())
            .map(|name_scalar| name_scalar.as_str().to_string())
        else {
            continue;
        };

        // Cache outputs can only inherit from other cache outputs, not from package outputs
        // Check if the cache to inherit from exists in the cache names we collected
        if !cache_names.contains(&cache_name_to_inherit) {
            let available: Vec<_> = cache_names.iter().map(|n| format!("'{}'", n)).collect();
            let help_msg = if available.is_empty() {
                "No cache outputs defined".to_string()
            } else {
                format!("Available caches: {}", available.join(", "))
            };
            return Err(ParsingError::from_partial(
                "",
                _partialerror!(
                    *output.span(),
                    ErrorKind::InvalidField(
                        format!(
                            "cache '{}' not found for cache inheritance",
                            cache_name_to_inherit
                        )
                        .into()
                    ),
                    help = help_msg
                ),
            ));
        }

        // Store the inheritance relationship for cycle detection
        cache_inheritance
            .entry(this_cache_name.clone())
            .or_insert_with(Vec::new)
            .push(cache_name_to_inherit.clone());
    }

    // Detect cycles in cache-to-cache inheritance
    if let Some(cycle) = detect_cache_inheritance_cycles(&cache_inheritance) {
        return Err(ParsingError::from_partial(
            "",
            _partialerror!(
                marked_yaml::Span::new_blank(),
                ErrorKind::InvalidField(
                    format!("cache inheritance cycle detected: {}", cycle.join(" -> ")).into()
                ),
                help = "Cache outputs cannot form inheritance cycles"
            ),
        ));
    }

    Ok(outputs)
}

/// Detect cycles in cache inheritance relationships using DFS
fn detect_cache_inheritance_cycles(
    inheritance: &HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    let mut visited = std::collections::HashMap::new();
    let mut recursion_stack = std::collections::HashMap::new();
    let mut parent = std::collections::HashMap::new();

    for cache_name in inheritance.keys() {
        if !(*visited.get(cache_name).unwrap_or(&false)) {
            let path = dfs_visit(
                cache_name,
                inheritance,
                &mut visited,
                &mut recursion_stack,
                &mut parent,
            );
            if let Some(cycle_path) = path {
                return Some(cycle_path);
            }
        }
    }
    None
}

/// Depth-first search to find cycles
fn dfs_visit(
    cache: &str,
    inheritance: &HashMap<String, Vec<String>>,
    visited: &mut std::collections::HashMap<String, bool>,
    recursion_stack: &mut std::collections::HashMap<String, bool>,
    parent: &mut std::collections::HashMap<String, String>,
) -> Option<Vec<String>> {
    visited.insert(cache.to_string(), true);
    recursion_stack.insert(cache.to_string(), true);

    if let Some(dependencies) = inheritance.get(cache) {
        for dependency in dependencies {
            if !(*visited.get(dependency).unwrap_or(&false)) {
                parent.insert(dependency.clone(), cache.to_string());
                if let Some(cycle) =
                    dfs_visit(dependency, inheritance, visited, recursion_stack, parent)
                {
                    return Some(cycle);
                }
            } else if *recursion_stack.get(dependency).unwrap_or(&false) {
                let mut cycle = vec![dependency.clone()];
                let mut current = cache.to_string();

                while current != *dependency {
                    cycle.push(current.clone());
                    if let Some(parent_cache) = parent.get(&current) {
                        current = parent_cache.clone();
                    } else {
                        break;
                    }
                }
                cycle.push(dependency.clone());
                cycle.reverse();

                return Some(cycle);
            }
        }
    }

    recursion_stack.insert(cache.to_string(), false);
    None
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
                            match PackageName::try_from(name_scalar.as_str().to_string()) {
                                Ok(package_name) => package_name,
                                Err(err) => {
                                    return Err(ParsingError::from_partial(
                                        "",
                                        _partialerror!(
                                            *name_node.span(),
                                            ErrorKind::from(err),
                                            help = "cache name must be a valid package name"
                                        ),
                                    ));
                                }
                            }
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
                        match PackageName::try_from("default".to_string()) {
                            Ok(package_name) => package_name,
                            Err(_) => {
                                return Err(ParsingError::from_partial(
                                    "",
                                    _partialerror!(
                                        marked_yaml::Span::new_blank(),
                                        ErrorKind::InvalidField("default".to_string().into()),
                                        help = "internal error: default cache name is invalid"
                                    ),
                                ));
                            }
                        }
                    };

                    // For now, create a basic cache output with defaults
                    return Ok(Some(crate::recipe::parser::CacheOutput {
                        name,
                        source: Vec::new(),
                        build: crate::recipe::parser::CacheBuild::default(),
                        requirements: crate::recipe::parser::CacheRequirements::default(),
                        run_exports: crate::recipe::parser::RunExports::default(),
                        ignore_run_exports: None,
                        about: None,
                        span: *output.span(),
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
        let Some(mapping) = output.as_mapping() else {
            continue;
        };

        let Some(package_node) = mapping.get("package") else {
            continue;
        };

        let Some(package_mapping) = package_node.as_mapping() else {
            continue;
        };
        let Some(name_node) = package_mapping.get("name") else {
            continue;
        };
        let Some(name_scalar) = name_node.as_scalar() else {
            continue;
        };
        let package_name = name_scalar.as_str().to_string();

        let Some(inherit_node) = package_mapping
            .get("inherit")
            .or_else(|| mapping.get("inherit"))
        else {
            continue;
        };

        let cache_name = if let Some(inherit_scalar) = inherit_node.as_scalar() {
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

/// Parse cache-to-cache inheritance relationships from outputs
fn parse_cache_to_cache_inheritance(
    outputs: &[Node],
) -> Result<HashMap<String, Vec<String>>, ParsingError<&'static str>> {
    let mut relationships: HashMap<String, Vec<String>> = HashMap::new();

    for output in outputs {
        let Some(mapping) = output.as_mapping() else {
            continue;
        };
        let Some(cache_node) = mapping.get("cache") else {
            continue;
        };
        let Some(cache_mapping) = cache_node.as_mapping() else {
            continue;
        };

        let cache_name = cache_mapping
            .get("name")
            .and_then(|n| n.as_scalar())
            .map(|s| s.as_str().to_string());

        let inherit_node = mapping
            .get("inherit")
            .or_else(|| cache_mapping.get("inherit"));

        let inherited_name = inherit_node.and_then(|inherit| {
            inherit
                .as_scalar()
                .map(|s| s.as_str().to_string())
                .or_else(|| {
                    inherit
                        .as_mapping()
                        .and_then(|m| m.get("from"))
                        .and_then(|f| f.as_scalar())
                        .map(|s| s.as_str().to_string())
                })
        });

        if let (Some(cache_name), Some(inherited_name)) = (cache_name, inherited_name) {
            relationships
                .entry(cache_name)
                .or_default()
                .push(inherited_name);
        }
    }

    Ok(relationships)
}

/// Topological sort to process caches in dependency order (ancestors before descendants)
fn topological_sort(inheritance: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut in_degree: HashMap<String, usize> = inheritance
        .iter()
        .map(|(node, ancestors)| (node.clone(), ancestors.len()))
        .collect();

    let all_nodes: HashSet<_> = inheritance
        .keys()
        .chain(inheritance.values().flat_map(|v| v.iter()))
        .cloned()
        .collect();

    let mut queue: Vec<_> = all_nodes
        .iter()
        .filter(|node| in_degree.get(*node).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();

    let mut result = Vec::new();
    while let Some(node) = queue.pop() {
        result.push(node.clone());
        // Decrease in-degree for dependents of this node
        for (dependent, _) in inheritance.iter().filter(|(_, and)| and.contains(&node)) {
            let degree = in_degree.get_mut(dependent).unwrap();
            *degree -= 1;
            if *degree == 0 {
                queue.push(dependent.clone());
            }
        }
    }
    result
}

/// Apply cache-to-cache inheritance to the cache outputs
fn apply_cache_to_cache_inheritance(
    cache_outputs: Vec<crate::recipe::parser::CacheOutput>,
    cache_inheritance_relationships: HashMap<String, Vec<String>>,
) -> Vec<crate::recipe::parser::CacheOutput> {
    let mut cache_map: HashMap<String, crate::recipe::parser::CacheOutput> = cache_outputs
        .into_iter()
        .map(|cache| (cache.name.as_normalized().to_string(), cache))
        .collect();

    // Compute topological order to process dependencies first
    let processing_order = topological_sort(&cache_inheritance_relationships);

    for cache_name in processing_order {
        let Some(ancestors) = cache_inheritance_relationships.get(&cache_name) else {
            continue;
        };
        let ancestor_data: Vec<_> = ancestors
            .iter()
            .filter_map(|ancestor_name| {
                cache_map.get(ancestor_name).map(|ancestor_cache| {
                    (
                        ancestor_cache.run_exports.clone(),
                        ancestor_cache.about.clone(),
                        ancestor_cache.ignore_run_exports.clone(),
                        ancestor_cache.requirements.build.clone(),
                        ancestor_cache.requirements.host.clone(),
                    )
                })
            })
            .collect();

        if let Some(target_cache) = cache_map.get_mut(&cache_name) {
            for (run_exports, about, ignore_run_exports, build_reqs, host_reqs) in ancestor_data {
                // Apply inheritance from ancestor cache
                target_cache.run_exports.extend_from(&run_exports);

                // Inherit about section
                if let Some(ancestor_about) = about {
                    target_cache
                        .about
                        .get_or_insert_with(|| ancestor_about.clone())
                        .merge_from(&ancestor_about);
                }

                // Inherit ignore_run_exports
                if let Some(ancestor_ignore) = ignore_run_exports {
                    target_cache
                        .ignore_run_exports
                        .get_or_insert_with(|| ancestor_ignore.clone())
                        .merge_from(&ancestor_ignore);
                }

                // Inherit requirements (build and host)
                target_cache.requirements.build.extend(build_reqs);
                target_cache.requirements.host.extend(host_reqs);
            }
        }
    }

    cache_map.into_values().collect()
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

    // Parse caches with context first
    let all_cache_outputs = parse_cache_outputs_with_context(&outputs, jinja)?;

    // Duplicate detection using a set (no nesting)
    let mut seen = HashSet::new();
    let mut duplicates = Vec::new();
    for name in all_cache_outputs
        .iter()
        .map(|c| c.name.as_normalized().to_string())
    {
        if !seen.insert(name.clone()) {
            duplicates.push(name);
        }
    }
    if !duplicates.is_empty() {
        return Err(ParsingError::from_partial(
            "",
            _partialerror!(
                marked_yaml::Span::new_blank(),
                ErrorKind::InvalidField(
                    format!("duplicate cache names: {}", duplicates.join(", ")).into()
                ),
                help = "Each cache output must have a unique name"
            ),
        ));
    }
    let cache_names: HashSet<_> = seen;

    // Validate package -> cache inheritance using iterator chains
    if let Some((bad_output, bad_name)) = outputs
        .iter()
        .filter_map(|o| pkg_inherit_node(o).map(|n| (o, n)))
        .filter_map(|(o, n)| inherit_name_from(n).map(|name| (o, name)))
        .find(|(_, name)| !cache_names.contains(name))
    {
        let help_msg = if cache_names.is_empty() {
            if has_toplevel_cache {
                "No cache outputs defined in outputs array. To use top-level cache, omit the 'inherit' key.".to_string()
            } else {
                "No cache outputs defined".to_string()
            }
        } else {
            format!(
                "Available caches: {}",
                cache_names
                    .iter()
                    .map(|n| format!("'{}'", n))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        return Err(ParsingError::from_partial(
            "",
            _partialerror!(
                *bad_output.span(),
                ErrorKind::InvalidField(format!("cache '{}' not found", bad_name).into()),
                help = help_msg
            ),
        ));
    }

    // Validate cache -> cache inheritance using iterator chains
    if let Some((bad_output, bad_name)) = outputs
        .iter()
        .filter_map(|o| cache_inherit_node(o).map(|n| (o, n)))
        .filter_map(|(o, n)| inherit_name_from(n).map(|name| (o, name)))
        .find(|(_, name)| !cache_names.contains(name))
    {
        let help_msg = format!(
            "Available caches: {}",
            cache_names
                .iter()
                .map(|n| format!("'{}'", n))
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Err(ParsingError::from_partial(
            "",
            _partialerror!(
                *bad_output.span(),
                ErrorKind::InvalidField(
                    format!("cache '{}' not found for cache inheritance", bad_name).into()
                ),
                help = help_msg
            ),
        ));
    }

    // Cache-to-cache graph + application
    let cache_to_cache_inheritance_relationships = parse_cache_to_cache_inheritance(&outputs)?;
    let inherited_cache_outputs = apply_cache_to_cache_inheritance(
        all_cache_outputs,
        cache_to_cache_inheritance_relationships.clone(),
    );

    let package_outputs: Vec<Node> = outputs
        .into_iter()
        .filter(|output| output.as_mapping().and_then(|m| m.get("package")).is_some())
        .collect();

    let mut inheritance_relationships = parse_inheritance_relationships(&package_outputs)?;

    // Merge cache-to-cache relationships into the main inheritance map
    for (cache_name, parent_caches) in cache_to_cache_inheritance_relationships {
        inheritance_relationships
            .entry(cache_name)
            .or_default()
            .extend(parent_caches);
    }

    Ok((
        package_outputs,
        inherited_cache_outputs,
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
