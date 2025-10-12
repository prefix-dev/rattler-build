//! Parser for converting YAML to stage0 recipe structures
//!
//! This module provides parsing functionality that converts YAML (via marked_yaml)
//! into stage0 recipe types. All parsing preserves span information for excellent
//! error messages.

mod about;
mod build;
mod extra;
mod helpers;
mod list;
mod package;
mod requirements;
mod source;
mod test_parser;
mod value;

#[cfg(all(test, feature = "miette"))]
mod error_tests;
#[cfg(test)]
mod recipe_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod unit_tests;

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ErrorKind, ParseError, ParseResult},
    span::Span,
    stage0::parser::helpers::get_span,
};

// Re-export parsing functions
pub use about::parse_about;
pub use build::parse_build;
pub use extra::parse_extra;
pub use list::parse_conditional_list;
pub use package::parse_package;
pub use requirements::parse_requirements;
pub use source::parse_source;
pub use test_parser::parse_tests;
pub use value::parse_value;

/// Parse a complete stage0 recipe from YAML source string
pub fn parse_recipe_from_source(source: &str) -> ParseResult<crate::stage0::Stage0Recipe> {
    let yaml = marked_yaml::parse_yaml(0, source).map_err(|e| {
        ParseError::new(ErrorKind::YamlError, Span::unknown())
            .with_message(format!("Failed to parse YAML: {}", e))
    })?;

    parse_recipe(&yaml)
}

/// Parse a complete stage0 recipe from YAML
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
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("Recipe must be a mapping")
    })?;

    // Parse optional schema_version
    let schema_version = if let Some(schema_node) = mapping.get("schema_version") {
        let scalar = schema_node.as_scalar().ok_or_else(|| {
            ParseError::expected_type("scalar", "non-scalar", get_span(schema_node))
                .with_message("schema_version must be an integer")
        })?;
        let version_str = scalar.as_str();
        let version: u32 = version_str.parse().map_err(|_| {
            ParseError::invalid_value(
                "schema_version",
                "not a valid integer",
                (*scalar.span()).into(),
            )
        })?;

        // Only version 1 is supported
        if version != 1 {
            return Err(ParseError::invalid_value(
                "schema_version",
                &format!(
                    "unsupported schema version {} (only version 1 is supported)",
                    version
                ),
                (*scalar.span()).into(),
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
        .ok_or_else(|| ParseError::missing_field("package", get_span(yaml)))?;
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
        Vec::new()
    };

    // Parse optional tests section (can be empty)
    let tests = if let Some(tests_node) = mapping.get("tests") {
        parse_tests(tests_node)?
    } else {
        Vec::new()
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
                &format!("unknown top-level field '{}'", key_str),
                (*key.span()).into(),
            )
            .with_suggestion("valid top-level fields are: package, build, about, requirements, extra, source, tests, schema_version, context"));
        }
    }

    Ok(crate::stage0::Stage0Recipe {
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
///
/// Context is an order-preserving mapping of variable names to values (can be templates or concrete).
/// Order is important because later context values can reference earlier ones.
fn parse_context(
    yaml: &MarkedNode,
) -> ParseResult<indexmap::IndexMap<String, crate::stage0::types::Value<String>>> {
    let mapping = yaml.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(yaml))
            .with_message("context must be a mapping")
    })?;

    let mut context = indexmap::IndexMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();
        let value = parse_value(value_node)?;
        context.insert(key, value);
    }

    Ok(context)
}

// All section parsers (parse_package, parse_about, parse_extra, parse_requirements)
// are now implemented in their respective modules and re-exported above
