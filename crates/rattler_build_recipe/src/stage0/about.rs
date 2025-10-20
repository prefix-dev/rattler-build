use std::fmt::Display;
use std::str::FromStr;

use crate::stage0::types::{ConditionalList, Value};
use itertools::Itertools as _;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, PartialEq, Debug)]
pub struct License(pub spdx::Expression);

impl Serialize for License {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_ref())
    }
}

impl<'de> Deserialize<'de> for License {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let expr = s.parse().map_err(serde::de::Error::custom)?;
        Ok(License(expr))
    }
}

impl FromStr for License {
    type Err = spdx::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<spdx::Expression>().map(License)
    }
}

impl Display for License {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct About {
    pub homepage: Option<Value<Url>>,
    pub license: Option<Value<License>>,
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub license_file: ConditionalList<String>,
    /// License family (deprecated, but still used in some recipes)
    pub license_family: Option<Value<String>>,
    pub summary: Option<Value<String>>,
    pub description: Option<Value<String>>,
    pub documentation: Option<Value<Url>>,
    pub repository: Option<Value<Url>>,
}

impl Display for About {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "About {{ homepage: {}, license: {}, license_file: [{}], summary: {}, description: {}, documentation: {}, repository: {} }}",
            self.homepage.as_ref().into_iter().format(", "),
            self.license.as_ref().into_iter().format(", "),
            self.license_file.iter().format(", "),
            self.summary.as_ref().into_iter().format(", "),
            self.description.as_ref().into_iter().format(", "),
            self.documentation.as_ref().into_iter().format(", "),
            self.repository.as_ref().into_iter().format(", ")
        )
    }
}

impl About {
    pub fn used_variables(&self) -> Vec<String> {
        let About {
            homepage,
            license,
            license_file,
            license_family,
            summary,
            description,
            documentation,
            repository,
        } = self;

        let mut vars = Vec::new();
        if let Some(homepage) = homepage {
            vars.extend(homepage.used_variables());
        }
        if let Some(license) = license {
            vars.extend(license.used_variables());
        }
        vars.extend(license_file.used_variables());
        if let Some(license_family) = license_family {
            vars.extend(license_family.used_variables());
        }
        if let Some(summary) = summary {
            vars.extend(summary.used_variables());
        }
        if let Some(description) = description {
            vars.extend(description.used_variables());
        }
        if let Some(documentation) = documentation {
            vars.extend(documentation.used_variables());
        }
        if let Some(repository) = repository {
            vars.extend(repository.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
