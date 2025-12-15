//! Parser for converting YAML to stage0 recipe structures
//!
//! This module provides parsing functionality that converts YAML (via marked_yaml)
//! into stage0 recipe types. All parsing preserves span information for excellent
//! error messages.

mod about;
mod build;
mod extra;
mod helpers;
mod output_parser;
mod package;
mod requirements;
mod source;
mod test_parser;

#[cfg(test)]
mod error_tests;
#[cfg(test)]
mod recipe_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod unit_tests;

use marked_yaml::{Node as MarkedNode, types::MarkedScalarNode};
use rattler_build_jinja::Variable;
use rattler_build_yaml_parser::{ParseError, ParseResult, parse_yaml};

use crate::Span;

// Re-export parsing functions
pub use about::parse_about;
pub use build::parse_build;
pub use extra::parse_extra;
pub use output_parser::parse_multi_output_recipe;
pub use package::parse_package;
pub use requirements::parse_requirements;
pub use source::parse_source;
pub use test_parser::parse_tests;

// Re-export helpers within crate only
pub(crate) use helpers::get_span;

/// Parse a recipe (single or multi-output) from YAML source string
///
/// This function automatically detects whether the recipe is single-output or multi-output
/// and returns the appropriate Recipe variant.
pub fn parse_recipe_or_multi_from_source(source: &str) -> ParseResult<crate::stage0::Recipe> {
    let yaml = parse_yaml(source).map_err(|e| {
        ParseError::generic(format!("Failed to parse YAML: {}", e), Span::new_blank())
    })?;

    parse_recipe_or_multi(&yaml)
}

/// Parse a complete stage0 recipe from YAML source string
///
/// Note: This function returns a SingleOutputRecipe for backwards compatibility.
/// For multi-output recipe support, use `parse_recipe_or_multi_from_source()`.
pub fn parse_recipe_from_source(source: &str) -> ParseResult<crate::stage0::Stage0Recipe> {
    let yaml = parse_yaml(source).map_err(|e| {
        ParseError::generic(format!("Failed to parse YAML: {}", e), Span::new_blank())
    })?;

    parse_recipe(&yaml)
}

/// Parse a recipe (single or multi-output) from YAML
///
/// This function automatically detects whether the recipe is single-output or multi-output:
/// - If the recipe has an "outputs" key, it's parsed as a multi-output recipe
/// - Otherwise, it's parsed as a single-output recipe
///
/// Multi-output recipes use a "recipe" section instead of "package" at the top level.
pub fn parse_recipe_or_multi(yaml: &MarkedNode) -> ParseResult<crate::stage0::Recipe> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", helpers::get_span(yaml))
            .with_message("Recipe must be a mapping")
    })?;

    // Detect multi-output by presence of "outputs" key
    if mapping.get("outputs").is_some() {
        // Multi-output recipe
        let multi = parse_multi_output_recipe(mapping)?;
        Ok(crate::stage0::Recipe::MultiOutput(Box::new(multi)))
    } else {
        // Single-output recipe
        let single = parse_single_output_recipe(yaml)?;
        Ok(crate::stage0::Recipe::SingleOutput(Box::new(single)))
    }
}

/// Parse a complete stage0 recipe from YAML
///
/// Note: This function parses single-output recipes only.
/// For multi-output recipe support, use `parse_recipe_or_multi()`.
///
/// The recipe must be a mapping with at minimum a `package` section.
/// Other sections (about, requirements, extra) are optional.
///
/// Example YAML:
/// ```yaml
/// package:
///   name: my-package
///   version: 1.0.0
/// about:
///   license: MIT
///   summary: A test package
/// requirements:
///   build:
///     - gcc
///   run:
///     - python
/// extra:
///   recipe-maintainers:
///     - alice
/// ```
pub fn parse_recipe(yaml: &MarkedNode) -> ParseResult<crate::stage0::Stage0Recipe> {
    parse_single_output_recipe(yaml)
}

/// Parse a single-output recipe from YAML
///
/// Internal function used by both parse_recipe and parse_recipe_or_multi.
fn parse_single_output_recipe(yaml: &MarkedNode) -> ParseResult<crate::stage0::SingleOutputRecipe> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", helpers::get_span(yaml))
            .with_message("Recipe must be a mapping")
    })?;

    // Parse optional schema_version
    let schema_version = if let Some(schema_node) = mapping.get("schema_version") {
        let scalar = schema_node.as_scalar().ok_or_else(|| {
            ParseError::expected_type("scalar", "non-scalar", helpers::get_span(schema_node))
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
        parse_context(context_node)?
    } else {
        indexmap::IndexMap::new()
    };

    // Parse required package section
    let package_node = mapping
        .get("package")
        .ok_or_else(|| ParseError::missing_field("package", helpers::get_span(yaml)))?;
    let package = parse_package(package_node)?;

    // Parse optional sections (will use default if not present)
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

    let requirements = if let Some(requirements_node) = mapping.get("requirements") {
        parse_requirements(requirements_node)?
    } else {
        crate::stage0::Requirements::default()
    };

    let extra = if let Some(extra_node) = mapping.get("extra") {
        parse_extra(extra_node)?
    } else {
        crate::stage0::Extra::default()
    };

    // Parse optional source section (can be empty)
    let source = if let Some(source_node) = mapping.get("source") {
        parse_source(source_node)?
    } else {
        crate::stage0::ConditionalList::default()
    };

    // Parse optional tests section (can be empty)
    let tests = if let Some(tests_node) = mapping.get("tests") {
        parse_tests(tests_node)?
    } else {
        crate::stage0::ConditionalList::default()
    };

    // Check for unknown top-level fields
    for (key, _) in mapping.iter() {
        let key_str = key.as_str();
        if !matches!(
            key_str,
            "package"
                | "build"
                | "about"
                | "requirements"
                | "extra"
                | "source"
                | "tests"
                | "schema_version"
                | "context"
        ) {
            return Err(ParseError::invalid_value(
                "recipe",
                format!("unknown top-level field '{}'", key_str),
                *key.span(),
            )
            .with_suggestion("valid top-level fields are: package, build, about, requirements, extra, source, tests, schema_version, context"));
        }
    }

    Ok(crate::stage0::SingleOutputRecipe {
        schema_version,
        context,
        package,
        build,
        about,
        requirements,
        extra,
        source,
        tests,
    })
}

/// Parse the context section from YAML
pub(crate) fn parse_context(
    yaml: &MarkedNode,
) -> ParseResult<
    indexmap::IndexMap<String, crate::stage0::types::Value<rattler_build_jinja::Variable>>,
> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", helpers::get_span(yaml))
            .with_message("context must be a mapping")
    })?;

    let mut context = indexmap::IndexMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();

        if key.contains('-') {
            return Err(ParseError::invalid_value(
                "context variable name",
                "variable names cannot contain hyphens (-) as they are not valid in Jinja expressions",
                *key_node.span(),
            ));
        }

        let value = parse_context_value(value_node, &key)?;
        context.insert(key, value);
    }

    Ok(context)
}

/// Parse a scalar into a Variable, preserving type information
fn parse_scalar_to_variable(s: &MarkedScalarNode) -> rattler_build_jinja::Variable {
    if s.may_coerce() {
        if let Some(as_bool) = s.as_bool() {
            return Variable::from(as_bool);
        } else if let Some(as_int) = s.as_i64() {
            return Variable::from(as_int);
        }
    }
    Variable::from(s.as_str().to_string())
}

/// Parse a context value - can be either a scalar or a list of uniform scalars
fn parse_context_value(
    yaml: &MarkedNode,
    key: &str,
) -> ParseResult<crate::stage0::types::Value<rattler_build_jinja::Variable>> {
    if let Some(sequence) = yaml.as_sequence() {
        parse_context_sequence(sequence, key, yaml)
    } else if let Some(scalar) = yaml.as_scalar() {
        parse_context_scalar(scalar)
    } else {
        Err(ParseError::expected_type(
            "scalar or list",
            "non-scalar/non-list",
            helpers::get_span(yaml),
        ).with_message("context values must be scalars (strings, numbers, booleans) or lists of uniform scalars"))
    }
}

/// Parse a sequence of scalars
fn parse_context_sequence(
    sequence: &[MarkedNode],
    key: &str,
    yaml: &MarkedNode,
) -> ParseResult<crate::stage0::types::Value<rattler_build_jinja::Variable>> {
    use rattler_build_jinja::Variable;

    let mut variables = Vec::new();

    for (index, item_node) in sequence.iter().enumerate() {
        let scalar = item_node.as_scalar().ok_or_else(|| {
            ParseError::expected_type("scalar", "non-scalar", helpers::get_span(item_node))
                .with_message(format!(
                    "context.{}[{}] must be a scalar (string, number, or boolean)",
                    key, index
                ))
        })?;

        variables.push(parse_scalar_to_variable(scalar));
    }

    let list_variable = Variable::from(variables);
    Ok(crate::stage0::types::Value::new_concrete(
        list_variable,
        Some(helpers::get_span(yaml)),
    ))
}

/// Parse a scalar value (may be a template or concrete value)
fn parse_context_scalar(
    scalar: &MarkedScalarNode,
) -> ParseResult<crate::stage0::types::Value<rattler_build_jinja::Variable>> {
    let s = scalar.as_str();
    let span = *scalar.span();

    if s.contains("${{") && s.contains("}}") {
        let template = crate::stage0::types::JinjaTemplate::new(s.to_string())
            .map_err(|e| ParseError::jinja_error(e, span))?;
        Ok(crate::stage0::types::Value::new_template(
            template,
            Some(span),
        ))
    } else {
        let variable = parse_scalar_to_variable(scalar);
        Ok(crate::stage0::types::Value::new_concrete(
            variable,
            Some(span),
        ))
    }
}
