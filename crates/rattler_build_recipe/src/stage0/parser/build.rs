use marked_yaml::{Node, types::MarkedMappingNode};

use crate::{
    ParseError,
    span::SpannedString,
    stage0::{
        build::{
            BinaryRelocation, Build, DynamicLinking, ForceFileType, PostProcess, PrefixDetection,
            PrefixIgnore, PythonBuild, VariantKeyUsage,
        },
        parser::helpers::get_span,
        types::Value,
    },
};

use super::{parse_conditional_list, parse_value};

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
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'number' to be a scalar value")
                })?;
                let spanned = SpannedString::from(scalar);
                let num_str = spanned.as_str();
                let num = num_str.parse::<u64>().map_err(|_| {
                    ParseError::invalid_value(
                        "number",
                        "not a valid positive integer",
                        spanned.span(),
                    )
                })?;
                build.number = num;
            }
            "string" => {
                build.string = Some(parse_value(value_node)?);
            }
            "script" => {
                build.script = parse_conditional_list(value_node)?;
            }
            "noarch" => {
                build.noarch = Some(parse_value(value_node)?);
            }
            "python" => {
                build.python = parse_python_build(value_node)?;
            }
            "skip" => {
                build.skip = Some(parse_value(value_node)?);
            }
            "always_copy_files" => {
                build.always_copy_files = parse_conditional_list(value_node)?;
            }
            "always_include_files" => {
                build.always_include_files = parse_conditional_list(value_node)?;
            }
            "merge_build_and_host_envs" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'merge_build_and_host_envs' to be a boolean")
                })?;
                let spanned = SpannedString::from(scalar);
                let bool_str = spanned.as_str();
                build.merge_build_and_host_envs = match bool_str {
                    "true" | "True" | "yes" | "Yes" => true,
                    "false" | "False" | "no" | "No" => false,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "merge_build_and_host_envs",
                            &format!("not a valid boolean value (found '{}')", bool_str),
                            spanned.span(),
                        ));
                    }
                };
            }
            "files" => {
                build.files = parse_conditional_list(value_node)?;
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
                    ParseError::invalid_value("build", &format!("unknown field '{}'", key), (*key_node.span()).into())
                        .with_suggestion("Valid fields are: number, string, script, noarch, python, skip, always_copy_files, always_include_files, merge_build_and_host_envs, files, dynamic_linking, variant, prefix_detection, post_process")
                );
            }
        }
    }

    Ok(build)
}

fn parse_binary_relocation(node: &Node) -> Result<BinaryRelocation, ParseError> {
    // Try to parse as a boolean first
    if let Some(scalar) = node.as_scalar() {
        let spanned = SpannedString::from(scalar);
        let str_val = spanned.as_str();

        // Check if it's a boolean-like value
        match str_val {
            "true" | "True" | "yes" | "Yes" => {
                return Ok(BinaryRelocation::Boolean(Value::Concrete(true)));
            }
            "false" | "False" | "no" | "No" => {
                return Ok(BinaryRelocation::Boolean(Value::Concrete(false)));
            }
            _ => {
                // If it contains ${{ }}, treat it as a template
                if str_val.contains("${{") {
                    return Ok(BinaryRelocation::Boolean(parse_value(node)?));
                }
                // Otherwise it's an error
                return Err(ParseError::invalid_value(
                    "binary_relocation",
                    "expected 'true', 'false', or a list of glob patterns",
                    spanned.span(),
                ));
            }
        }
    }

    // Try to parse as a list of patterns
    if node.as_sequence().is_some() {
        return Ok(BinaryRelocation::Patterns(parse_conditional_list(node)?));
    }

    Err(ParseError::expected_type(
        "boolean or list",
        "invalid type",
        get_span(node),
    ))
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
                dynamic_linking.overdepending_behavior = Some(parse_value(value_node)?);
            }
            "overlinking_behavior" => {
                dynamic_linking.overlinking_behavior = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(
                    ParseError::invalid_value("dynamic_linking", &format!("unknown field '{}'", key), (*key_node.span()).into())
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
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'use_python_app_entrypoint' to be a boolean")
                })?;
                let spanned = SpannedString::from(scalar);
                let bool_str = spanned.as_str();
                python.use_python_app_entrypoint = match bool_str {
                    "true" | "True" | "yes" | "Yes" => true,
                    "false" | "False" | "no" | "No" => false,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "use_python_app_entrypoint",
                            &format!("not a valid boolean value (found '{}')", bool_str),
                            spanned.span(),
                        ));
                    }
                };
            }
            "version_independent" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'version_independent' to be a boolean")
                })?;
                let spanned = SpannedString::from(scalar);
                let bool_str = spanned.as_str();
                python.version_independent = match bool_str {
                    "true" | "True" | "yes" | "Yes" => true,
                    "false" | "False" | "no" | "No" => false,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "version_independent",
                            &format!("not a valid boolean value (found '{}')", bool_str),
                            spanned.span(),
                        ));
                    }
                };
            }
            "site_packages_path" => {
                python.site_packages_path = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(
                    ParseError::invalid_value("python", &format!("unknown field '{}'", key), (*key_node.span()).into())
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
                variant.down_prioritize_variant = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "variant",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
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
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: text, binary"));
            }
        }
    }

    Ok(force_file_type)
}

fn parse_prefix_ignore(node: &Node) -> Result<PrefixIgnore, ParseError> {
    // Try to parse as a boolean first
    if let Some(scalar) = node.as_scalar() {
        let spanned = SpannedString::from(scalar);
        let str_val = spanned.as_str();

        // Check if it's a boolean-like value
        match str_val {
            "true" | "True" | "yes" | "Yes" => {
                return Ok(PrefixIgnore::Boolean(Value::Concrete(true)));
            }
            "false" | "False" | "no" | "No" => {
                return Ok(PrefixIgnore::Boolean(Value::Concrete(false)));
            }
            _ => {
                // If it contains ${{ }}, treat it as a template
                if str_val.contains("${{") {
                    return Ok(PrefixIgnore::Boolean(parse_value(node)?));
                }
                // Otherwise it's an error
                return Err(ParseError::invalid_value(
                    "prefix_detection.ignore",
                    "expected 'true', 'false', or a list of glob patterns",
                    spanned.span(),
                ));
            }
        }
    }

    // Try to parse as a list of patterns
    if node.as_sequence().is_some() {
        return Ok(PrefixIgnore::Patterns(parse_conditional_list(node)?));
    }

    Err(ParseError::expected_type(
        "boolean or list",
        "invalid type",
        get_span(node),
    ))
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
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'ignore_binary_files' to be a boolean")
                })?;
                let spanned = SpannedString::from(scalar);
                let bool_str = spanned.as_str();
                prefix_detection.ignore_binary_files = match bool_str {
                    "true" | "True" | "yes" | "Yes" => true,
                    "false" | "False" | "no" | "No" => false,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "ignore_binary_files",
                            &format!("not a valid boolean value (found '{}')", bool_str),
                            spanned.span(),
                        ));
                    }
                };
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "prefix_detection",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
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
                regex = Some(parse_value(value_node)?);
            }
            "replacement" => {
                replacement = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "post_process",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
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

fn parse_post_process_list(node: &Node) -> Result<Vec<PostProcess>, ParseError> {
    let sequence = node.as_sequence().ok_or_else(|| {
        ParseError::expected_type("sequence", "non-sequence", get_span(node))
            .with_message("Expected 'post_process' to be a list")
    })?;

    let mut post_process_list = Vec::new();
    for item in sequence.iter() {
        post_process_list.push(parse_post_process(item)?);
    }

    Ok(post_process_list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;

    #[test]
    fn test_parse_empty_build() {
        let yaml = "{}";
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();
        assert_eq!(build.number, 0);
        assert!(build.string.is_none());
        assert!(build.script.is_empty());
    }

    #[test]
    fn test_parse_build_with_number() {
        let yaml = "number: 5";
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let build = parse_build(&node).unwrap();
        assert_eq!(build.number, 5);
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
        assert_eq!(build.script.len(), 2);
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
        assert!(matches!(err.kind, ErrorKind::InvalidValue));
        assert!(err.message.as_ref().unwrap().contains("unknown field"));
    }
}
