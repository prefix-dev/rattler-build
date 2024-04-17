use std::ops::Deref;

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedNode, RenderedScalarNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Wrapper around a regex that can be serialized and deserialized
#[derive(Debug, Clone)]
pub struct SerializableRegex(Regex);

impl Default for SerializableRegex {
    fn default() -> Self {
        SerializableRegex(Regex::new("").unwrap())
    }
}

impl Deref for SerializableRegex {
    type Target = Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryConvertNode<SerializableRegex> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<SerializableRegex, Vec<PartialParsingError>> {
        self.as_scalar()
            .ok_or_else(|| vec![_partialerror!(*self.span(), ErrorKind::ExpectedScalar)])
            .and_then(|m| m.try_convert(name))
    }
}

impl TryConvertNode<SerializableRegex> for RenderedScalarNode {
    fn try_convert(&self, _name: &str) -> Result<SerializableRegex, Vec<PartialParsingError>> {
        Ok(SerializableRegex(Regex::new(self.as_str()).map_err(
            |err| {
                vec![_partialerror!(
                    *self.span(),
                    ErrorKind::RegexParsing(err),
                    help = format!("expected a valid regex")
                )]
            },
        )?))
    }
}

impl Serialize for SerializableRegex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableRegex {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Regex::new(&s)
            .map(SerializableRegex)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {}
