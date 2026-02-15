use serde::{Deserialize, Serialize};

use super::Value;

/// App section for Anaconda Navigator app discovery
///
/// When present in a recipe, the package will be discoverable as an application
/// in Anaconda Navigator. The fields map to corresponding entries in index.json.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct App {
    /// Command to launch the application (maps to app_entry in index.json)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<Value<String>>,

    /// Icon filename for the application (maps to icon in index.json)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Value<String>>,

    /// Short description of the application (maps to summary in index.json)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Value<String>>,

    /// Application type: "web" or "desk" (maps to app_type in index.json)
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub app_type: Option<Value<String>>,
}

impl App {
    /// Collect all template variables used in this section
    pub fn used_variables(&self) -> std::collections::HashSet<String> {
        let mut vars = std::collections::HashSet::new();
        if let Some(v) = &self.entry {
            vars.extend(v.used_variables());
        }
        if let Some(v) = &self.icon {
            vars.extend(v.used_variables());
        }
        if let Some(v) = &self.summary {
            vars.extend(v.used_variables());
        }
        if let Some(v) = &self.app_type {
            vars.extend(v.used_variables());
        }
        vars
    }
}
