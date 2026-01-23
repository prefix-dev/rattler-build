use marked_yaml::{Node, types::MarkedMappingNode};
use rattler_build_yaml_parser::ParseError;
use rattler_conda_types::NoArchType;

use crate::stage0::{
    Conditional, ConditionalList, Item, JinjaExpression, NestedItemList,
    build::{
        BinaryRelocation, Build, DynamicLinking, ForceFileType, PostProcess, PrefixDetection,
        PrefixIgnore, PythonBuild, VariantKeyUsage,
    },
    parser::helpers::get_span,
    types::{IncludeExclude, Value},
};
use rattler_build_yaml_parser::{
    helpers::contains_jinja_template, parse_conditional_list, parse_conditional_list_or_item,
    parse_value_with_name,
};

/// Macro to parse a value with automatic field name inference for better error messages
///
/// Usage: `parse_field!("field_name", node)` or `parse_field!("parent.field", node)`
/// This will automatically use the field name in error messages
macro_rules! parse_field {
    ($field:literal, $node:expr) => {{ parse_value_with_name($node, $field)? }};
}

/// Parse a boolean value from a YAML scalar node
///
/// Supports: true, True, yes, Yes, false, False, no, No
#[allow(dead_code)]
fn parse_bool(node: &Node, field_name: &str) -> Result<bool, ParseError> {
    let scalar = node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(node))
            .with_message(format!("Expected '{}' to be a boolean", field_name))
    })?;

    scalar.as_bool().ok_or_else(|| {
        ParseError::invalid_value(
            field_name,
            "expected boolean ('true', 'false')",
            *scalar.span(),
        )
    })
}

/// Parse a boolean value that can also be a Jinja template
///
/// Supports: true/false literals, or Jinja templates like `${{ false if osx else true }}`
fn parse_bool_value(node: &Node, field_name: &str) -> Result<Value<bool>, ParseError> {
    let scalar = node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(node)).with_message(format!(
            "Expected '{}' to be a boolean or Jinja template",
            field_name
        ))
    })?;

    // First check if it's a plain boolean
    if let Some(bool_val) = scalar.as_bool() {
        return Ok(Value::new_concrete(bool_val, Some(*scalar.span())));
    }

    // Check if it's a Jinja template
    let str_val = scalar.as_str();
    if str_val.contains("${{") {
        let template = crate::stage0::types::JinjaTemplate::new(str_val.to_string())
            .map_err(|e| ParseError::jinja_error(e, *scalar.span()))?;
        return Ok(Value::new_template(template, Some(*scalar.span())));
    }

    // Not a boolean or template - return error
    Err(ParseError::invalid_value(
        field_name,
        format!(
            "expected boolean ('true', 'false') or Jinja template (${{{{ ... }}}}), got '{}'",
            str_val
        ),
        *scalar.span(),
    ))
}

/// Parse a field that can be either a boolean or a list of patterns
///
/// This is used for fields like `binary_relocation` and `prefix_detection.ignore`
/// that support both `true`/`false` and list of glob patterns.
/// Also supports Jinja templates like `${{ false if osx else true }}`.
///
/// Returns an enum with either Boolean(Value<bool>) or Patterns(ConditionalList<String>)
fn parse_bool_or_patterns<T>(
    node: &Node,
    field_name: &str,
    bool_variant: fn(Value<bool>) -> T,
    patterns_variant: fn(crate::stage0::types::ConditionalList<String>) -> T,
) -> Result<T, ParseError> {
    // Try to parse as a scalar (boolean or template)
    if let Some(scalar) = node.as_scalar() {
        // First check if it's a plain boolean
        if let Some(bool_val) = scalar.as_bool() {
            let value = Value::new_concrete(bool_val, Some(*node.span()));
            return Ok(bool_variant(value));
        }

        // Check if it's a Jinja template
        let str_val = scalar.as_str();
        if str_val.contains("${{") {
            let template = crate::stage0::types::JinjaTemplate::new(str_val.to_string())
                .map_err(|e| ParseError::jinja_error(e, *scalar.span()))?;
            return Ok(bool_variant(Value::new_template(
                template,
                Some(*scalar.span()),
            )));
        }

        // Not a boolean or template - return error
        return Err(ParseError::expected_type(
            "boolean or Jinja template",
            "string",
            get_span(node),
        )
        .with_message(format!(
            "Expected '{}' to be a boolean (true/false) or Jinja template (${{{{ ... }}}})",
            field_name
        )));
    }

    // Try to parse as a list of patterns
    if node.as_sequence().is_some() {
        return Ok(patterns_variant(parse_conditional_list(node)?));
    }

    Err(ParseError::expected_type(
        "boolean or list",
        "invalid type",
        get_span(node),
    ))
}

/// Parse a noarch field from YAML
///
/// Noarch can be either:
/// - A scalar string: "python" or "generic"
/// - A template: "${{ noarch_type }}"
fn parse_noarch(node: &Node) -> Result<Value<NoArchType>, ParseError> {
    let scalar = node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(node))
            .with_message("Expected 'noarch' to be a string (\"python\" or \"generic\")")
    })?;

    let str_val = scalar.as_str();

    // Check if it's a template
    if str_val.contains("${{") {
        let template = crate::stage0::types::JinjaTemplate::new(str_val.to_string())
            .map_err(|e| ParseError::jinja_error(e, *scalar.span()))?;
        return Ok(Value::new_template(template, Some(*scalar.span())));
    }

    // Parse as concrete NoArchType
    let noarch = match str_val {
        "python" => NoArchType::python(),
        "generic" => NoArchType::generic(),
        _ => {
            return Err(ParseError::invalid_value(
                "noarch",
                format!(
                    "invalid noarch type '{}'. Expected 'python' or 'generic'",
                    str_val
                ),
                *scalar.span(),
            ));
        }
    };

    Ok(Value::new_concrete(noarch, Some(*scalar.span())))
}

/// Parse a script field from YAML
///
/// Script can be either:
/// - A sequence of strings: `["echo hello", "make install"]`
/// - A scalar multiline string: `|`
///   `echo hello`
///   `make install`
/// - A single script object mapping: `{env: {...}, content: [...]}`
///
/// For scalar strings, we split by newlines and filter out empty lines
pub(crate) fn parse_script(node: &Node) -> Result<crate::stage0::types::Script, ParseError> {
    use crate::stage0::types::{ConditionalList, Item, Script, Value};

    // Try parsing as a scalar string (multiline or single line) - simple case
    if let Some(scalar) = node.as_scalar() {
        let script_str = scalar.as_str();
        let span = *scalar.span();

        // Check if it's a template
        if contains_jinja_template(script_str) {
            // It's a templated script - keep as is
            let template = crate::stage0::types::JinjaTemplate::new(script_str.to_string())
                .map_err(|e| ParseError::jinja_error(e, span))?;
            let items = vec![Item::Value(Value::new_template(template, Some(span)))];
            return Ok(Script {
                content: Some(ConditionalList::new(items)),
                ..Default::default()
            });
        }

        // Keep the entire multiline string as a single item to preserve multiline formatting
        let items = vec![Item::Value(Value::new_concrete(
            script_str.to_string(),
            Some(span),
        ))];

        return Ok(Script {
            content: Some(ConditionalList::new(items)),
            ..Default::default()
        });
    }

    // Try parsing as a sequence - simple list of commands
    if node.as_sequence().is_some() {
        // TODO this should be a list of Value?
        let content = parse_conditional_list(node)?;
        return Ok(Script {
            content: Some(content),
            ..Default::default()
        });
    }

    // Try parsing as a full Script mapping with interpreter, env, content, etc.
    if let Some(mapping) = node.as_mapping() {
        // Parse as a Script object
        let mut interpreter = None;
        let mut env = indexmap::IndexMap::new();
        let mut secrets = Vec::new();
        let mut content = None;
        let mut file = None;
        let mut cwd = None;
        let mut content_explicit = false;

        for (key_node, value_node) in mapping.iter() {
            let key = key_node.as_str();

            match key {
                "interpreter" => {
                    interpreter = Some(parse_field!("script.interpreter", value_node));
                }
                "env" => {
                    let env_mapping = value_node.as_mapping().ok_or_else(|| {
                        ParseError::expected_type("mapping", "non-mapping", get_span(value_node))
                            .with_message("Expected 'env' to be a mapping")
                    })?;

                    for (env_key_node, env_value_node) in env_mapping.iter() {
                        let env_key = env_key_node.as_str().to_string();
                        let env_value = parse_field!("script.env", env_value_node);
                        env.insert(env_key, env_value);
                    }
                }
                "secrets" => {
                    let seq = value_node.as_sequence().ok_or_else(|| {
                        ParseError::expected_type("sequence", "non-sequence", get_span(value_node))
                            .with_message("Expected 'secrets' to be a list")
                    })?;

                    for item in seq.iter() {
                        let scalar = item.as_scalar().ok_or_else(|| {
                            ParseError::expected_type("string", "non-string", get_span(item))
                                .with_message("Expected secret name to be a string")
                        })?;
                        secrets.push(scalar.as_str().to_string());
                    }
                }
                "content" => {
                    // Mark that content was explicitly specified via `content:` field
                    content_explicit = true;
                    // Content can be either a string or a list
                    if let Some(scalar) = value_node.as_scalar() {
                        // Single string - convert to ConditionalList with one item
                        let content_str = scalar.as_str();
                        let span = *scalar.span();

                        // Check if it's a template
                        if contains_jinja_template(content_str) {
                            let template =
                                crate::stage0::types::JinjaTemplate::new(content_str.to_string())
                                    .map_err(|e| ParseError::jinja_error(e, span))?;
                            content = Some(crate::stage0::types::ConditionalList::new(vec![
                                crate::stage0::types::Item::Value(Value::new_template(
                                    template,
                                    Some(span),
                                )),
                            ]));
                        } else {
                            // Keep the entire multiline string as a single item to preserve multiline formatting
                            content = Some(crate::stage0::types::ConditionalList::new(vec![
                                crate::stage0::types::Item::Value(Value::new_concrete(
                                    content_str.to_string(),
                                    Some(span),
                                )),
                            ]));
                        }
                    } else {
                        // Parse as a list (with possible conditionals)
                        content = Some(parse_conditional_list(value_node)?);
                    }
                }
                "file" => {
                    file = Some(parse_field!("script.file", value_node));
                }
                "cwd" => {
                    cwd = Some(parse_field!("script.cwd", value_node));
                }
                _ => {
                    return Err(ParseError::invalid_value(
                        "script",
                        format!("unknown field '{}' in script object", key),
                        *key_node.span(),
                    )
                    .with_suggestion(
                        "Valid fields are: interpreter, env, secrets, content, file, cwd",
                    ));
                }
            }
        }

        return Ok(Script {
            interpreter,
            env,
            secrets,
            content,
            file,
            cwd,
            content_explicit,
        });
    }

    Err(ParseError::expected_type(
        "sequence, scalar string, or script object",
        "other",
        get_span(node),
    )
    .with_message(
        "script must be either a list of commands, a multiline string, or a script object",
    ))
}

/// Parse build files field - can be a list or include/exclude mapping
fn parse_build_files(node: &Node) -> Result<IncludeExclude, ParseError> {
    // Try parsing as a mapping with include/exclude first
    if let Some(mapping) = node.as_mapping() {
        let mut include = None;
        let mut exclude = None;

        for (key_node, value_node) in mapping.iter() {
            let key = key_node.as_str();

            match key {
                "include" => {
                    include = Some(parse_conditional_list(value_node)?);
                }
                "exclude" => {
                    exclude = Some(parse_conditional_list(value_node)?);
                }
                _ => {
                    return Err(ParseError::invalid_value(
                        "files",
                        format!("unknown field '{}' in files mapping", key),
                        *key_node.span(),
                    )
                    .with_suggestion("Valid fields are: include, exclude"));
                }
            }
        }

        return Ok(IncludeExclude::Mapping {
            include: include.unwrap_or_default(),
            exclude: exclude.unwrap_or_default(),
        });
    }

    // Otherwise parse as a simple list
    if node.as_sequence().is_some() {
        return Ok(IncludeExclude::List(parse_conditional_list(node)?));
    }

    Err(ParseError::expected_type(
        "sequence or mapping with include/exclude",
        "other",
        get_span(node),
    )
    .with_message(
        "files must be either a list of glob patterns or a mapping with include/exclude keys",
    ))
}

/// Parse a Build section from YAML
pub fn parse_build(node: &Node) -> Result<Build, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected 'build' to be a mapping")
    })?;

    parse_build_from_mapping(mapping)
}

fn parse_build_from_mapping(mapping: &MarkedMappingNode) -> Result<Build, ParseError> {
    let mut build = Build::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "number" => {
                build.number = Some(parse_field!("build.number", value_node));
            }
            "string" => {
                build.string = Some(parse_field!("build.string", value_node));
            }
            "script" => {
                build.script = parse_script(value_node)?;
            }
            "noarch" => {
                build.noarch = Some(parse_noarch(value_node)?);
            }
            "python" => {
                build.python = parse_python_build(value_node)?;
            }
            "skip" => {
                // Skip accepts both a single value (e.g., "win") or a list
                build.skip = parse_conditional_list_or_item(value_node)?.into();
            }
            "always_copy_files" => {
                build.always_copy_files = parse_conditional_list(value_node)?;
            }
            "always_include_files" => {
                build.always_include_files = parse_conditional_list(value_node)?;
            }
            "merge_build_and_host_envs" => {
                build.merge_build_and_host_envs =
                    parse_bool_value(value_node, "merge_build_and_host_envs")?;
            }
            "files" => {
                build.files = parse_build_files(value_node)?;
            }
            "dynamic_linking" => {
                build.dynamic_linking = parse_dynamic_linking(value_node)?;
            }
            "variant" => {
                build.variant = parse_variant_key_usage(value_node)?;
            }
            "prefix_detection" => {
                build.prefix_detection = parse_prefix_detection(value_node)?;
            }
            "post_process" => {
                build.post_process = parse_post_process_list(value_node)?;
            }
            _ => {
                return Err(
                    ParseError::invalid_value("build", format!("unknown field '{}'", key), *key_node.span())
                        .with_suggestion("Valid fields are: number, string, script, noarch, python, skip, always_copy_files, always_include_files, merge_build_and_host_envs, files, dynamic_linking, variant, prefix_detection, post_process")
                );
            }
        }
    }

    Ok(build)
}

fn parse_binary_relocation(node: &Node) -> Result<BinaryRelocation, ParseError> {
    parse_bool_or_patterns(
        node,
        "binary_relocation",
        BinaryRelocation::Boolean,
        BinaryRelocation::Patterns,
    )
}

fn parse_dynamic_linking(node: &Node) -> Result<DynamicLinking, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected 'dynamic_linking' to be a mapping")
    })?;

    let mut dynamic_linking = DynamicLinking::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "rpaths" => {
                dynamic_linking.rpaths = parse_conditional_list(value_node)?;
            }
            "binary_relocation" => {
                dynamic_linking.binary_relocation = parse_binary_relocation(value_node)?;
            }
            "missing_dso_allowlist" => {
                dynamic_linking.missing_dso_allowlist = parse_conditional_list(value_node)?;
            }
            "rpath_allowlist" => {
                dynamic_linking.rpath_allowlist = parse_conditional_list(value_node)?;
            }
            "overdepending_behavior" => {
                dynamic_linking.overdepending_behavior = Some(parse_field!(
                    "dynamic_linking.overdepending_behavior",
                    value_node
                ));
            }
            "overlinking_behavior" => {
                dynamic_linking.overlinking_behavior = Some(parse_field!(
                    "dynamic_linking.overlinking_behavior",
                    value_node
                ));
            }
            _ => {
                return Err(
                    ParseError::invalid_value("dynamic_linking", format!("unknown field '{}'", key), *key_node.span())
                        .with_suggestion("Valid fields are: rpaths, binary_relocation, missing_dso_allowlist, rpath_allowlist, overdepending_behavior, overlinking_behavior")
                );
            }
        }
    }

    Ok(dynamic_linking)
}

fn parse_python_build(node: &Node) -> Result<PythonBuild, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected 'python' to be a mapping")
    })?;

    let mut python = PythonBuild::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "entry_points" => {
                python.entry_points = parse_conditional_list(value_node)?;
            }
            "skip_pyc_compilation" => {
                python.skip_pyc_compilation = parse_conditional_list(value_node)?;
            }
            "use_python_app_entrypoint" => {
                python.use_python_app_entrypoint =
                    parse_bool_value(value_node, "use_python_app_entrypoint")?;
            }
            "version_independent" => {
                python.version_independent =
                    Some(parse_field!("python.version_independent", value_node));
            }
            "site_packages_path" => {
                python.site_packages_path =
                    Some(parse_field!("python.site_packages_path", value_node));
            }
            _ => {
                return Err(
                    ParseError::invalid_value("python", format!("unknown field '{}'", key), *key_node.span())
                        .with_suggestion("Valid fields are: entry_points, skip_pyc_compilation, use_python_app_entrypoint, version_independent, site_packages_path")
                );
            }
        }
    }

    Ok(python)
}

fn parse_variant_key_usage(node: &Node) -> Result<VariantKeyUsage, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected 'variant' to be a mapping")
    })?;

    let mut variant = VariantKeyUsage::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "use_keys" => {
                variant.use_keys = parse_conditional_list(value_node)?;
            }
            "ignore_keys" => {
                variant.ignore_keys = parse_conditional_list(value_node)?;
            }
            "down_prioritize_variant" => {
                variant.down_prioritize_variant =
                    Some(parse_field!("variant.down_prioritize_variant", value_node));
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "variant",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion(
                    "Valid fields are: use_keys, ignore_keys, down_prioritize_variant",
                ));
            }
        }
    }

    Ok(variant)
}

fn parse_force_file_type(node: &Node) -> Result<ForceFileType, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected 'force_file_type' to be a mapping")
    })?;

    let mut force_file_type = ForceFileType::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "text" => {
                force_file_type.text = parse_conditional_list(value_node)?;
            }
            "binary" => {
                force_file_type.binary = parse_conditional_list(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "force_file_type",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion("Valid fields are: text, binary"));
            }
        }
    }

    Ok(force_file_type)
}

fn parse_prefix_ignore(node: &Node) -> Result<PrefixIgnore, ParseError> {
    parse_bool_or_patterns(
        node,
        "prefix_detection.ignore",
        PrefixIgnore::Boolean,
        PrefixIgnore::Patterns,
    )
}

fn parse_prefix_detection(node: &Node) -> Result<PrefixDetection, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected 'prefix_detection' to be a mapping")
    })?;

    let mut prefix_detection = PrefixDetection::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "force_file_type" => {
                prefix_detection.force_file_type = parse_force_file_type(value_node)?;
            }
            "ignore" => {
                prefix_detection.ignore = parse_prefix_ignore(value_node)?;
            }
            "ignore_binary_files" => {
                prefix_detection.ignore_binary_files =
                    parse_bool_value(value_node, "ignore_binary_files")?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "prefix_detection",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion(
                    "Valid fields are: force_file_type, ignore, ignore_binary_files",
                ));
            }
        }
    }

    Ok(prefix_detection)
}

fn parse_post_process(node: &Node) -> Result<PostProcess, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Expected post-process item to be a mapping")
    })?;

    let mut files = None;
    let mut regex = None;
    let mut replacement = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "files" => {
                files = Some(parse_conditional_list(value_node)?);
            }
            "regex" => {
                regex = Some(parse_field!("post_process.regex", value_node));
            }
            "replacement" => {
                replacement = Some(parse_field!("post_process.replacement", value_node));
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "post_process",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion("Valid fields are: files, regex, replacement"));
            }
        }
    }

    // Ensure all required fields are present
    let files = files.ok_or_else(|| ParseError::missing_field("files", get_span(node)))?;
    let regex = regex.ok_or_else(|| ParseError::missing_field("regex", get_span(node)))?;
    let replacement =
        replacement.ok_or_else(|| ParseError::missing_field("replacement", get_span(node)))?;

    Ok(PostProcess {
        files,
        regex,
        replacement,
    })
}

/// Parse post_process list section from YAML (expects a sequence)
/// Returns a ConditionalList<PostProcess> which supports if/then/else conditionals
fn parse_post_process_list(node: &Node) -> Result<ConditionalList<PostProcess>, ParseError> {
    let sequence = node.as_sequence().ok_or_else(|| {
        ParseError::expected_type("sequence", "non-sequence", get_span(node))
            .with_message("Expected 'post_process' to be a list")
    })?;

    let mut items = Vec::new();
    for item in sequence.iter() {
        items.push(parse_post_process_item(item)?);
    }

    Ok(ConditionalList::new(items))
}

/// Parse a single post_process item which can be either a PostProcess or a conditional
fn parse_post_process_item(node: &Node) -> Result<Item<PostProcess>, ParseError> {
    // Check if it's a conditional (mapping with "if" key)
    if let Some(mapping) = node.as_mapping()
        && mapping.get("if").is_some()
    {
        return parse_conditional_post_process_item(mapping);
    }

    // Not a conditional - parse as a regular PostProcess
    let post_process = parse_post_process(node)?;
    Ok(Item::Value(Value::new_concrete(post_process, None)))
}

/// Parse a conditional post_process item with if/then/else branches
fn parse_conditional_post_process_item(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<Item<PostProcess>, ParseError> {
    let if_node = mapping
        .get("if")
        .ok_or_else(|| ParseError::missing_field("if", *mapping.span()))?;

    let condition_str = if_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(if_node))
            .with_message("'if' condition must be a string")
    })?;

    let condition_span = *condition_str.span();
    let condition = JinjaExpression::new(condition_str.as_str().to_string())
        .map_err(|e| ParseError::jinja_error(e, condition_span))?;

    let then_node = mapping
        .get("then")
        .ok_or_else(|| ParseError::missing_field("then", *mapping.span()))?;

    let then_items = parse_post_process_list_as_values(then_node)?;

    let else_items = if let Some(else_node) = mapping.get("else") {
        Some(parse_post_process_list_as_values(else_node)?)
    } else {
        None
    };

    Ok(Item::Conditional(Conditional {
        condition,
        then: then_items,
        else_value: else_items,
        condition_span: Some(condition_span),
    }))
}

/// Parse a post_process list from a sequence node (or a single post_process mapping)
/// Supports nested if/then/else conditionals
fn parse_post_process_list_as_values(
    node: &Node,
) -> Result<NestedItemList<PostProcess>, ParseError> {
    // If it's a sequence, parse each item as a post_process or conditional
    if let Some(seq) = node.as_sequence() {
        let mut items = Vec::new();
        for item_node in seq.iter() {
            items.push(parse_post_process_item(item_node)?);
        }
        Ok(NestedItemList::new(items))
    } else if node.as_mapping().is_some() {
        // Single post_process mapping - could be a post_process or a nested conditional
        let item = parse_post_process_item(node)?;
        Ok(NestedItemList::single(item))
    } else {
        Err(ParseError::expected_type(
            "sequence or mapping",
            "non-sequence/mapping",
            get_span(node),
        )
        .with_message(
            "'then' and 'else' must be sequences of post_process items or a single post_process",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseError;

    #[test]
    fn test_parse_empty_build() {
        let yaml = "{}";
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();
        // When number is not specified, it should be None (inherit from top-level)
        assert!(build.number.is_none());
        assert!(build.string.is_none());
        assert!(build.script.is_default());
    }

    #[test]
    fn test_parse_build_with_number() {
        let yaml = "number: 5";
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();
        if let Some(ref value) = build.number {
            if let Some(n) = value.as_concrete() {
                assert_eq!(*n, 5);
            } else {
                panic!("Expected concrete value");
            }
        } else {
            panic!("Expected Some(number)");
        }
    }

    #[test]
    fn test_parse_build_with_script() {
        let yaml = r#"
script:
  - echo "Building..."
  - make install
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();
        assert!(build.script.content.is_some());
        assert_eq!(build.script.content.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_parse_build_with_noarch() {
        let yaml = "noarch: python";
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();
        assert!(build.noarch.is_some());
    }

    #[test]
    fn test_parse_build_unknown_field() {
        let yaml = "unknown_field: value";
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let result = parse_build(&node);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Check that the error is about an invalid value (unknown field)
        assert!(matches!(err, ParseError::InvalidValue { .. }));
        let err_string = err.to_string();
        assert!(err_string.contains("unknown field"));
    }

    #[test]
    fn test_parse_post_process_conditional() {
        let yaml = r#"
post_process:
  - if: unix
    then:
      - files:
          - "*.txt"
        regex: "foo"
        replacement: "bar"
  - files:
      - "*.md"
    regex: "old"
    replacement: "new"
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();

        // Should have 2 items: one conditional and one direct
        assert_eq!(build.post_process.len(), 2);

        // First item should be a conditional
        let first = build.post_process.iter().next().unwrap();
        assert!(matches!(first, crate::stage0::Item::Conditional(_)));

        // Second item should be a direct value
        let second = build.post_process.iter().nth(1).unwrap();
        assert!(matches!(second, crate::stage0::Item::Value(_)));
    }

    #[test]
    fn test_parse_post_process_nested_conditional() {
        let yaml = r#"
post_process:
  - if: unix
    then:
      - if: osx
        then:
          - files:
              - "*.dylib"
            regex: "macos"
            replacement: "darwin"
        else:
          - files:
              - "*.so"
            regex: "linux"
            replacement: "gnu"
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();

        // Should have 1 conditional item at the top level
        assert_eq!(build.post_process.len(), 1);

        // First item should be a conditional
        let first = build.post_process.iter().next().unwrap();
        if let crate::stage0::Item::Conditional(cond) = first {
            // The 'then' branch should contain another conditional
            assert_eq!(cond.then.len(), 1);
            let inner = cond.then.iter().next().unwrap();
            assert!(matches!(inner, crate::stage0::Item::Conditional(_)));
        } else {
            panic!("Expected conditional item");
        }
    }
}
