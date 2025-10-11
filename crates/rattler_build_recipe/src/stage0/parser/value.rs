//! Value parsing functions

use marked_yaml::Node as MarkedNode;

use crate::{
    error::{ParseError, ParseResult},
    span::SpannedString,
    stage0::parser::helpers::get_span,
};

/// Parse a Value<T> from YAML
///
/// This handles both concrete values and Jinja templates
pub fn parse_value<T>(yaml: &MarkedNode) -> ParseResult<crate::stage0::types::Value<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let scalar = yaml
        .as_scalar()
        .ok_or_else(|| ParseError::expected_type("scalar", "non-scalar", get_span(yaml)))?;

    let spanned = SpannedString::from(scalar);
    let s = spanned.as_str();

    // Check if it contains a Jinja template
    if s.contains("${{") && s.contains("}}") {
        // It's a template
        let template = crate::stage0::types::JinjaTemplate::new(s.to_string())
            .map_err(|e| ParseError::jinja_error(e, spanned.span()))?;
        Ok(crate::stage0::types::Value::Template(template))
    } else {
        // Try to parse as concrete value
        let value = s
            .parse::<T>()
            .map_err(|e| ParseError::invalid_value("value", &e.to_string(), spanned.span()))?;
        Ok(crate::stage0::types::Value::Concrete(value))
    }
}
