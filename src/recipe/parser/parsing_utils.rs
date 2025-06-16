//! Common parsing utilities to reduce code duplication in output parsing
//!
//! This module provides shared functionality for parsing YAML nodes into
//! strongly-typed structures, reducing duplication across cache_output.rs,
//! output_parser.rs, and other parsing modules.

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};

/// A trait for types that can be parsed from a RenderedNode using a standard pattern
pub trait StandardTryConvert: Sized {
    /// The name to use in error messages (e.g., "CacheOutput", "Output")
    const TYPE_NAME: &'static str;

    /// Convert from a RenderedMappingNode
    fn from_mapping(
        mapping: &RenderedMappingNode,
        name: &str,
    ) -> Result<Self, Vec<PartialParsingError>>;
}

/// Generic implementation for converting RenderedNode to any type that implements StandardTryConvert
impl<T: StandardTryConvert> TryConvertNode<T> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<T, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping)])
            .and_then(|m| T::from_mapping(m, name))
    }
}

/// Helper to parse a required string field
pub fn parse_required_string(
    value: &RenderedNode,
    field_name: &str,
    context: &str,
) -> Result<String, Vec<PartialParsingError>> {
    Ok(value
        .as_scalar()
        .ok_or_else(|| {
            vec![_partialerror!(
                *value.span(),
                ErrorKind::ExpectedScalar,
                help = format!("expected a string for {} in {}", field_name, context)
            )]
        })?
        .as_str()
        .to_string())
}

/// Helper to parse an optional string field
pub fn parse_optional_string(
    value: &RenderedNode,
    field_name: &str,
    context: &str,
) -> Result<Option<String>, Vec<PartialParsingError>> {
    Ok(Some(parse_required_string(value, field_name, context)?))
}

/// Helper to parse a boolean field
pub fn parse_bool(
    value: &RenderedNode,
    field_name: &str,
    context: &str,
) -> Result<bool, Vec<PartialParsingError>> {
    let scalar = value.as_scalar().ok_or_else(|| {
        vec![_partialerror!(
            *value.span(),
            ErrorKind::ExpectedScalar,
            help = format!("expected a boolean for {} in {}", field_name, context)
        )]
    })?;

    scalar.as_bool().ok_or_else(|| {
        vec![_partialerror!(
            *value.span(),
            ErrorKind::ExpectedScalar,
            help = format!("expected a boolean for {} in {}", field_name, context)
        )]
    })
}

/// Helper to create a missing field error
pub fn missing_field_error(
    span: marked_yaml::Span,
    field_name: &str,
    type_name: &str,
) -> PartialParsingError {
    _partialerror!(
        span,
        ErrorKind::MissingField(field_name.to_string().into()),
        help = format!("{} must have a '{}' field", type_name, field_name)
    )
}

/// Helper to create an invalid field error
pub fn invalid_field_error(
    span: marked_yaml::Span,
    field_name: &str,
    help_text: Option<&str>,
) -> PartialParsingError {
    if let Some(help) = help_text {
        let help_string = help.to_string();
        _partialerror!(
            span,
            ErrorKind::InvalidField(field_name.to_string().into()),
            help = help_string
        )
    } else {
        _partialerror!(span, ErrorKind::InvalidField(field_name.to_string().into()))
    }
}

/// Helper to validate that a mapping only contains allowed keys
pub fn validate_mapping_keys(
    mapping: &RenderedMappingNode,
    allowed_keys: &[&str],
    context: &str,
) -> Result<(), Vec<PartialParsingError>> {
    let mut errors = Vec::new();

    for (key, _) in mapping.iter() {
        if !allowed_keys.contains(&key.as_str()) {
            errors.push(invalid_field_error(
                *key.span(),
                key.as_str(),
                Some(&format!(
                    "allowed keys in {} are: {}",
                    context,
                    allowed_keys.join(", ")
                )),
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Macro to generate standard field parsing match arms
#[macro_export]
macro_rules! parse_field {
    // Required field with custom parser
    ($field:ident, $value:expr, $parser:expr, $context:expr) => {
        $field = Some($parser($value, $context)?);
    };

    // Optional field with custom parser
    (optional $field:ident, $value:expr, $parser:expr, $context:expr) => {
        $field = $parser($value, $context)?;
    };

    // Direct try_convert
    ($field:ident, $value:expr, $context:expr) => {
        $field = Some($value.try_convert($context)?);
    };

    // Optional direct try_convert
    (optional $field:ident, $value:expr, $context:expr) => {
        $field = $value.try_convert($context)?;
    };
}

/// Helper for validating multi-output recipe root keys
pub fn validate_multi_output_root_keys(
    root_map: &RenderedMappingNode,
) -> Result<(), PartialParsingError> {
    if root_map.contains_key("package") {
        let key = root_map
            .keys()
            .find(|k| k.as_str() == "package")
            .expect("key exists");
        return Err(_partialerror!(
            *key.span(),
            ErrorKind::InvalidField("package".to_string().into()),
            help = "recipe cannot have both `outputs` and `package` fields. Rename `package` to `recipe` or remove `outputs`"
        ));
    }

    if root_map.contains_key("requirements") {
        let key = root_map
            .keys()
            .find(|k| k.as_str() == "requirements")
            .expect("key exists");
        return Err(_partialerror!(
            *key.span(),
            ErrorKind::InvalidField("requirements".to_string().into()),
            help = "multi-output recipes cannot have a top-level requirements field. Move `requirements` inside the individual output."
        ));
    }

    Ok(())
}
