use rattler_conda_types::{MatchSpec, ParseMatchSpecError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt::Display, str::FromStr};

// Wrapper for MatchSpec to enable serde support
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SerializableMatchSpec(pub MatchSpec);

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
        MatchSpec::from_str(&s, rattler_conda_types::ParseStrictness::Strict)
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
            MatchSpec::from_str(s, rattler_conda_types::ParseStrictness::Strict)
                .expect("Invalid MatchSpec"),
        )
    }
}

impl From<String> for SerializableMatchSpec {
    fn from(s: String) -> Self {
        SerializableMatchSpec(
            MatchSpec::from_str(&s, rattler_conda_types::ParseStrictness::Strict)
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
        MatchSpec::from_str(s, rattler_conda_types::ParseStrictness::Strict)
            .map(SerializableMatchSpec)
    }
}
