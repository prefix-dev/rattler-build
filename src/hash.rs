//! Compute the build string / hash info for a given variant
use std::collections::{BTreeMap, HashMap};

use rattler_conda_types::NoArchType;
use serde::{Deserialize, Serialize};
use serde_json::ser::Formatter;
use sha1::{Digest, Sha1};

use crate::{normalized_key::NormalizedKey, recipe::variable::Variable};

/// A hash will be added if all of these are true for any dependency:
///
/// 1. package is an explicit dependency in build, host, or run deps
/// 2. package has a matching entry in conda_build_config.yaml which is a pin to a specific
///    version, not a lower bound
/// 3. that package is not ignored by ignore_version (not implemented yet)
///
/// The hash is computed based on the pinning value, NOT the build
///    dependency build string. This means hashes won't change as often,
///    but it also means that if run_exports is overly permissive,
///    software may break more often.
///
/// A hash will also ALWAYS be added when a compiler package is a build
///    or host dependency. Reasoning for that is that the compiler
///    package represents compiler flags and other things that can and do
///    dramatically change compatibility. It is much more risky to drop
///    this info (by dropping the hash) than it is for other software.
///
/// used variables - anything with a value in conda_build_config.yaml that applies to this
///    recipe.  Includes compiler if compiler jinja2 function is used.
///
/// This implements a formatter that uses the same formatting as
/// as the standard lib python `json.dumps()`
#[derive(Clone, Debug)]
struct PythonFormatter {}

impl Formatter for PythonFormatter {
    #[inline]
    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    #[inline]
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    #[inline]
    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }
}

// TODO merge with the jinja function that we have for this
fn short_version_from_spec(input: &str, length: u32) -> String {
    let mut parts = input.split('.');
    let mut result = String::new();
    for _ in 0..length {
        if let Some(part) = parts.next() {
            result.push_str(part);
        }
    }
    result
}

/// The hash info for a given variant
#[derive(Debug, PartialEq, Clone, Eq, Hash, Serialize, Deserialize)]
pub struct HashInfo {
    /// The hash (first 7 letters of the sha1sum)
    pub hash: String,

    /// The hash prefix (e.g. `py38` or `np111`)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
}

/// Represents the input to compute the hash
pub struct HashInput(String);

impl HashInput {
    /// Create a new hash input from a variant
    pub fn from_variant(variant: &BTreeMap<NormalizedKey, Variable>) -> Self {
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, PythonFormatter {});

        // BTree has sorted keys, which is important for hashing
        variant
            .serialize(&mut ser)
            .expect("Failed to serialize input");

        Self(String::from_utf8(buf).expect("Failed to convert to string"))
    }

    /// Get the hash input as a string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the hash input as bytes
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Display for HashInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}h{}", self.prefix, self.hash)
    }
}

impl HashInfo {
    fn hash_prefix(variant: &BTreeMap<NormalizedKey, Variable>, noarch: &NoArchType) -> String {
        if noarch.is_python() {
            return "py".to_string();
        }

        let mut map: HashMap<String, String> = HashMap::new();

        for (variant_key, version_spec) in variant.iter() {
            let prefix = match variant_key.normalize().as_str() {
                "numpy" => "np",
                "python" => "py",
                "perl" => "pl",
                "lua" => "lua",
                "r" => "r",
                _ => continue,
            };

            let version_length = match prefix {
                "pl" => 3,
                _ => 2,
            };

            map.insert(
                prefix.to_string(),
                short_version_from_spec(&version_spec.to_string(), version_length),
            );
        }

        let order = vec!["np", "py", "pl", "lua", "r", "mro"];
        let mut result = String::new();
        for key in order {
            if let Some(value) = map.get(key) {
                result.push_str(format!("{}{}", key, value).as_str());
            }
        }
        result
    }

    fn hash_from_input(hash_input: &HashInput) -> String {
        let mut hasher = Sha1::new();
        hasher.update(hash_input.as_bytes());
        let result = hasher.finalize();

        const HASH_LENGTH: usize = 7;

        let res = format!("{:x}", result);
        res[..HASH_LENGTH].to_string()
    }

    /// Compute the build string for a given variant
    pub fn from_variant(variant: &BTreeMap<NormalizedKey, Variable>, noarch: &NoArchType) -> Self {
        Self {
            hash: Self::hash_from_input(&HashInput::from_variant(variant)),
            prefix: Self::hash_prefix(variant, noarch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_hash() {
        let mut input = BTreeMap::new();
        input.insert("rust_compiler".into(), "rust".into());
        input.insert("build_platform".into(), "osx-64".into());
        input.insert("c_compiler".into(), "clang".into());
        input.insert("target_platform".into(), "osx-arm64".into());
        input.insert("openssl".into(), "3".into());
        input.insert(
            "CONDA_BUILD_SYSROOT".into(),
            "/Applications/Xcode_13.2.1.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX11.0.sdk".into()
        );
        input.insert("channel_targets".into(), "conda-forge main".into());
        input.insert("python".into(), "3.11.* *_cpython".into());
        input.insert("c_compiler_version".into(), "14".into());

        let build_string_from_output = HashInfo::from_variant(&input, &NoArchType::none());
        assert_eq!(build_string_from_output.to_string(), "py311h507f6e9");
    }
}
