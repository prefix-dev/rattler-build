use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use spdx::Expression;
use url::Url;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{
            HasSpan, RenderedMappingNode, RenderedNode, RenderedScalarNode, TryConvertNode,
        },
        error::{ErrorKind, PartialParsingError},
    },
};

/// About information.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct About {
    #[serde(skip_serializing_if = "Option::is_none")]
    homepage: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<License>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license_family: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    license_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license_url: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prelink_message: Option<String>,
}

impl About {
    /// Returns true if the about has its default configuration.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Get the homepage.
    pub const fn homepage(&self) -> Option<&Url> {
        self.homepage.as_ref()
    }

    /// Get the repository.
    pub const fn repository(&self) -> Option<&Url> {
        self.repository.as_ref()
    }

    /// Get the documentation.
    pub const fn documentation(&self) -> Option<&Url> {
        self.documentation.as_ref()
    }

    /// Get the license.
    pub fn license(&self) -> Option<&License> {
        self.license.as_ref()
    }

    /// Get the license family.
    pub fn license_family(&self) -> Option<&str> {
        self.license_family.as_deref()
    }

    /// Get the license file.
    pub fn license_files(&self) -> &[String] {
        self.license_files.as_slice()
    }

    /// Get the license url.
    pub const fn license_url(&self) -> Option<&Url> {
        self.license_url.as_ref()
    }

    /// Get the summary.
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// Get the description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Get the prelink message.
    pub fn prelink_message(&self) -> Option<&str> {
        self.prelink_message.as_deref()
    }
}

impl TryConvertNode<About> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<About, PartialParsingError> {
        self.as_mapping()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedMapping,))
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<About> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<About, PartialParsingError> {
        let mut about = About::default();
        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "homepage" => about.homepage = value.try_convert(key_str)?,
                "repository" => {
                    about.repository = value.try_convert(key_str)?
                }
                "documentation" => {
                    about.documentation = value.try_convert(key_str)?
                }
                "license" => about.license = value.try_convert(key_str)?,
                "license_family" => {
                    about.license_family = value.try_convert(key_str)?
                }
                "license_file" => about.license_files = value.try_convert(key_str)?,
                "license_url" => about.license_url = value.try_convert(key_str)?,
                "summary" => about.summary = value.try_convert(key_str)?,
                "description" => about.description = value.try_convert(key_str)?,
                "prelink_message" => {
                    about.prelink_message = value.try_convert(key_str)?
                }
                invalid_key => {
                    return Err(_partialerror!(
                        *key.span(),
                        ErrorKind::InvalidField(invalid_key.to_string().into()),
                        help = format!("expected for `{name}` one of `homepage`, `repository`, `documentation`, `license`, `license_family`, `license_file`, `license_url`, `summary`, `description` or `prelink_message`")
                    ))
                }
            }
        }

        Ok(about)
    }
}

/// A parsed SPDX license
#[derive(Debug, Clone, SerializeDisplay, DeserializeFromStr)]
pub struct License {
    pub original: String,
    pub expr: spdx::Expression,
}

impl PartialEq for License {
    fn eq(&self, other: &Self) -> bool {
        self.expr == other.expr
    }
}

impl Display for License {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.original)
    }
}

impl FromStr for License {
    type Err = spdx::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(License {
            original: s.to_owned(),
            expr: Expression::parse(s)?,
        })
    }
}

impl TryConvertNode<License> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<License, PartialParsingError> {
        self.as_scalar()
            .ok_or_else(|| _partialerror!(*self.span(), ErrorKind::ExpectedScalar,))
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<License> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<License, PartialParsingError> {
        let original: String = self.try_convert(name)?;
        let expr = Expression::parse(original.as_str())
            .map_err(|err| _partialerror!(*self.span(), ErrorKind::from(err),))?;

        Ok(License { original, expr })
    }
}

#[cfg(test)]
mod test {
    use crate::{
        assert_miette_snapshot,
        recipe::{jinja::SelectorConfig, Recipe},
    };

    #[test]
    fn invalid_url() {
        let recipe = r#"
        package:
            name: test
            version: 0.0.1

        about:
            homepage: license_urla.asda:://sdskd
        "#;

        let err = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap_err();

        assert_miette_snapshot!(err);
    }

    #[test]
    fn invalid_license() {
        let recipe = r#"
        package:
            name: test
            version: 0.0.1

        about:
            license: MIT/X derivate
        "#;

        let err = Recipe::from_yaml(recipe, SelectorConfig::default()).unwrap_err();

        assert_miette_snapshot!(err);
    }
}
