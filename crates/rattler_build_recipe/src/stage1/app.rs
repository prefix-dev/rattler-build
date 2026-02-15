use serde::{Deserialize, Serialize};

/// Evaluated app section for Anaconda Navigator app discovery
///
/// This is the Stage 1 (evaluated) version of the app section.
/// All Jinja templates have been resolved to concrete string values.
///
/// When present, the following fields are written to index.json:
/// - `app_entry`: command to launch the app
/// - `app_type`: "web" or "desk"
/// - `icon`: icon filename
/// - `summary`: short description
/// - `type`: set to "app"
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct App {
    /// Command to launch the application
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,

    /// Icon filename for the application
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Short description of the application
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Application type: "web" or "desk"
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub app_type: Option<String>,
}

impl App {
    /// Returns true if all fields are None
    pub fn is_empty(&self) -> bool {
        self.entry.is_none()
            && self.icon.is_none()
            && self.summary.is_none()
            && self.app_type.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_is_empty() {
        let app = App::default();
        assert!(app.is_empty());

        let app = App {
            entry: Some("myapp launch".to_string()),
            ..Default::default()
        };
        assert!(!app.is_empty());
    }
}
