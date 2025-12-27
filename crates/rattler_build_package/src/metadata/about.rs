//! AboutJson builder

use rattler_conda_types::package::AboutJson;
use serde_json::Value;
use std::collections::BTreeMap;

/// Builder for creating AboutJson metadata
///
/// # Example
/// ```rust
/// use rattler_build_package::metadata::AboutJsonBuilder;
///
/// let about = AboutJsonBuilder::new()
///     .with_homepage("https://example.com".to_string())
///     .with_license("MIT".to_string())
///     .with_summary("A test package".to_string())
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct AboutJsonBuilder {
    home: Vec<String>,
    license: Option<String>,
    license_family: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    doc_url: Vec<String>,
    dev_url: Vec<String>,
    channels: Vec<String>,
    extra: BTreeMap<String, Value>,
}

impl AboutJsonBuilder {
    /// Create a new AboutJsonBuilder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the homepage URL
    pub fn with_homepage(mut self, url: String) -> Self {
        self.home = vec![url];
        self
    }

    /// Set multiple homepage URLs
    pub fn with_homepages(mut self, urls: Vec<String>) -> Self {
        self.home = urls;
        self
    }

    /// Set the license
    pub fn with_license(mut self, license: String) -> Self {
        self.license = Some(license);
        self
    }

    /// Set the license family
    pub fn with_license_family(mut self, family: String) -> Self {
        self.license_family = Some(family);
        self
    }

    /// Set the summary
    pub fn with_summary(mut self, summary: String) -> Self {
        self.summary = Some(summary);
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add a documentation URL
    pub fn with_doc_url(mut self, url: String) -> Self {
        self.doc_url.push(url);
        self
    }

    /// Add a development URL
    pub fn with_dev_url(mut self, url: String) -> Self {
        self.dev_url.push(url);
        self
    }

    /// Add a channel
    pub fn with_channel(mut self, channel: String) -> Self {
        self.channels.push(channel);
        self
    }

    /// Add extra metadata
    pub fn with_extra(mut self, key: String, value: Value) -> Self {
        self.extra.insert(key, value);
        self
    }

    /// Build the AboutJson
    pub fn build(self) -> AboutJson {
        use url::Url;

        let parse_urls = |urls: Vec<String>| -> Vec<Url> {
            urls.into_iter()
                .filter_map(|s| Url::parse(&s).ok())
                .collect()
        };

        AboutJson {
            home: parse_urls(self.home),
            license: self.license,
            license_family: self.license_family,
            summary: self.summary,
            description: self.description,
            doc_url: parse_urls(self.doc_url),
            dev_url: parse_urls(self.dev_url),
            source_url: None,
            channels: self.channels,
            extra: self.extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_about_builder() {
        use url::Url;

        let about = AboutJsonBuilder::new()
            .with_homepage("https://example.com".to_string())
            .with_license("MIT".to_string())
            .with_summary("Test".to_string())
            .build();

        assert_eq!(about.home.len(), 1);
        assert_eq!(about.home[0], Url::parse("https://example.com").unwrap());
        assert_eq!(about.license, Some("MIT".to_string()));
        assert_eq!(about.summary, Some("Test".to_string()));
    }
}
