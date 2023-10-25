use std::{borrow::Cow, convert::Infallible, fmt, str::ParseBoolError};

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
    #[label("{}", label.as_deref().unwrap_or("here"))]
    pub span: SourceSpan,

    /// Label text for this span. Defaults to `"here"`.
    pub label: Option<Cow<'static, str>>,

    /// Suggestion for fixing the parser error.
    #[help]
    pub help: Option<Cow<'static, str>>,

    // Specific error kind for the error.
    pub kind: ErrorKind,
}

impl ParsingError {
    pub fn from_partial(src: &str, err: PartialParsingError) -> Self {
        Self {
            src: src.to_owned(),
            span: marker_span_to_span(src, err.span),
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
    #[diagnostic(code(error::yaml_parsing))]
    YamlParsing(#[from] marked_yaml::LoadError),

    /// Error when expected mapping but got something else.
    #[diagnostic(code(error::expected_mapping))]
    ExpectedMapping,

    /// Error when expected scalar but got something else.
    #[diagnostic(code(error::expected_scalar))]
    ExpectedScalar,

    /// Error when expected sequence but got something else.
    #[diagnostic(code(error::expected_sequence))]
    ExpectedSequence,

    /// Error when if-selector condition is not a scalar.
    #[diagnostic(code(error::if_selector_condition_not_scalar))]
    IfSelectorConditionNotScalar,

    /// Error when if selector is missing a `then` field.
    #[diagnostic(code(error::if_selector_missing_then))]
    IfSelectorMissingThen,

    /// Error when invalid MD5 hash.
    #[diagnostic(code(error::invalid_md5))]
    InvalidMd5,

    /// Error when invalid SHA256 hash.
    #[diagnostic(code(error::invalid_sha256))]
    InvalidSha256,

    /// Error when there is a required missing field in a mapping.
    #[diagnostic(code(error::missing_field))]
    MissingField(Cow<'static, str>),

    /// Error when there is a invalid field in a mapping.
    #[diagnostic(code(error::invalid_field))]
    InvalidField(Cow<'static, str>),

    /// Error rendering a Jinja expression.
    #[diagnostic(code(error::jinja_rendering))]
    JinjaRendering(#[from] minijinja::Error),

    /// Error processing the condition of a if-selector.
    #[diagnostic(code(error::if_selector_condition_not_bool))]
    IfSelectorConditionNotBool(#[from] ParseBoolError),

    /// Error when processing URL
    #[diagnostic(code(error::url_parsing))]
    UrlParsing(#[from] url::ParseError),

    /// Error when parsing a integer.
    #[diagnostic(code(error::integer_parsing))]
    IntegerParsing(#[from] std::num::ParseIntError),

    /// Error when parsing a SPDX license.
    #[diagnostic(code(error::spdx_parsing))]
    SpdxParsing(#[from] spdx::ParseError),

    /// Error when parsing a [`MatchSpec`](rattler_conda_types::MatchSpec).
    #[diagnostic(code(error::match_spec_parsing))]
    MatchSpecParsing(#[from] rattler_conda_types::ParseMatchSpecError),

    /// Error when parsing a [`PackageName`](rattler_conda_types::PackageName).
    #[diagnostic(code(error::package_name_parsing))]
    PackageNameParsing(#[from] rattler_conda_types::InvalidPackageNameError),

    /// Error when parsing a [`EntryPoint`](rattler_conda_types::package::EntryPoint).
    #[diagnostic(code(error::entry_point_parsing))]
    EntryPointParsing(String),

    /// Generic unspecified error. If this is returned, the call site should
    /// be annotated with context, if possible.
    #[diagnostic(code(error::other))]
    Other,
}

/// Partial error type, almost the same as the [`ParsingError`] but without the source string.
///
/// This is to use on the context where you want to produce a [`ParsingError`] but you don't have
/// the source string, or including the source string would involve more complexity to handle. Like
/// leveraging traits, simple conversions, etc.
///
/// Examples of this is [`Node`](crate::recipe::custom_yaml::Node) to implement [`TryFrom`] for some
/// types.
#[derive(Debug, Error)]
#[error("Parsing: {kind}")]
pub struct PartialParsingError {
    /// Offset in chars of the error.
    pub span: marked_yaml::Span,

    /// Label text for this span. Defaults to `"here"`.
    pub label: Option<Cow<'static, str>>,

    /// Suggestion for fixing the parser error.
    pub help: Option<Cow<'static, str>>,

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
            ErrorKind::YamlParsing(LoadError::TopLevelMustBeMapping(_)) => {
                write!(f, "failed to parse YAML: top level must be a mapping.")
            }
            ErrorKind::YamlParsing(LoadError::UnexpectedAnchor(_)) => {
                write!(f, "failed to parse YAML: unexpected definition of anchor.")
            }
            ErrorKind::YamlParsing(LoadError::MappingKeyMustBeScalar(_)) => {
                write!(f, "failed to parse YAML: keys in mappings must be scalar.")
            }
            ErrorKind::YamlParsing(LoadError::UnexpectedTag(_)) => {
                write!(f, "failed to parse YAML: unexpected use of YAML tag.")
            }
            ErrorKind::YamlParsing(LoadError::ScanError(_, e)) => {
                // e.description() is deprecated but it's the only way to get
                // the exact info we want out of yaml-rust

                write!(f, "failed to parse YAML: {}", e.description())
            }
            ErrorKind::ExpectedMapping => write!(f, "expected a mapping."),
            ErrorKind::ExpectedScalar => write!(f, "expected a scalar value."),
            ErrorKind::ExpectedSequence => write!(f, "expected a sequence."),
            ErrorKind::IfSelectorConditionNotScalar => {
                write!(f, "condition in `if` selector must be a scalar.")
            }
            ErrorKind::IfSelectorMissingThen => {
                write!(f, "missing `then` field in the `if` selector.")
            }
            ErrorKind::InvalidMd5 => write!(f, "invalid MD5 checksum."),
            ErrorKind::InvalidSha256 => write!(f, "invalid SHA256 checksum."),
            ErrorKind::InvalidField(s) => write!(f, "invalid field `{s}`."),
            ErrorKind::MissingField(s) => write!(f, "missing field `{s}`"),
            ErrorKind::JinjaRendering(err) => {
                write!(f, "failed to render Jinja expression: {}", err.kind())
            }
            ErrorKind::IfSelectorConditionNotBool(err) => {
                write!(f, "condition in `if` selector must be a boolean: {}", err)
            }
            ErrorKind::UrlParsing(err) => write!(f, "failed to parse URL: {}", err),
            ErrorKind::IntegerParsing(err) => write!(f, "failed to parse integer: {}", err),
            ErrorKind::SpdxParsing(err) => {
                write!(f, "failed to parse SPDX license: {}", err.reason)
            }
            ErrorKind::MatchSpecParsing(err) => {
                write!(f, "failed to parse match spec: {}", err)
            }
            ErrorKind::PackageNameParsing(err) => {
                write!(f, "failed to parse package name: {}", err)
            }
            ErrorKind::EntryPointParsing(err) => {
                write!(f, "failed to parse entry point: {}", err)
            }
            ErrorKind::Other => write!(f, "an unspecified error occurred."),
        }
    }
}

impl From<Infallible> for ErrorKind {
    fn from(_: Infallible) -> Self {
        Self::Other
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
            label: Some($label.into()),
            help: None,
            kind: $kind,
        }
    }};
    ($src:expr, $span:expr, $kind:expr, help = $help:expr $(,)?) => {{
        $crate::recipe::error::ParsingError {
            src: $src.to_owned(),
            span: $span,
            label: None,
            help: Some($help.into()),
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
            label: Some($label.into()),
            help: Some($help.into()),
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
            label: Some($label.into()),
            help: None,
            kind: $kind,
        }
    }};
    ($span:expr, $kind:expr, help = $help:expr $(,)?) => {{
        $crate::recipe::error::PartialParsingError {
            span: $span,
            label: None,
            help: Some($help.into()),
            kind: $kind,
        }
    }};
    ($span:expr, $kind:expr, label = $label:expr, help = $help:expr $(,)?) => {{
        $crate::_partialerror!($span, $kind, $label, $help)
    }};
    ($span:expr, $kind:expr, help = $help:expr, label = $label:expr $(,)?) => {{
        $crate::_partialerror!($span, $kind, $label, $help)
    }};
    ($span:expr, $kind:expr, $label:expr, $help:expr $(,)?) => {{
        $crate::recipe::error::PartialParsingError {
            span: $span,
            label: Some($label.into()),
            help: Some($help.into()),
            kind: $kind,
        }
    }};
}

/// Error handler for [`marked_yaml::LoadError`].
pub(super) fn load_error_handler(src: &str, err: marked_yaml::LoadError) -> ParsingError {
    _error!(
        src,
        marker_to_span(src, marker(&err)),
        ErrorKind::YamlParsing(err),
        label = Cow::Borrowed(match err {
            marked_yaml::LoadError::TopLevelMustBeMapping(_) => "expected a mapping here",
            marked_yaml::LoadError::UnexpectedAnchor(_) => "unexpected anchor here",
            marked_yaml::LoadError::UnexpectedTag(_) => "unexpected tag here",
            _ => "here",
        })
    )
}

/// Convert a [`marked_yaml::Marker`] to a [`SourceSpan`].
pub(super) fn marker_to_span(src: &str, mark: marked_yaml::Marker) -> SourceSpan {
    let start = SourceOffset::from_location(src, mark.line(), mark.column());

    SourceSpan::new(
        start,
        SourceOffset::from(find_length(src, start)), //::from_location(src, mark.line(), mark.column()),
    )
}

/// Get the [`marked_yaml::Marker`] from a [`marked_yaml::LoadError`].
pub(super) fn marker(err: &marked_yaml::LoadError) -> marked_yaml::Marker {
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
pub(super) fn marker_span_to_span(src: &str, span: marked_yaml::Span) -> SourceSpan {
    let marked_start = span
        .start()
        .copied()
        .unwrap_or_else(|| marked_yaml::Marker::new(usize::MAX, usize::MAX, usize::MAX));
    let marked_end = span.end().copied();

    let start = SourceOffset::from_location(src, marked_start.line(), marked_start.column());

    let length = match marked_end {
        Some(m_end) => {
            if marked_start.line() == m_end.line() {
                m_end.column() - marked_start.column() + 1
            } else {
                let end = SourceOffset::from_location(src, m_end.line(), m_end.column() - 1);

                let mut end_offset = end.offset();
                while src[end_offset..].chars().next().unwrap().is_whitespace() {
                    end_offset -= 1;
                }

                end_offset - start.offset() + 1
            }
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
pub(super) fn marker_to_offset(src: &str, mark: marked_yaml::Marker) -> SourceOffset {
    SourceOffset::from_location(src, mark.line(), mark.column())
}

/// Find the length of the token string starting at the `start` [`SourceOffset`].
pub(super) fn find_length(src: &str, start: SourceOffset) -> usize {
    let start = start.offset();
    let mut end = 0;

    let mut iter = src[start..].char_indices();

    // FIXME: Implement `"`, `'` and `[` open and close detection.
    while let Some((i, c)) = iter.next() {
        if c == ':' {
            if let Some((_, c)) = iter.next() {
                if c == '\n' || c == '\r' || c == '\t' || c == ' ' {
                    end += i;
                    break;
                }
            }
        }

        if c == '\n' || c == '\r' || c == '\t' {
            end += i;
            break;
        }
    }

    end
}

#[cfg(test)]
mod tests {

    use crate::{assert_miette_snapshot, recipe::Recipe};

    #[test]
    fn miette_output() {
        let fault_yaml = r#"
            context:
                - a
                - b
            package:
                name: test
                version: 0.1.0
            "#;
        let res = Recipe::from_yaml(fault_yaml, Default::default());

        if let Err(err) = res {
            assert_miette_snapshot!(err);
        }
    }
}
