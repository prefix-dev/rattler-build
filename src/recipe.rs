use std::{fmt, str::FromStr};

use crate::{_partialerror, recipe::error::ErrorKind};

use self::{
    custom_yaml::{HasSpan, ScalarNode, TryConvertNode},
    error::PartialParsingError,
};
pub use self::{error::ParsingError, jinja::Jinja, stage1::RawRecipe, stage2::Recipe};

pub mod stage1;
pub mod stage2;

pub mod custom_yaml;
pub mod error;
pub mod jinja;

/// A trait to render a certain stage1 node into its final type.
pub(crate) trait Render<T> {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<T, error::PartialParsingError>;
}

/// A trait to render a certain stage1 node into its final type.
pub(crate) trait OldRender<T> {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<T, PartialParsingError>;
}

/// A jinja rendered string
struct Rendered(String);

impl Rendered {
    // Parses this rendered value into another type.
    pub fn parse<F: FromStr>(&self) -> Result<F, F::Err> {
        FromStr::from_str(&self.0)
    }
}

impl<N: TryConvertNode<ScalarNode> + HasSpan> OldRender<Rendered> for N {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Rendered, PartialParsingError> {
        jinja
            .render_str(self.try_convert(name)?.as_str())
            .map_err(|err| {
                _partialerror!(
                    *self.span(),
                    ErrorKind::JinjaRendering(err),
                    label = format!("error rendering {name}")
                )
            })
            .map(Rendered)
    }
}

impl<N: TryConvertNode<ScalarNode> + HasSpan, T: FromStr> OldRender<T> for N
where
    T::Err: fmt::Display,
{
    fn render(&self, jinja: &Jinja, name: &str) -> Result<T, PartialParsingError> {
        match Rendered::parse(&self.render(jinja, name)?) {
            Ok(result) => Ok(result),
            Err(e) => Err(_partialerror!(
                *self.span(),
                ErrorKind::Other,
                label = e.to_string()
            )),
        }
    }
}

impl<N: OldRender<T>, T: FromStr> OldRender<Option<T>> for Option<N> {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<Option<T>, PartialParsingError> {
        match self {
            None => Ok(None),
            Some(node) => Ok(Some(node.render(jinja, name)?)),
        }
    }
}

#[cfg(test)]
#[cfg_attr(test, macro_export)]
macro_rules! assert_miette_snapshot {
    ($value:expr, @$snapshot:literal) => {{
        let mut value = String::new();
        ::miette::GraphicalReportHandler::new_themed(::miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut value, &$value)
            .unwrap();
        ::insta::assert_snapshot!(value, stringify!($value), @$snapshot);
    }};
    ($name:expr, $value:expr) => {{
        let mut value = String::new();
        ::miette::GraphicalReportHandler::new_themed(::miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut value, &$value)
            .unwrap();
        ::insta::assert_snapshot!(Some($name), value, stringify!($value));
    }};
    ($value:expr) => {{
        let mut value = String::new();
        ::miette::GraphicalReportHandler::new_themed(::miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut value, &$value)
            .unwrap();
        ::insta::assert_snapshot!(::insta::_macro_support::AutoName, value, stringify!($value));
    }};
}
