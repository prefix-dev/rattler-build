//! The recipe module contains all the logic to parse a recipe file.

pub use self::{error::ParsingError, jinja::Jinja, parser::Recipe};

pub mod parser;

pub mod custom_yaml;
pub mod error;
pub mod jinja;
pub mod variable;

/// A trait to render a certain stage1 node into its final type.
pub(crate) trait Render<T> {
    fn render(&self, jinja: &Jinja, name: &str) -> Result<T, Vec<error::PartialParsingError>>;
}

/// Assert a miette snapshot using insta.
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
