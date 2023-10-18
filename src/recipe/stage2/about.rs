use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    _partialerror,
    recipe::{
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1::{self, node::SequenceNodeInternal, Node},
    },
};

/// About information.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct About {
    homepage: Option<Url>,
    repository: Option<Url>,
    documentation: Option<Url>,
    license: Option<String>,
    license_family: Option<String>,
    license_files: Vec<String>,
    license_url: Option<Url>,
    summary: Option<String>,
    description: Option<String>,
    prelink_message: Option<String>,
}

impl About {
    pub(super) fn from_stage1(
        about: &stage1::About,
        jinja: &Jinja,
    ) -> Result<Self, PartialParsingError> {
        let homepage = about
            .homepage
            .as_ref()
            .and_then(|n| n.as_scalar())
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.homepage.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering homepage"
                )
            })?
            .map(|url| Url::from_str(&url).unwrap());
        let repository = about
            .repository
            .as_ref()
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.repository.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering repository"
                )
            })?
            .map(|url| Url::from_str(url.as_str()).unwrap());
        let documentation = about
            .documentation
            .as_ref()
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.repository.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering repository"
                )
            })?
            .map(|url| Url::from_str(url.as_str()).unwrap());
        let license = about.license.as_ref().map(|s| s.as_str().to_owned());
        let license_family = about.license_family.as_ref().map(|s| s.as_str().to_owned());
        let license_url = about
            .license_url
            .as_ref()
            .map(|s| s.as_str().to_owned())
            .map(|url| Url::from_str(&url).unwrap());
        let license_files = about
            .license_file
            .as_ref()
            .map(|node| parse_license_files(node, jinja))
            .transpose()?
            .unwrap_or_default();
        let summary = about.summary.as_ref().map(|s| s.as_str().to_owned());
        let description = about.description.as_ref().map(|s| s.as_str().to_owned());
        let prelink_message = about
            .prelink_message
            .as_ref()
            .map(|s| jinja.render_str(s.as_str()))
            .transpose()
            .map_err(|err| {
                _partialerror!(
                    *about.prelink_message.as_ref().unwrap().span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering prelink_message"
                )
            })?;

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
    pub fn license(&self) -> Option<&str> {
        self.license.as_deref()
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
