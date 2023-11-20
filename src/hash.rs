//! Compute the build string for a given variant
use std::collections::{BTreeMap, HashMap};

use rattler_conda_types::NoArchType;
use serde::{Deserialize, Serialize};
use serde_json::ser::Formatter;
use sha1::{Digest, Sha1};

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

fn compute_hash_prefix(variant: &BTreeMap<String, String>, noarch: &NoArchType) -> String {
    if noarch.is_python() {
        return "py".to_string();
    }

    let mut map: HashMap<String, String> = HashMap::new();

    for (variant_key, version_spec) in variant.iter() {
        let prefix = match variant_key.as_str() {
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
            short_version_from_spec(version_spec, version_length),
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

fn hash_variant(variant: &BTreeMap<String, String>) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PythonFormatter {});

    // BTree has sorted keys, which is important for hashing
    variant
        .serialize(&mut ser)
        .expect("Failed to serialize input");

    let string = String::from_utf8(buf).expect("Failed to convert to string");

    let mut hasher = Sha1::new();
    hasher.update(string.as_bytes());
    let result = hasher.finalize();

    const HASH_LENGTH: usize = 7;

    let res = format!("{:x}", result);
    res[..HASH_LENGTH].to_string()
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct HashInfo {
    pub hash: String,
    pub hash_prefix: String,
}

impl std::fmt::Display for HashInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}h{}", self.hash_prefix, self.hash)
    }
}

impl Serialize for HashInfo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.to_string().as_str())
    }
}

impl<'de> Deserialize<'de> for HashInfo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        // split at `h` and take the first part
        if let Some((hash_prefix, hash)) = s.split_once('h') {
            return Ok(HashInfo {
                hash: hash.to_string(),
                hash_prefix: hash_prefix.to_string(),
            });
        }

        Err(serde::de::Error::custom(format!(
            "Failed to deserialize hash: {}",
            s
        )))
    }
}

pub fn compute_buildstring(variant: &BTreeMap<String, String>, noarch: &NoArchType) -> HashInfo {
    let hash_prefix = compute_hash_prefix(variant, noarch);
    let hash = hash_variant(variant);
    HashInfo { hash, hash_prefix }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_hash() {
        let mut input = BTreeMap::new();
        input.insert("rust_compiler".to_string(), "rust".to_string());
        input.insert("build_platform".to_string(), "osx-64".to_string());
        input.insert("c_compiler".to_string(), "clang".to_string());
        input.insert("target_platform".to_string(), "osx-arm64".to_string());
        input.insert("openssl".to_string(), "3".to_string());
        input.insert(
            "CONDA_BUILD_SYSROOT".to_string(),
            "/Applications/Xcode_13.2.1.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX11.0.sdk".to_string(),
        );
        input.insert(
            "channel_targets".to_string(),
            "conda-forge main".to_string(),
        );
        input.insert("python".to_string(), "3.11.* *_cpython".to_string());
        input.insert("c_compiler_version".to_string(), "14".to_string());

        let build_string_from_output = compute_buildstring(&input, &NoArchType::none());
        assert_eq!(build_string_from_output.to_string(), "py311h507f6e9");
    }
}
