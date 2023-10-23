use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use spdx::Expression;
use url::Url;

use crate::recipe::custom_yaml::HasSpan;
use crate::recipe::stage2::Render;
use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{Node, SequenceNodeInternal},
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1,
    },
};

/// About information.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct About {
    homepage: Option<Url>,
    repository: Option<Url>,
    documentation: Option<Url>,
    license: Option<License>,
    license_family: Option<String>,
    license_files: Vec<String>,
    license_url: Option<Url>,
    summary: Option<String>,
    description: Option<String>,
    prelink_message: Option<String>,
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

impl About {
    pub(super) fn from_stage1(
        about: &stage1::About,
        jinja: &Jinja,
    ) -> Result<Self, PartialParsingError> {
        let homepage = about.homepage.render(jinja, "homepage")?;
        let repository = about.repository.render(jinja, "repository")?;
        let documentation = about.documentation.render(jinja, "documentation")?;
        let license = about.license.render(jinja, "license")?;
        let license_family = about.license_family.render(jinja, "license_family")?;
        let license_url = about.license_url.render(jinja, "license_url")?;
        let license_files = about
            .license_file
            .as_ref()
            .map(|node| parse_license_files(node, jinja))
            .transpose()?
            .unwrap_or_default();
        let summary = about.summary.render(jinja, "summary")?;
        let description = about.description.render(jinja, "description")?;
        let prelink_message = about.prelink_message.render(jinja, "prelink_message")?;

        Ok(Self {
            homepage,
            repository,
            documentation,
            license,
            license_family,
            license_files,
            license_url,
            summary,
            description,
            prelink_message,
        })
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

fn parse_license_files(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let script = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `script`"
                )
            })?;
            Ok(vec![script])
        }
        Node::Sequence(seq) => {
            let mut scripts = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        scripts.extend(parse_license_files(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            scripts.extend(parse_license_files(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(scripts)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
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
