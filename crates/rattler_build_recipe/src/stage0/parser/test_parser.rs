use marked_yaml::Node;

use crate::{
    ParseError,
    span::SpannedString,
    stage0::{
        parser::helpers::get_span,
        tests::{
            CommandsTest, CommandsTestFiles, CommandsTestRequirements, DownstreamTest,
            PackageContentsCheckFiles, PackageContentsTest, PerlTest, PythonTest, PythonVersion,
            RTest, RubyTest, TestType,
        },
    },
};

use super::{helpers::validate_mapping_fields, parse_conditional_list, parse_value};

/// Parse tests section from YAML (expects a sequence)
pub fn parse_tests(node: &Node) -> Result<Vec<TestType>, ParseError> {
    let seq = node.as_sequence().ok_or_else(|| {
        ParseError::expected_type("sequence", "non-sequence", get_span(node))
            .with_message("Expected 'tests' to be a sequence")
    })?;

    let mut tests = Vec::new();
    for item in seq.iter() {
        tests.push(parse_single_test(item)?);
    }
    Ok(tests)
}

fn parse_single_test(node: &Node) -> Result<TestType, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Each test must be a mapping")
    })?;

    // Determine test type by checking which field is present
    if mapping.get("python").is_some() {
        let python_node = mapping.get("python").unwrap();
        let python = parse_python_test(python_node.as_mapping().ok_or_else(|| {
            ParseError::expected_type("mapping", "non-mapping", get_span(python_node))
        })?)?;
        Ok(TestType::Python { python })
    } else if mapping.get("perl").is_some() {
        let perl_node = mapping.get("perl").unwrap();
        let perl = parse_perl_test(perl_node.as_mapping().ok_or_else(|| {
            ParseError::expected_type("mapping", "non-mapping", get_span(perl_node))
        })?)?;
        Ok(TestType::Perl { perl })
    } else if mapping.get("r").is_some() {
        let r_node = mapping.get("r").unwrap();
        let r = parse_r_test(r_node.as_mapping().ok_or_else(|| {
            ParseError::expected_type("mapping", "non-mapping", get_span(r_node))
        })?)?;
        Ok(TestType::R { r })
    } else if mapping.get("ruby").is_some() {
        let ruby_node = mapping.get("ruby").unwrap();
        let ruby = parse_ruby_test(ruby_node.as_mapping().ok_or_else(|| {
            ParseError::expected_type("mapping", "non-mapping", get_span(ruby_node))
        })?)?;
        Ok(TestType::Ruby { ruby })
    } else if mapping.get("script").is_some() {
        Ok(TestType::Commands(parse_commands_test(mapping)?))
    } else if mapping.get("downstream").is_some() {
        Ok(TestType::Downstream(parse_downstream_test(mapping)?))
    } else if mapping.get("package_contents").is_some() {
        let package_contents_node = mapping.get("package_contents").unwrap();
        let package_contents =
            parse_package_contents_test(package_contents_node.as_mapping().ok_or_else(|| {
                ParseError::expected_type("mapping", "non-mapping", get_span(package_contents_node))
            })?)?;
        Ok(TestType::PackageContents { package_contents })
    } else {
        Err(ParseError::invalid_value(
            "test",
            "missing test type field (python, perl, r, ruby, script, downstream, package_contents)",
            get_span(node),
        ))
    }
}

fn parse_python_test(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<PythonTest, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut imports = ConditionalList::default();
    let mut pip_check = None;
    let mut python_version = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "imports" => {
                imports = parse_conditional_list(value_node)?;
            }
            "pip_check" => {
                pip_check = Some(parse_value(value_node)?);
            }
            "python_version" => {
                python_version = Some(parse_python_version(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "python test",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: imports, pip_check, python_version"));
            }
        }
    }

    Ok(PythonTest {
        imports,
        pip_check,
        python_version,
    })
}

fn parse_python_version(node: &Node) -> Result<PythonVersion, ParseError> {
    if let Some(seq) = node.as_sequence() {
        // Multiple versions
        let mut versions = Vec::new();
        for item in seq.iter() {
            versions.push(parse_value(item)?);
        }
        Ok(PythonVersion::Multiple(versions))
    } else {
        // Single version
        Ok(PythonVersion::Single(parse_value(node)?))
    }
}

fn parse_perl_test(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<PerlTest, ParseError> {
    use crate::stage0::types::ConditionalList;

    // Validate that all fields are known
    validate_mapping_fields(mapping, "perl test", &["uses"])?;

    let mut uses = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "uses" => {
                uses = parse_conditional_list(value_node)?;
            }
            _ => unreachable!("Unknown field should have been caught by validation"),
        }
    }

    Ok(PerlTest { uses })
}

fn parse_r_test(mapping: &marked_yaml::types::MarkedMappingNode) -> Result<RTest, ParseError> {
    use crate::stage0::types::ConditionalList;

    // Validate that all fields are known
    validate_mapping_fields(mapping, "r test", &["libraries"])?;

    let mut libraries = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "libraries" => {
                libraries = parse_conditional_list(value_node)?;
            }
            _ => unreachable!("Unknown field should have been caught by validation"),
        }
    }

    Ok(RTest { libraries })
}

fn parse_ruby_test(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<RubyTest, ParseError> {
    use crate::stage0::types::ConditionalList;

    // Validate that all fields are known
    validate_mapping_fields(mapping, "ruby test", &["requires"])?;

    let mut requires = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "requires" => {
                requires = parse_conditional_list(value_node)?;
            }
            _ => unreachable!("Unknown field should have been caught by validation"),
        }
    }

    Ok(RubyTest { requires })
}

fn parse_inline_script(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<crate::stage0::types::InlineScript, ParseError> {
    let mut interpreter = None;
    let mut env = indexmap::IndexMap::new();
    let mut secrets = Vec::new();
    let mut content = None;
    let mut file = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "interpreter" => {
                interpreter = Some(parse_value(value_node)?);
            }
            "env" => {
                let env_mapping = value_node.as_mapping().ok_or_else(|| {
                    ParseError::expected_type("mapping", "non-mapping", get_span(value_node))
                        .with_message("env must be a mapping")
                })?;

                for (env_key_node, env_value_node) in env_mapping.iter() {
                    let env_key = env_key_node.as_str().to_string();
                    let env_value = parse_value(env_value_node)?;
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
                    secrets.push(SpannedString::from(scalar).as_str().to_string());
                }
            }
            "content" => {
                // Content can be either a string or a list
                if let Some(scalar) = value_node.as_scalar() {
                    // Single string - convert to ConditionalList with one item
                    let spanned = SpannedString::from(scalar);
                    let content_str = spanned.as_str();

                    // Check if it's a template
                    if content_str.contains("${{") && content_str.contains("}}") {
                        let template =
                            crate::stage0::types::JinjaTemplate::new(content_str.to_string())
                                .map_err(|e| ParseError::jinja_error(e, spanned.span()))?;
                        content = Some(crate::stage0::types::ConditionalList::new(vec![
                            crate::stage0::types::Item::Value(
                                crate::stage0::types::Value::new_template(template, spanned.span()),
                            ),
                        ]));
                    } else {
                        // Plain string - split by newlines if multiline
                        let lines: Vec<String> = content_str
                            .lines()
                            .map(|s| s.to_string())
                            .filter(|s| !s.trim().is_empty())
                            .collect();

                        let items: Vec<crate::stage0::types::Item<String>> = lines
                            .into_iter()
                            .map(|line| {
                                crate::stage0::types::Item::Value(
                                    crate::stage0::types::Value::new_concrete(line, spanned.span()),
                                )
                            })
                            .collect();

                        content = Some(crate::stage0::types::ConditionalList::new(items));
                    }
                } else {
                    // Parse as a list (with possible conditionals)
                    content = Some(parse_conditional_list(value_node)?);
                }
            }
            "file" => {
                file = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "inline script",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: interpreter, env, secrets, content, file"));
            }
        }
    }

    Ok(crate::stage0::types::InlineScript {
        interpreter,
        env,
        secrets,
        content,
        file,
    })
}

fn parse_script_field(
    node: &Node,
) -> Result<crate::stage0::types::ConditionalList<crate::stage0::types::ScriptContent>, ParseError>
{
    use crate::stage0::types::{ConditionalList, Item, ScriptContent, Value};

    // Try parsing as a sequence first (the standard way for multiple items)
    if node.as_sequence().is_some() {
        return parse_conditional_list(node);
    }

    // Try parsing as a single scalar string
    if let Some(scalar) = node.as_scalar() {
        let spanned = SpannedString::from(scalar);
        let script_str = spanned.as_str();

        // Check if it's a template
        if script_str.contains("${{") && script_str.contains("}}") {
            let template = crate::stage0::types::JinjaTemplate::new(script_str.to_string())
                .map_err(|e| ParseError::jinja_error(e, spanned.span()))?;
            let items = vec![Item::Value(Value::new_template(template, spanned.span()))];
            return Ok(ConditionalList::new(items));
        }

        // Plain string command
        let items = vec![Item::Value(Value::new_concrete(
            ScriptContent::Command(script_str.to_string()),
            spanned.span(),
        ))];
        return Ok(ConditionalList::new(items));
    }

    // Try parsing as a single inline script object (mapping)
    if let Some(mapping) = node.as_mapping() {
        // Parse the inline script from the mapping
        let inline_script = parse_inline_script(mapping)?;
        let span = get_span(node);
        let items = vec![Item::Value(Value::new_concrete(
            ScriptContent::Inline(Box::new(inline_script)),
            span,
        ))];
        return Ok(ConditionalList::new(items));
    }

    Err(ParseError::expected_type(
        "sequence, scalar string, or inline script object",
        "other",
        get_span(node),
    )
    .with_message("script must be a string, list of strings/objects, or an inline script object"))
}

fn parse_commands_test(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<CommandsTest, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut script = ConditionalList::default();
    let mut requirements = None;
    let mut files = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "script" => {
                script = parse_script_field(value_node)?;
            }
            "requirements" => {
                requirements = Some(parse_commands_test_requirements(
                    value_node.as_mapping().ok_or_else(|| {
                        ParseError::expected_type("mapping", "non-mapping", get_span(value_node))
                    })?,
                )?);
            }
            "files" => {
                files = Some(parse_commands_test_files(
                    value_node.as_mapping().ok_or_else(|| {
                        ParseError::expected_type("mapping", "non-mapping", get_span(value_node))
                    })?,
                )?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "commands test",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: script, requirements, files"));
            }
        }
    }

    Ok(CommandsTest {
        script,
        requirements,
        files,
    })
}

fn parse_commands_test_requirements(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<CommandsTestRequirements, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut run = ConditionalList::default();
    let mut build = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "run" => {
                run = parse_conditional_list(value_node)?;
            }
            "build" => {
                build = parse_conditional_list(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "commands test requirements",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: run, build"));
            }
        }
    }

    Ok(CommandsTestRequirements { run, build })
}

fn parse_commands_test_files(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<CommandsTestFiles, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut source = ConditionalList::default();
    let mut recipe = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "source" => {
                source = parse_conditional_list(value_node)?;
            }
            "recipe" => {
                recipe = parse_conditional_list(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "commands test files",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: source, recipe"));
            }
        }
    }

    Ok(CommandsTestFiles { source, recipe })
}

fn parse_downstream_test(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<DownstreamTest, ParseError> {
    let mut downstream = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "downstream" => {
                downstream = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "downstream test",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: downstream"));
            }
        }
    }

    let downstream = downstream.ok_or_else(|| {
        ParseError::missing_field("downstream", get_span(&Node::Mapping(mapping.clone())))
    })?;

    Ok(DownstreamTest { downstream })
}

fn parse_package_contents_test(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<PackageContentsTest, ParseError> {
    let mut files = None;
    let mut site_packages = None;
    let mut bin = None;
    let mut lib = None;
    let mut include = None;
    let mut strict = false;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "files" => {
                files = Some(parse_package_contents_check_files_flexible(value_node)?);
            }
            "site_packages" => {
                site_packages = Some(parse_package_contents_check_files_flexible(value_node)?);
            }
            "bin" => {
                bin = Some(parse_package_contents_check_files_flexible(value_node)?);
            }
            "lib" => {
                lib = Some(parse_package_contents_check_files_flexible(value_node)?);
            }
            "include" => {
                include = Some(parse_package_contents_check_files_flexible(value_node)?);
            }
            "strict" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'strict' to be a boolean")
                })?;
                let spanned = SpannedString::from(scalar);
                strict = match spanned.as_str() {
                    "true" | "True" | "yes" | "Yes" => true,
                    "false" | "False" | "no" | "No" => false,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "strict",
                            &format!("not a valid boolean value (found '{}')", spanned.as_str()),
                            spanned.span(),
                        ));
                    }
                };
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "package_contents test",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion(
                    "Valid fields are: files, site_packages, bin, lib, include, strict",
                ));
            }
        }
    }

    Ok(PackageContentsTest {
        files,
        site_packages,
        bin,
        lib,
        include,
        strict,
    })
}

/// Parse package contents check files with flexible format support
/// Supports two formats:
/// 1. Shorthand (list): `- foo` means "foo must exist"
/// 2. Explicit (mapping): `exists: [foo]` and/or `not_exists: [bar]`
fn parse_package_contents_check_files_flexible(
    node: &Node,
) -> Result<PackageContentsCheckFiles, ParseError> {
    use crate::stage0::types::ConditionalList;

    // Try to parse as a mapping first (explicit format with exists/not_exists)
    if let Some(mapping) = node.as_mapping() {
        return parse_package_contents_check_files(mapping);
    }

    // Try to parse as a sequence (shorthand format - simple list means "exists")
    if node.as_sequence().is_some() {
        let exists = parse_conditional_list(node)?;
        return Ok(PackageContentsCheckFiles {
            exists,
            not_exists: ConditionalList::default(),
        });
    }

    Err(ParseError::expected_type(
        "sequence or mapping with exists/not_exists",
        "other",
        get_span(node),
    )
    .with_message(
        "package_contents field must be either a list of files (shorthand) or a mapping with exists/not_exists keys",
    ))
}

fn parse_package_contents_check_files(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<PackageContentsCheckFiles, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut exists = ConditionalList::default();
    let mut not_exists = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();
        match key {
            "exists" => {
                exists = parse_conditional_list(value_node)?;
            }
            "not_exists" => {
                not_exists = parse_conditional_list(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "package_contents check files",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion("Valid fields are: exists, not_exists"));
            }
        }
    }

    Ok(PackageContentsCheckFiles { exists, not_exists })
}
