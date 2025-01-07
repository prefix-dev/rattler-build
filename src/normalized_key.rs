use rattler_conda_types::PackageName;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// A key in a variant configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct NormalizedKey(pub String);

impl NormalizedKey {
    /// Returns the normalized form of the key.
    pub fn normalize(&self) -> String {
        self.0
            .chars()
            .map(|c| match c {
                '-' | '_' | '.' => '_',
                x => x,
            })
            .collect()
    }
}

impl Serialize for NormalizedKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.normalize().serialize(serializer)
    }
}

impl Hash for NormalizedKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.normalize().hash(state)
    }
}

impl PartialEq for NormalizedKey {
    fn eq(&self, other: &Self) -> bool {
        self.normalize() == other.normalize()
    }
}

impl Eq for NormalizedKey {}

impl PartialOrd for NormalizedKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NormalizedKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.normalize().cmp(&other.normalize())
    }
}

// For convenience, implement From<String> and From<&str>
impl From<String> for NormalizedKey {
    fn from(s: String) -> Self {
        NormalizedKey(s)
    }
}

impl From<&str> for NormalizedKey {
    fn from(s: &str) -> Self {
        NormalizedKey(s.to_string())
    }
}

impl From<&PackageName> for NormalizedKey {
    fn from(p: &PackageName) -> Self {
        p.as_normalized().into()
    }
}
