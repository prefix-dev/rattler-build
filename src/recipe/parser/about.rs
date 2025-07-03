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
    validate_keys,
};

use super::{FlattenErrors, GlobVec};

/// About information.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct About {
    /// The homepage of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<Url>,
    /// The repository of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<Url>,
    /// The documentation of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<Url>,
    /// The license of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,
    /// The license family of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_family: Option<String>,
    /// The license file(s) of the package.
    #[serde(default, skip_serializing_if = "GlobVec::is_empty")]
    pub license_file: GlobVec,
    /// The summary of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The description of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The prelink message of the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prelink_message: Option<String>,
}

impl About {
    /// Returns true if the about has its default configuration.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl TryConvertNode<About> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<About, Vec<PartialParsingError>> {
        self.as_mapping()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedMapping,)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<About> for RenderedMappingNode {
    fn try_convert(&self, _name: &str) -> Result<About, Vec<PartialParsingError>> {
        let mut about = About::default();

        validate_keys!(
            about,
            self.iter(),
            homepage,
            repository,
            documentation,
            license,
            license_family,
            license_file,
            summary,
            description,
            prelink_message
        );

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
    fn try_convert(&self, name: &str) -> Result<License, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedScalar,)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<License> for RenderedScalarNode {
    fn try_convert(&self, name: &str) -> Result<License, Vec<PartialParsingError>> {
        let original: String = self.try_convert(name)?;
        let expr = Expression::parse(original.as_str())
            .map_err(|err| vec![_partialerror!(*self.span(), ErrorKind::from(err),)])?;

        Ok(License { original, expr })
    }
}

#[cfg(test)]
mod test {
    use crate::{
        assert_miette_snapshot,
        recipe::{Recipe, jinja::SelectorConfig},
        variant_config::ParseErrors,
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

        let err: ParseErrors<_> = Recipe::from_yaml(recipe, SelectorConfig::default())
            .unwrap_err()
            .into();

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

        let err: ParseErrors<_> = Recipe::from_yaml(recipe, SelectorConfig::default())
            .unwrap_err()
            .into();

        assert_miette_snapshot!(err);
    }
}
