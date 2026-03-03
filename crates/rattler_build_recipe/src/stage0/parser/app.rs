use crate::stage0::App;
use rattler_build_yaml_parser::ParseMapping;

use super::{MarkedNode, ParseResult};

/// Parse the `app` section of a recipe
pub fn parse_app(yaml: &MarkedNode) -> ParseResult<App> {
    yaml.validate_keys("app", &["entry", "icon", "summary", "type"])?;

    Ok(App {
        entry: yaml.try_get_field("entry")?,
        icon: yaml.try_get_field("icon")?,
        summary: yaml.try_get_field("summary")?,
        app_type: yaml.try_get_field("type")?,
    })
}
