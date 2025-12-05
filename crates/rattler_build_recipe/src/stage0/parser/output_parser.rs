//! Parser for multi-output recipes with staging support

use marked_yaml::Node as MarkedNode;
use rattler_build_jinja::JinjaTemplate;
use rattler_build_yaml_parser::{ParseMapping, parse_value};

use crate::{
    error::{ParseError, ParseResult},
    stage0::{
        ConditionalList, Requirements, Value,
        output::{
            CacheInherit, Inherit, MultiOutputRecipe, Output, PackageOutput, RecipeMetadata,
            StagingBuild, StagingMetadata, StagingOutput,
        },
        parser::{
            get_span, parse_about, parse_build, parse_extra, parse_requirements, parse_source,
            parse_tests,
        },
    },
};

/// Parse a multi-output recipe from YAML
///
/// Multi-output recipes have an "outputs" section and use "recipe" instead of "package"
pub fn parse_multi_output_recipe(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> ParseResult<MultiOutputRecipe> {
    // Parse optional schema_version
    let schema_version = if let Some(schema_node) = mapping.get("schema_version") {
        let scalar = schema_node.as_scalar().ok_or_else(|| {
            ParseError::expected_type("scalar", "non-scalar", get_span(schema_node))
                .with_message("schema_version must be an integer")
        })?;
        let version_str = scalar.as_str();
        let version: u32 = version_str.parse().map_err(|_| {
            ParseError::invalid_value("schema_version", "not a valid integer", *scalar.span())
        })?;

        // Only version 1 is supported
        if version != 1 {
            return Err(ParseError::invalid_value(
                "schema_version",
                format!(
                    "unsupported schema version {} (only version 1 is supported)",
                    version
                ),
                *scalar.span(),
            ));
        }
        Some(version)
    } else {
        None
    };

    // Parse optional context
    let context = if let Some(context_node) = mapping.get("context") {
        super::parse_context(context_node)?
    } else {
        indexmap::IndexMap::new()
    };

    // Parse optional recipe metadata
    let recipe = if let Some(recipe_node) = mapping.get("recipe") {
        parse_recipe_metadata(recipe_node)?
    } else if let Some(version_node) = mapping.get("version") {
        // version can also be at top-level for backwards compatibility
        RecipeMetadata {
            name: None,
            version: Some(parse_value(version_node)?),
        }
    } else {
        // No recipe or version at top level - outputs must provide all necessary info
        RecipeMetadata::default()
    };

    // Parse top-level inheritable sections
    let source = if let Some(source_node) = mapping.get("source") {
        parse_source(source_node)?
    } else {
        Vec::new()
    };

    let build = if let Some(build_node) = mapping.get("build") {
        parse_build(build_node)?
    } else {
        crate::stage0::Build::default()
    };

    let about = if let Some(about_node) = mapping.get("about") {
        parse_about(about_node)?
    } else {
        crate::stage0::About::default()
    };

    let extra = if let Some(extra_node) = mapping.get("extra") {
        parse_extra(extra_node)?
    } else {
        crate::stage0::Extra::default()
    };

    let tests = if let Some(tests_node) = mapping.get("tests") {
        parse_tests(tests_node)?
    } else {
        ConditionalList::default()
    };

    // Parse outputs (required)
    let outputs_node = mapping.get("outputs").ok_or_else(|| {
        ParseError::missing_field("outputs", get_span(&MarkedNode::Mapping(mapping.clone())))
    })?;
    let outputs = parse_outputs(outputs_node)?;

    // Validate: at least one output required
    if outputs.is_empty() {
        return Err(ParseError::invalid_value(
            "outputs",
            "at least one output is required",
            get_span(outputs_node),
        ));
    }

    // Validate field names
    let node = MarkedNode::Mapping(mapping.clone());
    node.validate_keys(
        "multi-output recipe",
        &[
            "recipe",
            "version",
            "build",
            "about",
            "extra",
            "source",
            "tests",
            "outputs",
            "schema_version",
            "context",
        ],
    )?;

    Ok(MultiOutputRecipe {
        schema_version,
        context,
        recipe,
        source,
        build,
        about,
        extra,
        tests,
        outputs,
    })
}

/// Parse recipe metadata (name optional, version required)
fn parse_recipe_metadata(yaml: &MarkedNode) -> ParseResult<RecipeMetadata> {
    // Validate field names first
    yaml.validate_keys("recipe", &["name", "version"])?;

    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("recipe must be a mapping")
    })?;

    // Parse optional name (needs special handling for PackageName)
    let name = if let Some(name_node) = mapping.get("name") {
        let scalar = name_node.as_scalar().ok_or_else(|| {
            ParseError::expected_type("scalar", "non-scalar", get_span(name_node))
                .with_message("recipe name must be a scalar")
        })?;

        let name_str = scalar.as_str();
        let span = *scalar.span();

        // Check if it's a template
        if name_str.contains("${{") && name_str.contains("}}") {
            let template = JinjaTemplate::new(name_str.to_string())
                .map_err(|e| ParseError::jinja_error(e, span))?;
            Some(Value::new_template(template, Some(span)))
        } else {
            // Parse as PackageName
            let package_name = rattler_conda_types::PackageName::try_from(name_str)
                .map_err(|e| ParseError::invalid_value("name", e.to_string(), span))?;
            Some(Value::new_concrete(
                crate::stage0::package::PackageName(package_name),
                Some(span),
            ))
        }
    } else {
        None
    };

    // Parse optional version
    let version = if let Some(version_node) = mapping.get("version") {
        Some(parse_value(version_node)?)
    } else {
        None
    };

    Ok(RecipeMetadata { name, version })
}

/// Parse outputs section (list of staging and package outputs)
fn parse_outputs(yaml: &MarkedNode) -> ParseResult<Vec<Output>> {
    let sequence = yaml.as_sequence().ok_or_else(|| {
        ParseError::expected_type("sequence", "non-sequence", get_span(yaml))
            .with_message("outputs must be a list")
    })?;

    let mut outputs = Vec::new();

    for item in sequence.iter() {
        let mapping = item.as_mapping().ok_or_else(|| {
            ParseError::expected_type("mapping", "non-mapping", get_span(item))
                .with_message("each output must be a mapping")
        })?;

        // Determine output type by checking which key is present
        if mapping.get("staging").is_some() {
            outputs.push(Output::Staging(Box::new(parse_staging_output(mapping)?)));
        } else if mapping.get("package").is_some() {
            outputs.push(Output::Package(Box::new(parse_package_output(mapping)?)));
        } else {
            return Err(ParseError::invalid_value(
                "output",
                "must have either 'staging' or 'package' key",
                get_span(item),
            )
            .with_suggestion("outputs should have either a 'staging' key (for cache outputs) or 'package' key (for package outputs)"));
        }
    }

    Ok(outputs)
}

/// Parse a staging output
fn parse_staging_output(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> ParseResult<StagingOutput> {
    // Parse staging metadata (required)
    let staging_node = mapping.get("staging").ok_or_else(|| {
        ParseError::missing_field("staging", get_span(&MarkedNode::Mapping(mapping.clone())))
    })?;
    let staging = parse_staging_metadata(staging_node)?;

    // Parse optional source
    let source = if let Some(source_node) = mapping.get("source") {
        parse_source(source_node)?
    } else {
        Vec::new()
    };

    // Parse optional requirements (only build/host/ignore_run_exports allowed)
    let requirements = if let Some(req_node) = mapping.get("requirements") {
        parse_staging_requirements(req_node)?
    } else {
        Requirements::default()
    };

    // Parse optional build (only script allowed)
    let build = if let Some(build_node) = mapping.get("build") {
        parse_staging_build(build_node)?
    } else {
        StagingBuild::default()
    };

    // Validate field names
    let node = MarkedNode::Mapping(mapping.clone());
    node.validate_keys(
        "staging output",
        &["staging", "source", "requirements", "build"],
    )?;

    // Validate: staging outputs cannot have about or tests
    if mapping.get("about").is_some() {
        return Err(ParseError::invalid_value(
            "staging output",
            "staging outputs cannot have an 'about' section",
            *mapping.get("about").unwrap().span(),
        ));
    }
    if mapping.get("tests").is_some() {
        return Err(ParseError::invalid_value(
            "staging output",
            "staging outputs cannot have a 'tests' section",
            *mapping.get("tests").unwrap().span(),
        ));
    }

    Ok(StagingOutput {
        staging,
        source,
        requirements,
        build,
    })
}

/// Parse staging metadata
fn parse_staging_metadata(yaml: &MarkedNode) -> ParseResult<StagingMetadata> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("staging must be a mapping")
    })?;

    // Parse required name
    let name_node = mapping
        .get("name")
        .ok_or_else(|| ParseError::missing_field("name", get_span(yaml)))?;
    let name = parse_value(name_node)?;

    // Validate field names
    let node = MarkedNode::Mapping(mapping.clone());
    node.validate_keys("staging", &["name"])?;

    Ok(StagingMetadata { name })
}

/// Parse staging requirements (only build/host/ignore_run_exports allowed)
fn parse_staging_requirements(yaml: &MarkedNode) -> ParseResult<Requirements> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("requirements must be a mapping")
    })?;

    // Check for disallowed run-time fields with helpful error message
    for (key_node, _) in mapping.iter() {
        let key = key_node.as_str();
        if matches!(key, "run" | "run_constraints" | "run_exports") {
            return Err(ParseError::invalid_value(
                "staging requirements",
                format!("'{}' is not allowed in staging requirements", key),
                *key_node.span(),
            )
            .with_suggestion(
                "staging outputs can only have 'build', 'host', and 'ignore_run_exports' requirements",
            ));
        }
    }

    // Validate field names
    yaml.validate_keys(
        "staging requirements",
        &["build", "host", "ignore_run_exports"],
    )?;

    // Parse using the regular parse_requirements function
    parse_requirements(yaml)
}

/// Parse staging build (only script allowed)
fn parse_staging_build(yaml: &MarkedNode) -> ParseResult<StagingBuild> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("build must be a mapping")
    })?;

    let mut build = StagingBuild::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "script" => {
                build.script = crate::stage0::parser::build::parse_script(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "staging build",
                    format!(
                        "unknown field '{}' - only 'script' is allowed in staging builds",
                        key
                    ),
                    *key_node.span(),
                )
                .with_suggestion(
                    "staging outputs can only have a 'script' field in the build section",
                ));
            }
        }
    }

    Ok(build)
}

/// Parse package metadata for multi-output recipes (version is optional)
fn parse_package_metadata(yaml: &MarkedNode) -> ParseResult<crate::stage0::PackageMetadata> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("package must be a mapping")
    })?;

    // Parse required name
    let name_node = mapping
        .get("name")
        .ok_or_else(|| ParseError::missing_field("name", get_span(yaml)))?;

    let scalar = name_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(name_node))
            .with_message("package name must be a scalar")
    })?;

    let name_str = scalar.as_str();
    let span = *scalar.span();

    // Check if it's a template
    let name = if name_str.contains("${{") && name_str.contains("}}") {
        let template = JinjaTemplate::new(name_str.to_string())
            .map_err(|e| ParseError::jinja_error(e, span))?;
        Value::new_template(template, Some(span))
    } else {
        // Parse as PackageName
        let package_name = rattler_conda_types::PackageName::try_from(name_str)
            .map_err(|e| ParseError::invalid_value("name", e.to_string(), span))?;
        Value::new_concrete(
            crate::stage0::package::PackageName(package_name),
            Some(span),
        )
    };

    // Parse optional version (can be inherited from recipe)
    let version = if let Some(version_node) = mapping.get("version") {
        Some(parse_value(version_node)?)
    } else {
        None
    };

    // Validate field names
    yaml.validate_keys("package", &["name", "version"])?;

    Ok(crate::stage0::PackageMetadata { name, version })
}

/// Parse a package output
fn parse_package_output(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> ParseResult<PackageOutput> {
    // Parse package metadata (required)
    let package_node = mapping.get("package").ok_or_else(|| {
        ParseError::missing_field("package", get_span(&MarkedNode::Mapping(mapping.clone())))
    })?;
    let package = parse_package_metadata(package_node)?;

    // Parse optional inherit
    let inherit = if let Some(inherit_node) = mapping.get("inherit") {
        parse_inherit(inherit_node)?
    } else {
        Inherit::TopLevel
    };

    // Parse optional source
    let source = if let Some(source_node) = mapping.get("source") {
        parse_source(source_node)?
    } else {
        Vec::new()
    };

    // Parse optional requirements
    let requirements = if let Some(req_node) = mapping.get("requirements") {
        parse_requirements(req_node)?
    } else {
        crate::stage0::Requirements::default()
    };

    // Parse optional build
    let build = if let Some(build_node) = mapping.get("build") {
        parse_build(build_node)?
    } else {
        crate::stage0::Build::default()
    };

    // Parse optional about
    let about = if let Some(about_node) = mapping.get("about") {
        parse_about(about_node)?
    } else {
        crate::stage0::About::default()
    };

    // Parse optional tests
    let tests = if let Some(tests_node) = mapping.get("tests") {
        parse_tests(tests_node)?
    } else {
        ConditionalList::default()
    };

    // Validate field names
    let node = MarkedNode::Mapping(mapping.clone());
    node.validate_keys(
        "package output",
        &[
            "package",
            "inherit",
            "source",
            "requirements",
            "build",
            "about",
            "tests",
        ],
    )?;

    Ok(PackageOutput {
        package,
        inherit,
        source,
        requirements,
        build,
        about,
        tests,
    })
}

/// Parse inherit configuration
fn parse_inherit(yaml: &MarkedNode) -> ParseResult<Inherit> {
    // Check for string (short form - just the cache name) or null
    if let Some(scalar) = yaml.as_scalar() {
        let s = scalar.as_str();
        let span = *scalar.span();

        // Check for null values (null, ~, or empty string)
        if s == "null" || s == "~" || s.is_empty() {
            return Ok(Inherit::TopLevel);
        }

        // Check if it's a template
        if s.contains("${{") && s.contains("}}") {
            let template =
                JinjaTemplate::new(s.to_string()).map_err(|e| ParseError::jinja_error(e, span))?;
            return Ok(Inherit::CacheName(Value::new_template(
                template,
                Some(span),
            )));
        }

        // Plain string
        return Ok(Inherit::CacheName(Value::new_concrete(
            s.to_string(),
            Some(span),
        )));
    }

    // Check for mapping (long form with options)
    if let Some(mapping) = yaml.as_mapping() {
        // Validate field names first
        yaml.validate_keys("inherit", &["from", "run_exports"])?;

        let mut from = None;
        let mut run_exports = true; // default

        for (key_node, value_node) in mapping.iter() {
            let key = key_node.as_str();

            match key {
                "from" => {
                    from = Some(parse_value(value_node)?);
                }
                "run_exports" => {
                    let scalar = value_node.as_scalar().ok_or_else(|| {
                        ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                            .with_message("run_exports must be a boolean")
                    })?;
                    let bool_str = scalar.as_str();
                    run_exports = match bool_str {
                        "true" | "True" | "yes" | "Yes" => true,
                        "false" | "False" | "no" | "No" => false,
                        _ => {
                            return Err(ParseError::invalid_value(
                                "run_exports",
                                format!("not a valid boolean value (found '{}')", bool_str),
                                *scalar.span(),
                            ));
                        }
                    };
                }
                _ => unreachable!("validated by validate_keys"),
            }
        }

        let from = from.ok_or_else(|| ParseError::missing_field("from", get_span(yaml)))?;

        return Ok(Inherit::CacheWithOptions(CacheInherit {
            from,
            run_exports,
        }));
    }

    Err(ParseError::expected_type(
        "null, string, or mapping",
        "other",
        get_span(yaml),
    )
    .with_message("inherit must be null (for top-level), a string (cache name), or a mapping with 'from' and optional 'run_exports'"))
}
