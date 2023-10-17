use std::{fmt, str::ParseBoolError};

use miette::{Diagnostic, SourceOffset, SourceSpan};
use thiserror::Error;

/// The Error type for the first stage of the recipe parser.
///
/// This type was designed to be compatible with [`miette`], and its [`Diagnostic`] trait.
#[derive(Debug, Error, Diagnostic)]
#[error("Parsing: {kind}")]
pub struct ParsingError {
    // Source string of the recipe.
    #[source_code]
    pub src: String,

    /// Offset in chars of the error.
    #[label("{}", label.unwrap_or("here"))]
    pub span: SourceSpan,

    /// Label text for this span. Defaults to `"here"`.
    pub label: Option<&'static str>,

    /// Suggestion for fixing the parser error.
    #[help]
    pub help: Option<&'static str>,

    // Specific error kind for the error.
    pub kind: ErrorKind,
}

impl ParsingError {
    pub fn from_partial(src: &str, err: PartialParsingError) -> Self {
        Self {
            src: src.to_owned(),
            span: markerspan2span(src, err.span),
            label: err.label,
            help: err.help,
            kind: err.kind,
        }
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
}

/// Type that represents the kind of error that can happen in the first stage of the recipe parser.
#[derive(Debug, Error, Diagnostic)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Error while parsing YAML.
    #[diagnostic(code(error::stage1::yaml_parsing))]
    YamlParsing(#[from] marked_yaml::LoadError),

    /// Error when expected mapping but got something else.
    #[diagnostic(code(error::stage1::expected_mapping))]
    ExpectedMapping,

    /// Error when expected scalar but got something else.
    #[diagnostic(code(error::stage1::expected_scalar))]
    ExpectedScalar,

    /// Error when expected sequence but got something else.
    #[diagnostic(code(error::stage1::expected_sequence))]
    ExpectedSequence,

    /// Error when if-selector condition is not a scalar.
    #[diagnostic(code(error::stage1::if_selector_condition_not_scalar))]
    IfSelectorConditionNotScalar,

    /// Error when if selector is missing a `then` field.
    #[diagnostic(code(error::stage1::if_selector_missing_then))]
    IfSelectorMissingThen,

    /// Error rendering a Jinja expression.
    #[diagnostic(code(error::stage2::jinja_rendering))]
    JinjaRendering(#[from] minijinja::Error),

    /// Error processing the condition of a if-selector.
    #[diagnostic(code(error::stage2::if_selector_condition_not_bool))]
    IfSelectorConditionNotBool(#[from] ParseBoolError),

    /// Generic unspecified error. If this is returned, the call site should
    /// be annotated with context, if possible.
    #[diagnostic(code(error::other))]
    Other,
}

#[derive(Debug, Error)]
#[error("Parsing: {kind}")]
pub struct PartialParsingError {
    /// Offset in chars of the error.
    pub span: marked_yaml::Span,

    /// Label text for this span. Defaults to `"here"`.
    pub label: Option<&'static str>,

    /// Suggestion for fixing the parser error.
    pub help: Option<&'static str>,

    // Specific error kind for the error.
    pub kind: ErrorKind,
}

// Implement Display for ErrorKind manually bacause [`marked_yaml::LoadError`] does not implement
// the way we want it.
// CAUTION: Because of this impl, we cannot use `#[error()]` on the enum.
impl fmt::Display for ErrorKind {
    #[allow(deprecated)] // for e.description()
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use marked_yaml::LoadError;
        use std::error::Error;

        match self {
            ErrorKind::YamlParsing(err) => {
                write!(f, "Failed to parse YAML: ")?;
                match err {
                    LoadError::TopLevelMustBeMapping(_) => {
                        write!(f, "Top level must be a mapping.")
                    }
                    LoadError::UnexpectedAnchor(_) => write!(f, "Unexpected definition of anchor."),
                    LoadError::MappingKeyMustBeScalar(_) => {
                        write!(f, "Keys in mappings must be scalar.")
                    }
                    LoadError::UnexpectedTag(_) => write!(f, "Unexpected use of YAML tag."),
                    LoadError::ScanError(_, e) => {
                        // e.description() is deprecated but it's the only way to get
                        // the exact info we want out of yaml-rust

                        write!(f, "{}", e.description())
                    }
                }
            }
            ErrorKind::ExpectedMapping => write!(f, "Expected a mapping."),
            ErrorKind::ExpectedScalar => write!(f, "Expected a scalar value."),
            ErrorKind::ExpectedSequence => write!(f, "Expected a sequence."),
            ErrorKind::IfSelectorConditionNotScalar => {
                write!(f, "Condition in `if` selector must be a scalar.")
            }
            ErrorKind::IfSelectorMissingThen => {
                write!(f, "Missing `then` field in the `if` selector.")
            }
            ErrorKind::JinjaRendering(err) => {
                write!(f, "Failed to render Jinja expression: {}", err.kind())
            }
            ErrorKind::IfSelectorConditionNotBool(err) => {
                write!(f, "Condition in `if` selector must be a boolean: {}", err)
            }
            ErrorKind::Other => write!(f, "An unspecified error occurred."),
        }
    }
}

/// Macro to facilitate the creation of [`Error`]s.
#[macro_export]
#[doc(hidden)]
macro_rules! _error {
    ($src:expr, $span:expr, $kind:expr $(,)?) => {{
        $crate::recipe::error::ParsingError {
            src: $src.to_owned(),
            span: $span,
            label: None,
            help: None,
            kind: $kind,
        }
    }};
    ($src:expr, $span:expr, $kind:expr, label = $label:expr $(,)?) => {{
        $crate::recipe::error::ParsingError {
            src: $src.to_owned(),
            span: $span,
            label: Some($label),
            help: None,
            kind: $kind,
        }
    }};
    ($src:expr, $span:expr, $kind:expr, help = $help:expr $(,)?) => {{
        $crate::recipe::error::ParsingError {
            src: $src.to_owned(),
            span: $span,
            label: None,
            help: Some($help),
            kind: $kind,
        }
    }};
    ($src:expr, $span:expr, $kind:expr, label = $label:expr, help = $help:expr $(,)?) => {{
        $crate::_error!($src, $span, $kind, $label, $help)
    }};
    ($src:expr, $span:expr, $kind:expr, help = $help:expr, label = $label:expr $(,)?) => {{
        $crate::_error!($src, $span, $kind, $label, $help)
    }};
    ($src:expr, $span:expr, $kind:expr, $label:expr, $help:expr $(,)?) => {{
        $crate::recipe::error::ParsingError {
            src: $src.to_owned(),
            span: $span,
            label: Some($label),
            help: Some($help),
            kind: $kind,
        }
    }};
}

/// Macro to facilitate the creation of [`Error`]s.
#[macro_export]
#[doc(hidden)]
macro_rules! _partialerror {
    ($span:expr, $kind:expr $(,)?) => {{
        $crate::recipe::error::PartialParsingError {
            span: $span,
            label: None,
            help: None,
            kind: $kind,
        }
    }};
    ($span:expr, $kind:expr, label = $label:expr $(,)?) => {{
        $crate::recipe::error::PartialParsingError {
            span: $span,
            label: Some($label),
            help: None,
            kind: $kind,
        }
    }};
    ($span:expr, $kind:expr, help = $help:expr $(,)?) => {{
        $crate::recipe::error::PartialParsingError {
            span: $span,
            label: None,
            help: Some($help),
            kind: $kind,
        }
    }};
    ($span:expr, $kind:expr, label = $label:expr, help = $help:expr $(,)?) => {{
        $crate::_error!($src, $span, $kind, $label, $help)
    }};
    ($span:expr, $kind:expr, help = $help:expr, label = $label:expr $(,)?) => {{
        $crate::_error!($src, $span, $kind, $label, $help)
    }};
    ($span:expr, $kind:expr, $label:expr, $help:expr $(,)?) => {{
        $crate::recipe::error::PartialParsingError {
            span: $span,
            label: Some($label),
            help: Some($help),
            kind: $kind,
        }
    }};
}

/// Error handler for [`marked_yaml::LoadError`].
pub(crate) fn load_error_handler(src: &str, err: marked_yaml::LoadError) -> ParsingError {
    _error!(
        src,
        marker2span(src, marker(&err)),
        ErrorKind::YamlParsing(err),
        label = match err {
            marked_yaml::LoadError::TopLevelMustBeMapping(_) => "expected a mapping here",
            marked_yaml::LoadError::UnexpectedAnchor(_) => "unexpected anchor here",
            marked_yaml::LoadError::UnexpectedTag(_) => "unexpected tag here",
            _ => "here",
        }
    )
}

/// Convert a [`marked_yaml::Marker`] to a [`SourceSpan`].
#[inline(always)]
pub(crate) fn marker2span(src: &str, mark: marked_yaml::Marker) -> SourceSpan {
    let start = SourceOffset::from_location(src, mark.line(), mark.column());

    SourceSpan::new(
        start,
        SourceOffset::from(find_length(src, start)), //::from_location(src, mark.line(), mark.column()),
    )
}

/// Get the [`marked_yaml::Marker`] from a [`marked_yaml::LoadError`].
#[inline(always)]
pub(crate) fn marker(err: &marked_yaml::LoadError) -> marked_yaml::Marker {
    use marked_yaml::LoadError::*;
    match err {
        TopLevelMustBeMapping(m) => *m,
        UnexpectedAnchor(m) => *m,
        MappingKeyMustBeScalar(m) => *m,
        UnexpectedTag(m) => *m,
        ScanError(m, _) => *m,
    }
}

/// Convert a [`marked_yaml::Span`] to a [`SourceSpan`].
pub(crate) fn markerspan2span(src: &str, span: marked_yaml::Span) -> SourceSpan {
    let marked_start = span
        .start()
        .copied()
        .unwrap_or_else(|| marked_yaml::Marker::new(usize::MAX, usize::MAX, usize::MAX));
    let marked_end = span.end().copied();

    let start = SourceOffset::from_location(src, marked_start.line(), marked_start.column());

    let length = match marked_end {
        Some(end) => {
            let end = SourceOffset::from_location(src, end.line() - 1, end.column());
            end.offset() - start.offset() - 1
        }
        None => {
            let l = find_length(src, start);
            if l == 0 {
                1
            } else {
                l
            }
        }
    };

    SourceSpan::new(start, SourceOffset::from(length))
}

#[allow(dead_code)]
pub(crate) fn marker2offset(src: &str, mark: marked_yaml::Marker) -> SourceOffset {
    SourceOffset::from_location(src, mark.line(), mark.column())
}

/// Find the length of the token string starting at the `start` [`SourceOffset`].
pub(crate) fn find_length(src: &str, start: SourceOffset) -> usize {
    let start = start.offset();
    let mut end = 0;

    for (i, c) in src[start..].char_indices() {
        if c.is_whitespace() {
            end += i;
            break;
        }
    }

    end
}

#[cfg(test)]
mod tests {

    // use super::*;
    // use crate::recipe::stage1::RawRecipe;

    // #[test]
    // fn miette_output() -> miette::Result<()> {
    //     let fault_yaml = r#"
    //     context:
    //         - a
    //         - b
    //     package:
    //         name: test
    //         version: 0.1.0
    //     "#;

    //     let err = RawRecipe::from_yaml(fault_yaml)?;

    //     Ok(())
    // }
}
