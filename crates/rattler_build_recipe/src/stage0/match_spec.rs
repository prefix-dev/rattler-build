use rattler_conda_types::{
    MatchSpec, ParseMatchSpecError, ParseMatchSpecOptions, ParseStrictness, RepodataRevision,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt::Display, str::FromStr};

// Wrapper for MatchSpec to enable serde support
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SerializableMatchSpec(pub MatchSpec);

impl SerializableMatchSpec {
    pub(crate) fn parse_with_v3(
        s: &str,
        strictness: ParseStrictness,
        v3: bool,
    ) -> Result<Self, ParseMatchSpecError> {
        MatchSpec::from_str(s, matchspec_parse_options(strictness, v3)).map(Self)
    }
}

pub(crate) fn matchspec_parse_options(
    strictness: ParseStrictness,
    v3: bool,
) -> ParseMatchSpecOptions {
    let revision = if v3 {
        RepodataRevision::V3
    } else {
        RepodataRevision::Legacy
    };
    ParseMatchSpecOptions::new(strictness).with_repodata_revision(revision)
}

impl Serialize for SerializableMatchSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for SerializableMatchSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        MatchSpec::from_str(&s, matchspec_parse_options(ParseStrictness::Strict, false))
            .map(SerializableMatchSpec)
            .map_err(serde::de::Error::custom)
    }
}

impl From<MatchSpec> for SerializableMatchSpec {
    fn from(spec: MatchSpec) -> Self {
        SerializableMatchSpec(spec)
    }
}

impl From<&str> for SerializableMatchSpec {
    fn from(s: &str) -> Self {
        SerializableMatchSpec(
            MatchSpec::from_str(s, matchspec_parse_options(ParseStrictness::Strict, false))
                .expect("Invalid MatchSpec"),
        )
    }
}

impl From<String> for SerializableMatchSpec {
    fn from(s: String) -> Self {
        SerializableMatchSpec(
            MatchSpec::from_str(&s, matchspec_parse_options(ParseStrictness::Strict, false))
                .expect("Invalid MatchSpec"),
        )
    }
}

impl Display for SerializableMatchSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for SerializableMatchSpec {
    type Err = ParseMatchSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MatchSpec::from_str(s, matchspec_parse_options(ParseStrictness::Strict, false))
            .map(SerializableMatchSpec)
    }
}
